use base64::Engine as _;
use image::{DynamicImage, ImageFormat};
use rayon::prelude::*;
use rayon::ThreadPoolBuilder;
use serde::{Deserialize, Serialize};
use std::io::{Cursor, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;
use tauri::{AppHandle, Emitter, State};
use walkdir::WalkDir;

use crate::commands::path_guard::PathWhitelist;
use crate::image::formats::{detect_format, DetectedFormat, InputFormat};
use crate::image::processor::{
    ImageProcessor, ProcessingOptions, ProcessingOptionsPayload, ProcessingResult,
};

/// Batch processing statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchStats {
    pub total_files: usize,
    pub processed_files: usize,
    pub successful_files: usize,
    pub failed_files: usize,
    pub total_original_size: u64,
    pub total_output_size: u64,
    pub overall_reduction_percent: f64,
    pub average_reduction_percent: f64,
    pub median_reduction_percent: f64,
}

/// Progress update event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProgressUpdate {
    pub current: usize,
    pub total: usize,
    pub current_file: String,
    pub percent: f64,
}

/// Destination for events sent to the frontend.
///
/// `tauri::Emitter` is sealed and an `AppHandle` cannot be built outside a
/// running app, so batch processing emits through this trait instead. That lets
/// the tests run a whole batch against an emitter that always fails.
pub trait EventEmitter {
    fn emit_event<S: Serialize + Clone>(&self, event: &str, payload: S) -> Result<(), String>;
}

impl EventEmitter for AppHandle {
    fn emit_event<S: Serialize + Clone>(&self, event: &str, payload: S) -> Result<(), String> {
        self.emit(event, payload).map_err(|e| e.to_string())
    }
}

/// Where messages about failed emissions go.
type LogSink<'a> = &'a (dyn Fn(String) + Sync);

/// Default log sink: stderr.
fn log_to_stderr(message: String) {
    eprintln!("[clt] {}", message);
}

/// Best-effort canonicalization of a single path.
///
/// Resolves symlinks and makes the path absolute (via `canonicalize`) so two
/// logically-identical inputs — different separators, a symlinked folder, or a
/// relative path — collapse to the same canonical ancestor during the
/// shared-parent search. When `canonicalize` fails (the file is missing, lives
/// on a network share, or lacks read permission) we fall back to the original
/// path, which keeps the function working for the common case.
fn normalize_path(path: &Path) -> PathBuf {
    let canon = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    // On Windows `canonicalize` prepends the `\\?\` verbatim prefix (and may
    // rewrite UNC paths as `\\?\UNC\`). Strip it so the result compares equal
    // to the non-canonicalized inputs used elsewhere and stays human-readable,
    // while keeping the symlink/separator resolution we actually want.
    #[cfg(windows)]
    let canon = {
        let s = canon.as_os_str().to_string_lossy();
        if let Some(rest) = s.strip_prefix(r"\\?\UNC\") {
            PathBuf::from(format!(r"\\{}", rest))
        } else if let Some(rest) = s.strip_prefix(r"\\?\") {
            PathBuf::from(rest.to_string())
        } else {
            canon
        }
    };
    canon
}

/// Case-insensitive comparison of two path components.
///
/// Windows filesystems are case-insensitive, so a path that was canonicalized
/// (real on-disk case) and one that fell back to its raw form (user-typed case)
/// can denote the same directory while differing in letter case. Comparison is
/// Unicode-aware lowercase over the lossy string — pragmatic for real names.
#[cfg(windows)]
fn os_components_eq(a: &std::ffi::OsStr, b: &std::ffi::OsStr) -> bool {
    a.to_string_lossy().to_lowercase() == b.to_string_lossy().to_lowercase()
}

/// On case-sensitive platforms component comparison is exact equality.
#[cfg(not(windows))]
fn os_components_eq(a: &std::ffi::OsStr, b: &std::ffi::OsStr) -> bool {
    a == b
}

/// Case-insensitive [`Path::starts_with`] (see [`os_components_eq`]).
fn starts_with_ci(path: &Path, prefix: &Path) -> bool {
    let prefix: Vec<_> = prefix.components().collect();
    let comps: Vec<_> = path.components().collect();
    comps.len() >= prefix.len()
        && comps
            .iter()
            .zip(prefix.iter())
            .all(|(c, p)| os_components_eq(c.as_os_str(), p.as_os_str()))
}

/// Case-insensitive component-wise path equality (see [`os_components_eq`]).
fn same_path_ci(a: &Path, b: &Path) -> bool {
    let ac: Vec<_> = a.components().collect();
    let bc: Vec<_> = b.components().collect();
    ac.len() == bc.len()
        && ac
            .iter()
            .zip(bc.iter())
            .all(|(x, y)| os_components_eq(x.as_os_str(), y.as_os_str()))
}

/// Case-insensitive [`Path::strip_prefix`] (see [`os_components_eq`]).
///
/// Returns the remainder of `path` after `prefix` when every prefix component
/// matches, or `None` when it does not — callers degrade to flattening.
fn strip_prefix_ci(path: &Path, prefix: &Path) -> Option<PathBuf> {
    let prefix: Vec<_> = prefix.components().collect();
    let comps: Vec<_> = path.components().collect();
    if comps.len() < prefix.len()
        || !comps
            .iter()
            .zip(prefix.iter())
            .all(|(c, p)| os_components_eq(c.as_os_str(), p.as_os_str()))
    {
        return None;
    }
    let mut rel = PathBuf::new();
    for c in &comps[prefix.len()..] {
        rel.push(c.as_os_str());
    }
    Some(rel)
}

/// Deepest directory that is an ancestor of every input file.
///
/// Used for `--mirror` output: `D:/pics/2024/a.jpg` and `D:/pics/2025/b.jpg`
/// share `D:/pics`, so outputs land in `<out>/2024/a.jpg` and `<out>/2025/b.jpg`.
/// Returns `None` when the only shared ancestor is a filesystem root (e.g.
/// files from two drives), in which case mirroring would recreate full absolute
/// paths and is skipped in favour of flattening.
///
/// Every input is canonicalized first (see [`normalize_path`]) so the search
/// compares resolved forms rather than raw strings that may differ only in
/// separators or symlink targets.
fn common_base_dir(paths: &[String]) -> Option<PathBuf> {
    if paths.is_empty() {
        return None;
    }
    // Normalize every input up front so the shared-ancestor search compares
    // canonical forms, not raw strings that may differ only in separators or
    // symlink targets.
    let norm: Vec<PathBuf> = paths.iter().map(|p| normalize_path(Path::new(p))).collect();
    let mut base = norm[0].parent()?;

    'outer: loop {
        for p in &norm {
            // The file must live strictly inside `base` for it to be a real
            // shared parent directory. The comparison is case-insensitive on
            // Windows: a mixed canonicalize outcome (one input resolved to
            // on-disk case, another falling back to user-typed case) must not
            // hide a shared parent that differs only in letter case.
            if !starts_with_ci(p, base) || same_path_ci(p, base) {
                // Walk one level up. Reaching a drive/filesystem root ends the
                // search — these inputs share no real parent directory, so
                // mirroring would recreate full absolute paths; the caller
                // falls back to flattening.
                base = base.parent()?;
                continue 'outer;
            }
        }
        return Some(base.to_path_buf());
    }
}

/// Emit an event, recording any failure instead of discarding it.
///
/// The failure is logged and processing continues: one lost notification must
/// not abort a batch that is already half done, but it must not be invisible
/// either - a dropped `processing-complete` is what used to leave the UI stuck
/// in the `processing` state.
fn emit_or_log<E, S>(emitter: &E, event: &str, payload: S, log: LogSink<'_>)
where
    E: EventEmitter,
    S: Serialize + Clone,
{
    if let Err(err) = emitter.emit_event(event, payload) {
        log(format!("failed to emit \"{}\" event: {}", event, err));
    }
}

/// Get list of image files from paths (supports files and directories).
///
/// Every requested path must be whitelisted (see [`PathWhitelist`]) — the
/// frontend registers dialog picks, drag-and-drop targets and paste files.
/// Runs on a blocking thread: walking a large tree must not stall the UI.
#[tauri::command]
pub async fn get_image_files(
    paths: Vec<String>,
    whitelist: State<'_, PathWhitelist>,
) -> Result<Vec<String>, String> {
    let whitelist = whitelist.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        for p in &paths {
            whitelist.ensure_allowed(Path::new(p))?;
        }
        get_image_files_impl(paths)
    })
    .await
    .map_err(|e| format!("File scan task failed: {}", e))?
}

pub fn get_image_files_impl(paths: Vec<String>) -> Result<Vec<String>, String> {
    let mut image_files = Vec::new();

    for path_str in paths {
        let path = Path::new(&path_str);

        if path.is_file() {
            if let Some(ext) = path.extension() {
                if InputFormat::is_supported(&ext.to_string_lossy()) {
                    image_files.push(path_str);
                }
            }
        } else if path.is_dir() {
            // Recursively walk directory.
            // NOTE: these commands run inside the Rust side and bypass Tauri's
            // `fs` capability scope — the path whitelist above is the check
            // that keeps the IPC surface scoped to user-picked roots. We do
            // NOT follow symlinks: a link pointing at `/etc`, `C:\Windows`,
            // etc. would otherwise be traversed and queued for processing
            // (DoS / privacy read). A recursion is still bounded by walkdir's
            // cycle detection, but links are intentionally not expanded.
            for entry in WalkDir::new(path)
                .follow_links(false)
                .into_iter()
                .filter_map(|e| e.ok())
            {
                let entry_path = entry.path();
                if entry_path.is_file() {
                    if let Some(ext) = entry_path.extension() {
                        if InputFormat::is_supported(&ext.to_string_lossy()) {
                            image_files.push(entry_path.to_string_lossy().to_string());
                        }
                    }
                }
            }
        }
    }

    Ok(image_files)
}

/// Cancellation epoch for batches.
///
/// [`process_batch_with`] captures the epoch when it starts; workers drop any
/// remaining file once the global epoch has moved on, and
/// [`cancel_processing`] bumps it. A monotonically increasing counter (rather
/// than a resettable bool) means a batch cancelled right before a new one
/// starts can never be resurrected: the old epoch stays invalid forever, no
/// matter how quickly the next batch begins.
static CANCEL_EPOCH: AtomicU64 = AtomicU64::new(0);

/// Ask the running batch to stop before the next file.
///
/// Files already in flight finish; queued files are dropped. Unlike the old
/// "visual only" cancel, real disk writes actually stop.
#[tauri::command]
pub fn cancel_processing() {
    CANCEL_EPOCH.fetch_add(1, Ordering::SeqCst);
}

/// Calculate optimal thread count for image processing
/// Limits parallelism to avoid I/O bottlenecks and excessive memory usage
fn calculate_optimal_threads() -> usize {
    let cpu_count = std::thread::available_parallelism()
        .map(|p| p.get())
        .unwrap_or(4);

    // Use half of available CPUs, with a minimum of 2 and maximum of 8
    // This prevents I/O saturation and reduces memory pressure for large images
    cpu_count.div_ceil(2).clamp(2, 8)
}

/// Estimated peak decoded memory the batch may hold at once. Each worker
/// thread can be mid-pipeline on one image; the budget bounds how many large
/// images run concurrently so a folder of panoramas cannot push the app into
/// swap or an OOM kill. The per-image estimate doubles the raw RGBA size to
/// account for the transient second copy that resize/rotation holds.
const BATCH_MEMORY_BUDGET_BYTES: u64 = 4 * 1024 * 1024 * 1024;

/// Process multiple images in batch.
///
/// The heavy lifting runs inside `spawn_blocking` (rayon handles the
/// parallelism); awaiting it keeps the async runtime worker free.
#[tauri::command]
pub async fn process_batch(
    app: AppHandle,
    input_paths: Vec<String>,
    output_dir: String,
    options: ProcessingOptionsPayload,
    whitelist: State<'_, PathWhitelist>,
) -> Result<BatchStats, String> {
    let whitelist = whitelist.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        for p in &input_paths {
            whitelist.ensure_allowed(Path::new(p))?;
        }
        whitelist.ensure_allowed(Path::new(&output_dir))?;
        // The frontend sends the flat wire format; convert it into the
        // canonical, grouped domain model at the IPC boundary.
        let options: ProcessingOptions = options.into();
        process_batch_with(&app, input_paths, output_dir, options, 0, &log_to_stderr)
    })
    .await
    .map_err(|e| format!("Batch task failed: {}", e))?
}

/// Export a previously collected batch result set as a CSV report.
///
/// The GUI receives per-file results through events; this lets the user save
/// them on demand. The same writer backs the CLI's `--report` flag.
///
/// The `path` comes from a save dialog, so its parent directory is a legitimate
/// user choice: it is whitelisted here (like any dialog-picked input) and the
/// write is then checked against the same whitelist as every other disk-touching
/// command. This closes the one gap where an arbitrary destination could be
/// written without going through the path guard.
#[tauri::command]
pub async fn export_report(
    records: Vec<ProcessingResult>,
    path: String,
    whitelist: State<'_, PathWhitelist>,
) -> Result<(), String> {
    let whitelist = whitelist.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        // The report file does not exist yet, so the location to validate is
        // its parent — the directory the user actually chose in the save
        // dialog.
        //
        // Check *before* registering: registering first and then calling
        // `ensure_allowed` on the same path is a tautology, since `register`
        // just added it to the whitelist. Checking first keeps the
        // credential-directory rule enforceable on this path.
        let path_buf = PathBuf::from(&path);
        let parent = path_buf
            .parent()
            .ok_or_else(|| "Invalid report path".to_string())?
            .to_path_buf();
        whitelist.ensure_allowed_tree(&parent)?;
        whitelist.register(&[parent.to_string_lossy().to_string()]);
        write_report_csv(&records, &path_buf)
    })
    .await
    .map_err(|e| format!("Report task failed: {}", e))?
}

/// Batch processing itself, generic over the event emitter and the log sink.
///
/// `pub` so the head-less CLI binary can drive the exact same pipeline.
pub fn process_batch_with<E: EventEmitter + Sync>(
    emitter: &E,
    input_paths: Vec<String>,
    output_dir: String,
    options: ProcessingOptions,
    jobs: usize,
    log: LogSink<'_>,
) -> Result<BatchStats, String> {
    let total_files = input_paths.len();
    let output_dir_path = PathBuf::from(&output_dir);

    // Ensure output directory exists
    std::fs::create_dir_all(&output_dir_path)
        .map_err(|e| format!("Failed to create output directory: {}", e))?;

    // When mirroring, find the deepest directory shared by every input file so
    // the relative tree can be recreated under `output_dir`. `None` means
    // "flatten" (the previous behaviour) — e.g. when inputs share no parent.
    let mirror_base = if options.output.mirror_subdirs {
        common_base_dir(&input_paths)
    } else {
        None
    };

    // The watermark is decoded / rasterized once per batch, not once per file.
    let watermark = ImageProcessor::prepare_watermark(&options).map(Arc::new);

    // Calculate optimal thread count to balance CPU and I/O. `jobs > 0` lets
    // the CLI override the auto-tuned value.
    //
    // Batch memory cap: probe every input's header (cheap — no pixels are
    // decoded) and clamp the worker count so the estimated in-flight decoded
    // bytes stay inside the budget. A folder of 100 MP panoramas on an
    // 8-core machine would otherwise decode eight of them simultaneously.
    // Explicit `--jobs` still wins, but never past the memory clamp: an OOM
    // kill is worse than a slower batch.
    let max_est_bytes: u64 = input_paths
        .par_iter()
        .filter_map(|p| {
            ImageProcessor::probe_dimensions(Path::new(p))
                .map(|(w, h)| u64::from(w) * u64::from(h) * 4 * 2)
        })
        .max()
        .unwrap_or(0);
    // `checked_div` yields None when every probe failed (max_est_bytes == 0):
    // nothing known about sizes, so no clamp is applied.
    let memory_clamp = match BATCH_MEMORY_BUDGET_BYTES.checked_div(max_est_bytes) {
        Some(quotient) => quotient.max(1) as usize,
        None => usize::MAX,
    };
    let num_threads = if jobs > 0 {
        jobs.clamp(1, 64).min(memory_clamp)
    } else {
        calculate_optimal_threads().min(memory_clamp)
    };

    // Create a custom thread pool with limited parallelism
    let pool = ThreadPoolBuilder::new()
        .num_threads(num_threads)
        .build()
        .map_err(|e| format!("Failed to create thread pool: {}", e))?;

    // The epoch this batch started in. Workers compare against it per file:
    // a cancel (or any later bump) invalidates the remaining queue. The CLI
    // never bumps the epoch, so head-less batches are never cancelled.
    let cancel_epoch = CANCEL_EPOCH.load(Ordering::SeqCst);

    // Use atomic counter for accurate progress tracking across threads
    let processed_count = Arc::new(AtomicUsize::new(0));

    // Process images in parallel with controlled concurrency.
    // Cancelled inputs yield None and are excluded from results/stats.
    let results: Vec<ProcessingResult> = pool.install(|| {
        input_paths
            .par_iter()
            .enumerate()
            .filter_map(|(_index, input_path)| {
                if CANCEL_EPOCH.load(Ordering::SeqCst) != cancel_epoch {
                    return None;
                }

                // Process the image (naming, conflicts and skip-when-larger
                // are handled inside the processor).
                let file_out_dir = match &mirror_base {
                    Some(base) => {
                        // Normalize the input path too, so the relative strip
                        // matches the canonicalized `base` even when the frontend
                        // handed us a relative or differently-separated path.
                        let norm = normalize_path(Path::new(input_path));
                        // A failed strip means `base` no longer matches this
                        // path's canonical form; fall back to flattening into
                        // the output root. Never join the absolute `norm` —
                        // `PathBuf::join` would discard `output_dir_path` and
                        // send outputs next to the originals.
                        // Case-insensitive strip: `base` may come from a
                        // canonicalized input (on-disk case) while this path
                        // fell back to its raw form (user-typed case) — on
                        // Windows they are the same directory.
                        match strip_prefix_ci(&norm, base) {
                            Some(rel) => output_dir_path
                                .join(rel)
                                .parent()
                                .map(|p| p.to_path_buf())
                                .unwrap_or_else(|| output_dir_path.clone()),
                            None => output_dir_path.clone(),
                        }
                    }
                    None => output_dir_path.clone(),
                };
                // Mirroring recreates the input tree under the output dir, so
                // the per-file directory may not exist yet. Create it up front
                // (creating an existing dir is a no-op) so the encode write
                // never hits a "path not found".
                if let Err(e) = std::fs::create_dir_all(&file_out_dir) {
                    log(format!(
                        "failed to create output dir {}: {}",
                        file_out_dir.display(),
                        e
                    ));
                }
                let result = match ImageProcessor::process_image_with(
                    input_path,
                    &file_out_dir,
                    &options,
                    watermark.as_deref(),
                ) {
                    Ok(result) => result,
                    Err(e) => ProcessingResult {
                        original_path: input_path.clone(),
                        output_path: String::new(),
                        original_size: 0,
                        output_size: 0,
                        reduction_percent: 0.0,
                        success: false,
                        error: Some(e.to_string()),
                        skipped: false,
                        within_target: true,
                    },
                };

                // Update progress counter atomically
                let current = processed_count.fetch_add(1, Ordering::SeqCst) + 1;

                // Emit progress update
                emit_or_log(
                    emitter,
                    "processing-progress",
                    ProgressUpdate {
                        current,
                        total: total_files,
                        current_file: input_path.clone(),
                        percent: (current as f64 / total_files as f64) * 100.0,
                    },
                    log,
                );

                // Emit individual file result
                emit_or_log(emitter, "processing-result", &result, log);

                Some(result)
            })
            .collect()
    });

    // Calculate statistics
    let stats = calculate_batch_stats(&results);

    // Emit completion event
    emit_or_log(emitter, "processing-complete", &stats, log);

    Ok(stats)
}

/// Calculate batch processing statistics
fn calculate_batch_stats(results: &[ProcessingResult]) -> BatchStats {
    let total_files = results.len();
    let successful_results: Vec<&ProcessingResult> = results.iter().filter(|r| r.success).collect();

    let successful_files = successful_results.len();
    let failed_files = total_files - successful_files;

    let total_original_size: u64 = successful_results.iter().map(|r| r.original_size).sum();

    let total_output_size: u64 = successful_results.iter().map(|r| r.output_size).sum();

    let overall_reduction_percent = if total_original_size > 0 {
        ((total_original_size as f64 - total_output_size as f64) / total_original_size as f64)
            * 100.0
    } else {
        0.0
    };

    let mut reductions: Vec<f64> = successful_results
        .iter()
        .map(|r| r.reduction_percent)
        .collect();

    let average_reduction_percent = if !reductions.is_empty() {
        reductions.iter().sum::<f64>() / reductions.len() as f64
    } else {
        0.0
    };

    let median_reduction_percent = if !reductions.is_empty() {
        reductions.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let mid = reductions.len() / 2;
        if reductions.len().is_multiple_of(2) {
            (reductions[mid - 1] + reductions[mid]) / 2.0
        } else {
            reductions[mid]
        }
    } else {
        0.0
    };

    BatchStats {
        total_files,
        processed_files: total_files,
        successful_files,
        failed_files,
        total_original_size,
        total_output_size,
        overall_reduction_percent,
        average_reduction_percent,
        median_reduction_percent,
    }
}

/// Write a processing report as CSV.
///
/// One row per processed file with the columns a user actually wants to audit:
/// source, destination, sizes, reduction, success/skip/target flags and any
/// error. Fields containing commas, quotes or newlines are quoted (RFC 4180).
///
/// Text fields that could be interpreted as spreadsheet formulas are
/// neutralized: a cell starting with `=`, `+`, `-`, `@`, TAB or CR makes
/// Excel / LibreOffice treat the rest as a formula or macro invocation. A file
/// named `=HYPERLINK(...)` must not become an executable cell when the report
/// is opened, so such fields get the standard single-quote prefix.
pub fn write_report_csv(results: &[ProcessingResult], path: &Path) -> Result<(), String> {
    let mut file =
        std::fs::File::create(path).map_err(|e| format!("Failed to create report: {}", e))?;
    writeln!(
        file,
        "original_path,output_path,original_size,output_size,reduction_percent,success,skipped,within_target,error"
    )
    .map_err(|e| e.to_string())?;

    for r in results {
        let escape = |s: &str| -> String {
            // Neutralize formula injection first, then apply RFC 4180 quoting
            // to whatever the sanitized text turned out to be.
            const FORMULA_PREFIXES: [char; 6] = ['=', '+', '-', '@', '\t', '\r'];
            let sanitized = if s
                .chars()
                .next()
                .is_some_and(|c| FORMULA_PREFIXES.contains(&c))
            {
                format!("'{s}")
            } else {
                s.to_string()
            };
            if sanitized.contains(',') || sanitized.contains('"') || sanitized.contains('\n') {
                format!("\"{}\"", sanitized.replace('"', "\"\""))
            } else {
                sanitized
            }
        };
        let error = r.error.as_deref().unwrap_or("");
        writeln!(
            file,
            "{},{},{},{},{:.2},{},{},{},{}",
            escape(&r.original_path),
            escape(&r.output_path),
            r.original_size,
            r.output_size,
            r.reduction_percent,
            r.success,
            r.skipped,
            r.within_target,
            escape(error),
        )
        .map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Information about a file's real image type, detected from its signature.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileTypeInfo {
    /// Absolute path of the file
    pub path: String,
    /// Lowercase file extension without the leading dot (empty if none)
    pub extension: String,
    /// Format detected from magic bytes
    pub detected_format: DetectedFormat,
    /// MIME type of the detected format
    pub detected_mime: String,
    /// Whether the extension matches the detected format
    pub matches_extension: bool,
    /// Pixel width if the image could be decoded (None for unsupported containers)
    pub width: Option<u32>,
    /// Pixel height if the image could be decoded
    pub height: Option<u32>,
    /// File size in bytes
    pub size_bytes: u64,
}

/// At most this many bytes are read for signature detection. Every signature
/// checked by `detect_format` lives in the first bytes of the file; reading a
/// whole multi-hundred-MB image just to sniff a header is pure waste.
const HEADER_SNIFF_BYTES: u64 = 64 * 1024;

/// Detect the real type of one image file from its signature bytes.
///
/// The file does not need to be decodable: container formats (HEIC/AVIF/JXL)
/// whose pixels the `image` crate cannot read are still recognised, and a
/// lying extension is flagged. This is the shared implementation behind the
/// batch command (see [`detect_file_types`]); there is deliberately no
/// single-file IPC command — one round-trip per drop is all the frontend
/// needs.
fn detect_file_type_impl(path_str: String) -> Result<FileTypeInfo, String> {
    let path = Path::new(&path_str);

    let metadata = std::fs::metadata(path).map_err(|e| format!("Failed to read file: {}", e))?;
    let size_bytes = metadata.len();

    let extension = path
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();

    // Only the first bytes are needed for signature detection.
    let mut header = Vec::new();
    std::fs::File::open(path)
        .map_err(|e| format!("Failed to read file: {}", e))?
        .take(HEADER_SNIFF_BYTES)
        .read_to_end(&mut header)
        .map_err(|e| format!("Failed to read file: {}", e))?;
    let detected = detect_format(&header);

    let matches_extension = match detected.canonical_extension() {
        Some(ext) => extension == ext,
        None => false,
    };

    // Best-effort dimensions; unsupported containers (HEIC/AVIF) yield None.
    // `into_dimensions` decodes the header only, never the pixels. Note that
    // the reported dimensions are the on-disk orientation (EXIF, if any, is
    // NOT applied here).
    let (width, height) = match ImageProcessor::probe_dimensions(path) {
        Some((w, h)) => (Some(w), Some(h)),
        None => (None, None),
    };

    Ok(FileTypeInfo {
        path: path_str,
        extension,
        detected_format: detected,
        detected_mime: detected.mime().to_string(),
        matches_extension,
        width,
        height,
        size_bytes,
    })
}

/// One entry of the batch `detect_file_types` command: either the detected
/// info or an error message. Best-effort by design — one unreadable or
/// disallowed file must not fail the whole drop.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileTypeDetectResult {
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub info: Option<FileTypeInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Batch type detection: one IPC round-trip for a whole drop instead of one
/// per file. Files are probed in parallel; every entry reports its own
/// success or failure.
#[tauri::command]
pub async fn detect_file_types(
    paths: Vec<String>,
    whitelist: State<'_, PathWhitelist>,
) -> Result<Vec<FileTypeDetectResult>, String> {
    let whitelist = whitelist.inner().clone();
    tauri::async_runtime::spawn_blocking(move || detect_file_types_impl(paths, &whitelist))
        .await
        .map_err(|e| format!("Detect task failed: {}", e))
}

fn detect_file_types_impl(
    paths: Vec<String>,
    whitelist: &PathWhitelist,
) -> Vec<FileTypeDetectResult> {
    paths
        .into_par_iter()
        .map(|path| match whitelist.ensure_allowed(Path::new(&path)) {
            Ok(()) => match detect_file_type_impl(path.clone()) {
                Ok(info) => FileTypeDetectResult {
                    path,
                    info: Some(info),
                    error: None,
                },
                Err(e) => FileTypeDetectResult {
                    path,
                    info: None,
                    error: Some(e),
                },
            },
            Err(e) => FileTypeDetectResult {
                path,
                info: None,
                error: Some(e),
            },
        })
        .collect()
}

/// Before/after preview for the currently selected file (Squoosh-style).
///
/// `after_size` is a REAL size: the processed image is encoded with the same
/// pipeline as the batch output, in memory. No formulas, no estimates.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreviewData {
    /// Downscaled original, PNG, base64 (display only)
    pub before_base64: String,
    /// Downscaled result, PNG, base64 (display only)
    pub after_base64: String,
    /// Real size of the original file
    pub before_size: u64,
    /// Real size of the encoded result
    pub after_size: u64,
    /// Final pixel width after the pipeline
    pub width: u32,
    /// Final pixel height after the pipeline
    pub height: u32,
}

/// Largest side of the preview display images.
const PREVIEW_MAX_SIDE: u32 = 512;

fn preview_base64(img: &DynamicImage) -> String {
    let display = if img.width() > PREVIEW_MAX_SIDE || img.height() > PREVIEW_MAX_SIDE {
        img.resize(
            PREVIEW_MAX_SIDE,
            PREVIEW_MAX_SIDE,
            image::imageops::FilterType::Lanczos3,
        )
    } else {
        img.clone()
    };
    let mut cursor = Cursor::new(Vec::new());
    // Preview is best-effort; an encoding failure yields an empty image
    // rather than failing the whole panel.
    if display.write_to(&mut cursor, ImageFormat::Png).is_ok() {
        base64::engine::general_purpose::STANDARD.encode(cursor.into_inner())
    } else {
        String::new()
    }
}

/// Generate a before/after preview with exact output size estimation.
///
/// Async + `spawn_blocking` so dragging a quality slider re-encodes off the
/// main thread and the UI keeps animating.
#[tauri::command]
pub async fn preview_image(
    path: String,
    options: ProcessingOptionsPayload,
    whitelist: State<'_, PathWhitelist>,
) -> Result<PreviewData, String> {
    let whitelist = whitelist.inner().clone();
    let options: ProcessingOptions = options.into();
    tauri::async_runtime::spawn_blocking(move || {
        whitelist.ensure_allowed(Path::new(&path))?;
        preview_image_impl(&path, &options)
    })
    .await
    .map_err(|e| format!("Preview task failed: {}", e))?
}

/// Single-entry cache of the preview source bytes. Dragging the quality
/// slider re-renders the same file many times a second, and re-reading a
/// large photo from disk on every tick is pure waste. The entry is keyed by
/// path and validated against mtime + size, so an edited file is re-read;
/// only files up to [`PREVIEW_SOURCE_CACHE_MAX_BYTES`] are cached to bound
/// memory.
const PREVIEW_SOURCE_CACHE_MAX_BYTES: u64 = 64 * 1024 * 1024;

struct PreviewSourceCache {
    path: PathBuf,
    modified: Option<SystemTime>,
    len: u64,
    bytes: Arc<Vec<u8>>,
}

static PREVIEW_SOURCE_CACHE: Mutex<Option<PreviewSourceCache>> = Mutex::new(None);

fn read_preview_source(path: &Path) -> Result<Arc<Vec<u8>>, String> {
    let meta = std::fs::metadata(path).map_err(|e| format!("Failed to read file: {}", e))?;
    let modified = meta.modified().ok();
    let len = meta.len();

    {
        let cached = PREVIEW_SOURCE_CACHE
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if let Some(c) = cached.as_ref() {
            if c.path == path && c.len == len && c.modified == modified {
                return Ok(c.bytes.clone());
            }
        }
    }

    let bytes = Arc::new(std::fs::read(path).map_err(|e| format!("Failed to read file: {}", e))?);
    if len <= PREVIEW_SOURCE_CACHE_MAX_BYTES {
        let mut cached = PREVIEW_SOURCE_CACHE
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        *cached = Some(PreviewSourceCache {
            path: path.to_path_buf(),
            modified,
            len,
            bytes: bytes.clone(),
        });
    }
    Ok(bytes)
}

/// Shared preview pipeline: decode → apply → encode → [`PreviewData`].
///
/// `preview_image` (file path) and `preview_image_data` (base64 template)
/// differ only in where the source bytes come from; everything downstream is
/// identical, so it lives here.
fn render_preview(bytes: &[u8], options: &ProcessingOptions) -> Result<PreviewData, String> {
    let before_size = bytes.len() as u64;

    // Orientation-aware decode: the preview must match what the batch run
    // will write (both bake EXIF orientation in before the pipeline).
    let img = ImageProcessor::decode_bytes_with_orientation(bytes)
        .map_err(|e| format!("Failed to decode image: {}", e))?;

    let watermark = ImageProcessor::prepare_watermark(options);
    let processed = ImageProcessor::apply_pipeline(img.clone(), options, watermark.as_ref());
    let (width, height) = (processed.width(), processed.height());

    let after_bytes =
        ImageProcessor::encode_to_memory(&processed, options).map_err(|e| e.to_string())?;
    let after_size = after_bytes.len() as u64;

    Ok(PreviewData {
        before_base64: preview_base64(&img),
        after_base64: preview_base64(&processed),
        before_size,
        after_size,
        width,
        height,
    })
}

fn preview_image_impl(path: &str, options: &ProcessingOptions) -> Result<PreviewData, String> {
    let bytes = read_preview_source(Path::new(path))?;
    render_preview(&bytes, options)
}

/// Same as `preview_image`, but the source is a base64-encoded image instead of
/// a file path. Used to preview effects on a built-in template image when the
/// user has not selected any file yet.
#[tauri::command]
pub async fn preview_image_data(
    input_base64: String,
    options: ProcessingOptionsPayload,
) -> Result<PreviewData, String> {
    // Base64 input has no filesystem path, hence no whitelist check.
    let options: ProcessingOptions = options.into();
    tauri::async_runtime::spawn_blocking(move || preview_image_data_impl(&input_base64, &options))
        .await
        .map_err(|e| format!("Preview task failed: {}", e))?
}

fn preview_image_data_impl(
    input_base64: &str,
    options: &ProcessingOptions,
) -> Result<PreviewData, String> {
    // Tolerate a `data:image/...;base64,` prefix if the caller included one.
    let raw = input_base64
        .split_once("base64,")
        .map(|(_, b)| b)
        .unwrap_or(input_base64);
    // Same IPC input bound as the paste path: cap the decode-time allocation.
    super::ensure_base64_input_bounded(raw)?;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(raw.trim())
        .map_err(|e| format!("Failed to decode base64: {}", e))?;
    render_preview(&bytes, options)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicU32;
    use std::sync::Mutex;

    /// Emitter that always fails, standing in for a webview that went away
    /// mid-batch.
    struct FailingEmitter;

    impl EventEmitter for FailingEmitter {
        fn emit_event<S: Serialize + Clone>(
            &self,
            _event: &str,
            _payload: S,
        ) -> Result<(), String> {
            Err("webview channel closed".to_string())
        }
    }

    struct OkEmitter;

    impl EventEmitter for OkEmitter {
        fn emit_event<S: Serialize + Clone>(
            &self,
            _event: &str,
            _payload: S,
        ) -> Result<(), String> {
            Ok(())
        }
    }

    static TEST_DIR_COUNTER: AtomicU32 = AtomicU32::new(0);

    /// Create a unique scratch directory for one test.
    fn scratch_dir(name: &str) -> PathBuf {
        let id = TEST_DIR_COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!("clt-{}-{}-{}", name, std::process::id(), id));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("failed to create scratch dir");
        dir
    }

    /// Write a small valid PNG and return its path.
    fn write_png(dir: &Path, name: &str) -> String {
        let path = dir.join(name);
        image::RgbaImage::from_pixel(8, 8, image::Rgba([12, 34, 56, 255]))
            .save(&path)
            .expect("failed to write test png");
        path.to_string_lossy().to_string()
    }

    #[test]
    fn emit_failure_is_logged_with_event_name_and_cause() {
        let logged = Mutex::new(Vec::new());

        emit_or_log(&FailingEmitter, "processing-complete", (), &|message| {
            logged.lock().unwrap().push(message)
        });

        let logged = logged.into_inner().unwrap();
        assert_eq!(
            logged.len(),
            1,
            "emit failure must leave exactly one record"
        );
        assert!(
            logged[0].contains("processing-complete"),
            "log must name the event: {:?}",
            logged[0]
        );
        assert!(
            logged[0].contains("webview channel closed"),
            "log must carry the cause: {:?}",
            logged[0]
        );
    }

    #[test]
    fn successful_emit_logs_nothing() {
        let logged = Mutex::new(Vec::new());

        emit_or_log(&OkEmitter, "processing-progress", (), &|message| {
            logged.lock().unwrap().push(message)
        });

        assert!(logged.into_inner().unwrap().is_empty());
    }

    #[test]
    fn batch_finishes_and_logs_every_failed_emit() {
        let dir = scratch_dir("batch-emit-failure");
        let input_dir = dir.join("input");
        std::fs::create_dir_all(&input_dir).expect("failed to create input dir");
        let inputs = vec![
            write_png(&input_dir, "a.png"),
            write_png(&input_dir, "b.png"),
        ];
        let output_dir = dir.join("output").to_string_lossy().to_string();
        let logged = Mutex::new(Vec::new());

        let stats = process_batch_with(
            &FailingEmitter,
            inputs,
            output_dir,
            ProcessingOptions::default(),
            0,
            &|message| logged.lock().unwrap().push(message),
        )
        .expect("batch must finish even when every emit fails");

        // Failing notifications must not take the batch down with them.
        assert_eq!(stats.total_files, 2);
        assert_eq!(stats.successful_files, 2);
        assert_eq!(stats.failed_files, 0);

        // ...but each failure has to be traceable to the event it belongs to.
        let logged = logged.into_inner().unwrap();
        for event in [
            "processing-progress",
            "processing-result",
            "processing-complete",
        ] {
            assert!(
                logged.iter().any(|message| message.contains(event)),
                "no log for failed \"{}\" emit: {:?}",
                event,
                logged
            );
        }

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn common_base_dir_finds_shared_parent() {
        // Drive letters and separators differ per OS, so derive the inputs from
        // the system temp dir. `common_base_dir` works on the paths directly
        // (via `normalize_path`, which falls back to the raw path when a file is
        // absent), so the shared-ancestor search is OS-agnostic.
        let base = std::env::temp_dir().join("clt-cbd-test");

        // Files under two sub-folders of the same tree share that tree.
        let inputs = vec![
            base.join("2024/a.jpg").to_string_lossy().to_string(),
            base.join("2025/b.png").to_string_lossy().to_string(),
        ];
        assert_eq!(common_base_dir(&inputs), Some(base.clone()));

        // All files in one directory -> that directory is the base (mirror of a
        // single flat folder is just the folder itself).
        let flat = vec![
            base.join("a.jpg").to_string_lossy().to_string(),
            base.join("b.jpg").to_string_lossy().to_string(),
        ];
        assert_eq!(common_base_dir(&flat), Some(base.clone()));
    }

    // Only Windows can have two paths with genuinely no common ancestor (distinct
    // drive letters). `normalize_path` keeps the raw drive form when the files
    // are absent, so the lexical search sees separate roots even without a real
    // `D:` volume mounted.
    #[cfg(windows)]
    #[test]
    fn common_base_dir_no_shared_parent_across_drives() {
        let mixed = vec!["C:/a/x.jpg".to_string(), "D:/b/y.jpg".to_string()];
        assert!(common_base_dir(&mixed).is_none());
    }

    #[cfg(windows)]
    #[test]
    fn common_base_dir_is_case_insensitive_on_windows() {
        // A mixed canonicalize outcome — one input resolved to real on-disk
        // case, another falling back to raw user-typed case — must still find
        // the shared parent: Windows filesystems are case-insensitive.
        let inputs = vec![
            "D:/Pics/2024/a.jpg".to_string(),
            "d:/pics/2025/b.png".to_string(),
        ];
        assert_eq!(common_base_dir(&inputs), Some(PathBuf::from("D:/Pics")));
    }

    #[test]
    fn strip_prefix_ci_matches_platform_case_semantics() {
        let base = Path::new("C:/Users/me/Pics");
        let path = Path::new("c:/users/me/pics/2024/a.png");
        #[cfg(windows)]
        assert_eq!(
            strip_prefix_ci(path, base),
            Some(PathBuf::from("2024").join("a.png"))
        );
        // On case-sensitive platforms the helper is exact equality: these two
        // paths are genuinely different, so nothing may be stripped.
        #[cfg(not(windows))]
        assert_eq!(strip_prefix_ci(path, base), None);
    }

    #[test]
    fn common_base_dir_resolves_real_paths_via_canonicalize() {
        // The canonicalize branch (used for real, existing files) must agree
        // with the lexical fallback: symlinks/separators are normalized before
        // the shared-ancestor search. Exercises `normalize_path` directly.
        let dir = scratch_dir("common-base-real");
        let a = dir.join("sub").join("a.jpg");
        let b = dir.join("sub").join("b.png");
        std::fs::create_dir_all(a.parent().unwrap()).unwrap();
        std::fs::write(&a, b"x").unwrap();
        std::fs::write(&b, b"y").unwrap();

        let inputs = vec![
            a.to_string_lossy().to_string(),
            b.to_string_lossy().to_string(),
        ];
        assert_eq!(common_base_dir(&inputs), Some(dir.join("sub")));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn detect_file_types_batch_reports_info_and_per_file_errors() {
        let dir = scratch_dir("detect-batch");
        let png = write_png(&dir, "a.png");
        let garbage = dir.join("junk.bin");
        std::fs::write(&garbage, b"definitely not an image").unwrap();

        let wl = PathWhitelist::default();
        wl.register(&[dir.to_string_lossy().to_string()]);

        // NOT whitelisted: must come back as a per-file error, not fail the
        // whole batch.
        let outside = std::env::temp_dir().join("clt-detect-outside.png");

        let results = detect_file_types_impl(
            vec![
                png,
                garbage.to_string_lossy().to_string(),
                outside.to_string_lossy().to_string(),
            ],
            &wl,
        );

        assert_eq!(results.len(), 3);
        // Real PNG: full info, no error.
        let info = results[0].info.as_ref().expect("png must detect");
        assert!(info.width.is_some() && info.height.is_some());
        assert!(results[0].error.is_none());
        // Garbage bytes: detection still succeeds per file (type unknown).
        assert!(results[1].info.is_some());
        assert!(results[1].error.is_none());
        // Outside the whitelist: per-file error, no info.
        assert!(results[2].info.is_none());
        assert!(results[2].error.is_some());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn preview_source_cache_invalidates_on_change() {
        let dir = scratch_dir("preview-cache");
        let p = write_png(&dir, "a.png");

        let first = read_preview_source(Path::new(&p)).unwrap();
        // Same path + unchanged mtime/size -> the cached bytes are reused.
        assert!(Arc::ptr_eq(
            &first,
            &read_preview_source(Path::new(&p)).unwrap()
        ));

        // Rewrite with different content and a bumped mtime -> re-read.
        std::thread::sleep(std::time::Duration::from_millis(20));
        std::fs::write(&p, b"changed content").unwrap();
        let second = read_preview_source(Path::new(&p)).unwrap();
        assert!(!Arc::ptr_eq(&first, &second));
        assert_eq!(second.as_slice(), &b"changed content"[..]);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn write_report_csv_produces_escaped_rows() {
        let dir = scratch_dir("report-csv");
        let path = dir.join("report.csv");
        let results = vec![
            ProcessingResult {
                original_path: "a.png".into(),
                output_path: "a.webp".into(),
                original_size: 1000,
                output_size: 500,
                reduction_percent: 50.0,
                success: true,
                error: None,
                skipped: false,
                within_target: true,
            },
            ProcessingResult {
                original_path: "b.png".into(),
                output_path: String::new(),
                original_size: 200,
                output_size: 200,
                reduction_percent: 0.0,
                success: true,
                error: Some("oops, comma".into()),
                skipped: true,
                within_target: true,
            },
        ];
        write_report_csv(&results, &path).expect("write csv");

        let content = std::fs::read_to_string(&path).unwrap();
        let lines: Vec<&str> = content.lines().collect();
        assert_eq!(lines.len(), 3, "header + 2 rows");
        assert!(lines[1].starts_with("a.png,"));
        // A comma inside the error field must be quoted (RFC 4180).
        assert!(lines[2].contains("\"oops, comma\""), "got: {}", lines[2]);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn csv_formula_prefixes_are_neutralized() {
        // Every spreadsheet-dangerous prefix must come out carrying the
        // neutralizing single quote, so opening the report in Excel or
        // LibreOffice cannot execute a crafted filename as a formula.
        let dir = scratch_dir("report-csv-formula");
        let path = dir.join("report.csv");
        let mk = |original: &str, output: &str, error: &str| ProcessingResult {
            original_path: original.into(),
            output_path: output.into(),
            original_size: 10,
            output_size: 10,
            reduction_percent: 0.0,
            success: true,
            error: Some(error.into()),
            skipped: false,
            within_target: true,
        };
        let results = vec![
            mk(
                "=HYPERLINK(\"http://evil\",\"click\")",
                "+cmd|' /C calc",
                "@import",
            ),
            mk("-dash.png", "\tstart", "\rCR"),
        ];
        write_report_csv(&results, &path).expect("write csv");

        let content = std::fs::read_to_string(&path).unwrap();
        for neutralized in [
            "'=HYPERLINK",
            "'+cmd",
            "'@import",
            "'-dash.png",
            "'\tstart",
            "'\rCR",
        ] {
            assert!(
                content.contains(neutralized),
                "must contain {neutralized:?}; got: {content}"
            );
        }

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn preview_image_data_rejects_oversized_base64() {
        // One byte over the IPC input bound must fail before any decode
        // allocation happens — the reverse of the compliant-size path.
        let options = ProcessingOptions::default();
        let oversized = "A".repeat(super::super::MAX_BASE64_IMAGE_INPUT + 1);
        let err = preview_image_data_impl(&oversized, &options)
            .expect_err("over-limit payload must be rejected");
        assert!(err.contains("too large"), "got: {err}");
    }

    #[test]
    fn mirror_subdirs_recreates_input_tree() {
        // Regression: `--mirror` must create the per-file output directories,
        // not only the top-level output dir. Without that, the encode write
        // hit "path not found" (os error 3) on Windows because the nested
        // output folder did not exist yet.
        let dir = scratch_dir("mirror-tree");
        let input_dir = dir.join("input");
        for sub in ["2024", "2025"] {
            std::fs::create_dir_all(input_dir.join(sub)).unwrap();
        }
        write_png(&input_dir.join("2024"), "a.png");
        write_png(&input_dir.join("2024"), "b.png");
        write_png(&input_dir.join("2025"), "c.png");

        let output_dir = dir.join("output").to_string_lossy().to_string();
        let opts = ProcessingOptions {
            output: crate::image::processor::OutputOptions {
                mirror_subdirs: true,
                ..Default::default()
            },
            ..Default::default()
        };

        let inputs = vec![
            input_dir.join("2024/a.png").to_string_lossy().to_string(),
            input_dir.join("2024/b.png").to_string_lossy().to_string(),
            input_dir.join("2025/c.png").to_string_lossy().to_string(),
        ];

        let stats = process_batch_with(&OkEmitter, inputs, output_dir, opts, 0, &|_| {})
            .expect("batch must finish");

        assert_eq!(stats.total_files, 3);
        assert_eq!(stats.successful_files, 3, "mirrored files must be written");

        // The input tree must be recreated under the output directory.
        assert!(dir.join("output/2024/a.webp").exists());
        assert!(dir.join("output/2024/b.webp").exists());
        assert!(dir.join("output/2025/c.webp").exists());

        let _ = std::fs::remove_dir_all(&dir);
    }
}
