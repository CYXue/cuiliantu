//! Batch processing: the runner, its event channel and the statistics.
//!
//! Split out of the former `image_commands.rs` monolith. `process_batch_with`
//! is generic over [`EventEmitter`] so the head-less CLI drives the exact same
//! pipeline as the GUI.

use rayon::prelude::*;
use rayon::ThreadPoolBuilder;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use tauri::{AppHandle, Emitter, State};
use walkdir::WalkDir;

use super::path_guard::PathWhitelist;
use super::paths::{common_base_dir, normalize_path, strip_prefix_ci};
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

/// Everything the batch runner emits, as a typed enum.
///
/// This replaces the old `(event_name, S: Serialize)` pair: implementors now
/// receive the payload already typed, so the CLI reads it with a plain
/// `match` instead of a `serde_json::Value` round-trip (which serialized AND
/// deserialized every event), while the GUI side serializes exactly once.
#[derive(Debug, Clone)]
pub enum BatchEvent {
    /// Throttled batch progress.
    Progress(ProgressUpdate),
    /// One file's outcome.
    Result(ProcessingResult),
    /// Batch summary.
    Complete(BatchStats),
}

impl BatchEvent {
    /// The frontend channel this event is emitted on (the webview listens on
    /// these exact names).
    pub fn channel(&self) -> &'static str {
        match self {
            BatchEvent::Progress(_) => "processing-progress",
            BatchEvent::Result(_) => "processing-result",
            BatchEvent::Complete(_) => "processing-complete",
        }
    }
}

/// Destination for events sent to the frontend.
///
/// `tauri::Emitter` is sealed and an `AppHandle` cannot be built outside a
/// running app, so batch processing emits through this trait instead. That lets
/// the tests run a whole batch against an emitter that always fails.
pub trait EventEmitter {
    fn emit_event(&self, event: BatchEvent) -> Result<(), String>;
}

impl EventEmitter for AppHandle {
    fn emit_event(&self, event: BatchEvent) -> Result<(), String> {
        // Serialize the concrete payload (not the enum wrapper) so the
        // webview keeps seeing the exact shapes it always did.
        let payload = match &event {
            BatchEvent::Progress(p) => serde_json::to_value(p),
            BatchEvent::Result(r) => serde_json::to_value(r),
            BatchEvent::Complete(s) => serde_json::to_value(s),
        }
        .map_err(|e| e.to_string())?;
        self.emit(event.channel(), payload)
            .map_err(|e| e.to_string())
    }
}

/// Where messages about failed emissions go.
type LogSink<'a> = &'a (dyn Fn(String) + Sync);

/// Default log sink: stderr.
pub fn log_to_stderr(message: String) {
    eprintln!("[clt] {}", message);
}

/// Emit an event, recording any failure instead of discarding it.
///
/// The failure is logged and processing continues: one lost notification must
/// not abort a batch that is already half done, but it must not be invisible
/// either - a dropped `processing-complete` is what used to leave the UI stuck
/// in the `processing` state.
pub(super) fn emit_or_log<E>(emitter: &E, event: BatchEvent, log: LogSink<'_>)
where
    E: EventEmitter,
{
    let channel = event.channel();
    if let Err(err) = emitter.emit_event(event) {
        log(format!("failed to emit \"{}\" event: {}", channel, err));
    }
}

/// Scan paths (files or directories) into the flat list of supported images.
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
                if crate::image::formats::InputFormat::is_supported(&ext.to_string_lossy()) {
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
                        if crate::image::formats::InputFormat::is_supported(&ext.to_string_lossy())
                        {
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

                // Emit progress updates at ~1% granularity (plus the first and
                // the last file): a 5000-file batch would otherwise fire 5000
                // serialized IPC events that the progress bar cannot display
                // as sub-percent steps anyway. Per-file results stay
                // one-per-file — the file list needs every single one.
                if current == 1
                    || current == total_files
                    || current.is_multiple_of((total_files / 100).max(1))
                {
                    emit_or_log(
                        emitter,
                        BatchEvent::Progress(ProgressUpdate {
                            current,
                            total: total_files,
                            current_file: input_path.clone(),
                            percent: (current as f64 / total_files as f64) * 100.0,
                        }),
                        log,
                    );
                }

                // Emit individual file result
                emit_or_log(emitter, BatchEvent::Result(result.clone()), log);

                Some(result)
            })
            .collect()
    });

    // Calculate statistics
    let stats = calculate_batch_stats(&results);

    // Emit completion event
    emit_or_log(emitter, BatchEvent::Complete(stats.clone()), log);

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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// Emitter that always fails, standing in for a webview that went away
    /// mid-batch.
    struct FailingEmitter;

    impl EventEmitter for FailingEmitter {
        fn emit_event(&self, _event: BatchEvent) -> Result<(), String> {
            Err("webview channel closed".to_string())
        }
    }

    struct OkEmitter;

    impl EventEmitter for OkEmitter {
        fn emit_event(&self, _event: BatchEvent) -> Result<(), String> {
            Ok(())
        }
    }

    static TEST_DIR_COUNTER: AtomicUsize = AtomicUsize::new(0);

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

        emit_or_log(
            &FailingEmitter,
            BatchEvent::Complete(BatchStats {
                total_files: 1,
                processed_files: 1,
                successful_files: 1,
                failed_files: 0,
                total_original_size: 100,
                total_output_size: 50,
                overall_reduction_percent: 50.0,
                average_reduction_percent: 50.0,
                median_reduction_percent: 50.0,
            }),
            &|message| logged.lock().unwrap().push(message),
        );

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

        emit_or_log(
            &OkEmitter,
            BatchEvent::Progress(ProgressUpdate {
                current: 1,
                total: 1,
                current_file: "a.png".to_string(),
                percent: 100.0,
            }),
            &|message| logged.lock().unwrap().push(message),
        );

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
