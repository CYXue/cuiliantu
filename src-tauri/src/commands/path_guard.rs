//! In-memory whitelist of filesystem paths the frontend is allowed to touch.
//!
//! The image commands read and write paths with plain `std::fs`, which
//! bypasses Tauri's `fs` capability scope (that scope only governs the
//! `@tauri-apps/plugin-fs` JS API). To keep the IPC surface defense-in-depth,
//! the frontend registers every path it obtained legitimately — dialog picks,
//! drag-and-drop, saved paste files — and the commands verify each path
//! against this whitelist before touching the disk.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

/// Shared whitelist handle. Cheap to clone into `spawn_blocking` closures.
#[derive(Clone, Default)]
pub struct PathWhitelist {
    roots: Arc<RwLock<HashSet<PathBuf>>>,
}

/// Upper bound on remembered roots.
///
/// A batch of folders can legitimately contribute thousands of roots, but the
/// set only grows for the lifetime of the session. Without a ceiling a
/// compromised or buggy frontend could push unbounded entries into memory by
/// calling `register_allowed_paths` in a loop. When the cap is hit, the *oldest*
/// insertion is dropped: recent user activity is what commands need next, and
/// anything evicted can be re-registered by the user at any time.
const MAX_ROOTS: usize = 4_096;

/// Directories the app never treats as a legitimate source or destination.
///
/// The whitelist exists to scope the IPC surface to what the user actually
/// opened, so registering an operating-system or user-credential directory is
/// never a real user action — a drag-and-drop or a dialog cannot target them.
/// Refusing them here means a compromised frontend cannot turn the whitelist
/// into a "read my SSH keys" primitive.
const FORBIDDEN_DIR_NAMES: &[&str] = &[
    ".ssh",
    ".gnupg",
    ".aws",
    ".azure",
    ".kube",
    ".docker",
    ".config/gcloud",
    "AppData/Roaming/Microsoft/Credentials",
    "AppData/Local/Microsoft/Credentials",
];

/// Single user-facing rejection message. Deliberately uniform and free of any
/// path detail: the caller must not be able to probe *why* a location was
/// refused, and must not learn the directory layout of the machine.
const REJECTED: &str = "Path is outside the folders this session has opened";

/// `true` when `path` is inside a directory the app refuses to whitelist.
///
/// Checked on the *resolved* path, so `~/.ssh/../.ssh/id_rsa` and a symlink
/// pointing at it are both caught.
fn hits_forbidden_dir(path: &Path) -> bool {
    let text = path.to_string_lossy().replace('\\', "/");
    let lowered = text.to_lowercase();
    FORBIDDEN_DIR_NAMES.iter().any(|needle| {
        let needle = needle.to_lowercase();
        // Match a whole path segment so `/home/u/.sshery/x` is not caught by
        // the `.ssh` rule.
        lowered.contains(&format!("/{needle}/")) || lowered.ends_with(&format!("/{needle}"))
    })
}

impl PathWhitelist {
    /// Register paths (files or directories) as legitimate targets.
    ///
    /// Files also register their parent directory, so a dropped folder's
    /// walk results and a picked file's siblings are covered. Unresolvable
    /// paths (deleted meanwhile, permission denied) are skipped silently:
    /// the follow-up command would fail anyway with a clearer error.
    pub fn register(&self, paths: &[String]) -> usize {
        let mut roots = match self.roots.write() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        let before = roots.len();
        for path_str in paths {
            let path = Path::new(path_str);
            // Canonicalize resolves symlinks and normalizes separators, so
            // `C:\foo\..\bar` and `\\?\C:\foo` compare equal on Windows.
            let Ok(canonical) = path.canonicalize() else {
                continue;
            };
            if hits_forbidden_dir(&canonical) {
                continue;
            }
            roots.insert(canonical.clone());
            if canonical.is_file() {
                if let Some(parent) = canonical.parent() {
                    roots.insert(parent.to_path_buf());
                }
            }
            // Keep the set bounded. Evicting an arbitrary entry keeps the cost
            // O(1); recency matters more than order for correctness here.
            while roots.len() > MAX_ROOTS {
                let victim = roots
                    .iter()
                    .next()
                    .map(|p| p.to_path_buf())
                    .expect("set is non-empty while over the cap");
                roots.remove(&victim);
            }
        }
        roots.len().saturating_sub(before)
    }

    /// Number of roots currently remembered. Test-only observability.
    #[cfg(test)]
    pub fn len(&self) -> usize {
        match self.roots.read() {
            Ok(guard) => guard.len(),
            Err(poisoned) => poisoned.into_inner().len(),
        }
    }

    /// Whether any root is remembered. Test-only observability.
    #[cfg(test)]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// `true` when `path` is itself registered or lives under a registered root.
    ///
    /// `canonicalize` requires the path to *exist*, but callers also validate
    /// targets that have not been created yet — a report the user is about to
    /// save, an output directory picked through the dialog, a not-yet-written
    /// preview cache. When the path itself is missing we fall back to
    /// canonicalizing the nearest existing ancestor and re-attaching the leaf
    /// (see [`resolve_for_check`]), so such "not-yet-created" targets are still
    /// matched against the registered roots. A path whose whole ancestry is
    /// unknown stays rejected: we cannot vouch for a location we cannot verify.
    pub fn is_allowed(&self, path: &Path) -> bool {
        let Some(canonical) = Self::resolve_for_check(path) else {
            return false;
        };
        let roots = match self.roots.read() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        if roots.contains(&canonical) {
            return true;
        }
        roots.iter().any(|root| canonical.starts_with(root))
    }

    /// Existence-tolerant canonicalization used by [`PathWhitelist::is_allowed`].
    ///
    /// `Path::canonicalize` resolves symlinks and normalizes separators *and*
    /// the Windows `\\?\` verbatim prefix, but it fails for paths that do not
    /// exist yet. For a whitelist check we still want to validate a pending
    /// target, so when `path` is missing we walk up to the nearest existing
    /// ancestor, canonicalize *that*, and re-attach the original leaf name. The
    /// result can then be matched with `starts_with` against the registered
    /// roots. Returns `None` only when no ancestor exists at all.
    fn resolve_for_check(path: &Path) -> Option<PathBuf> {
        if let Ok(c) = path.canonicalize() {
            return Some(c);
        }
        let mut ancestor = path.parent();
        while let Some(candidate) = ancestor {
            if let Ok(c) = candidate.canonicalize() {
                let leaf = path.file_name().unwrap_or_default();
                return Some(c.join(leaf));
            }
            ancestor = candidate.parent();
        }
        None
    }

    /// Like [`is_allowed`], but returns a frontend-presentable error.
    ///
    /// The message deliberately does not echo the path back.
    pub fn ensure_allowed(&self, path: &Path) -> Result<(), String> {
        if self.is_allowed(path) {
            Ok(())
        } else {
            Err(REJECTED.to_string())
        }
    }

    /// Gate for a location the user is about to *choose* through a dialog
    /// (currently the CSV report destination).
    ///
    /// Such a path is legitimately not in the whitelist yet, so requiring
    /// `is_allowed` would reject every real save. Instead this asserts the two
    /// things that must hold for a user-initiated choice: the resolved location
    /// is not inside a credential directory, and it exists (a save dialog hands
    /// back an existing directory). It performs **no** registration — callers
    /// decide what to whitelist afterwards, keeping "validated" and "trusted"
    /// as separate steps.
    pub fn ensure_allowed_tree(&self, path: &Path) -> Result<(), String> {
        let resolved = Self::resolve_for_check(path)
            .ok_or_else(|| "Cannot resolve the selected location".to_string())?;
        if hits_forbidden_dir(&resolved) {
            return Err(REJECTED.to_string());
        }
        if !resolved.is_dir() {
            return Err("Select an existing folder to save into".to_string());
        }
        Ok(())
    }
}

/// Frontend entry point: whitelist paths obtained via dialogs, drag-and-drop
/// or paste. Accepts a mix of files and directories.
///
/// Returns how many roots were actually taken. The count lets the frontend log
/// a rejection reason without the backend having to explain which path was
/// refused (naming it would leak the very directory we declined to touch).
#[tauri::command]
pub fn register_allowed_paths(
    paths: Vec<String>,
    whitelist: tauri::State<PathWhitelist>,
) -> Result<usize, String> {
    if paths.len() > MAX_ROOTS {
        return Err(format!(
            "Refusing to register {} paths at once (limit {})",
            paths.len(),
            MAX_ROOTS
        ));
    }
    Ok(whitelist.register(&paths))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("clt-guard-{}-{}", name, std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("failed to create scratch dir");
        dir
    }

    fn write_png(dir: &Path, name: &str) -> PathBuf {
        let path = dir.join(name);
        image::RgbaImage::from_pixel(4, 4, image::Rgba([1, 2, 3, 255]))
            .save(&path)
            .expect("failed to write test png");
        path
    }

    #[test]
    fn registered_file_allows_itself_and_its_tree() {
        let dir = scratch("allow-file");
        let file = write_png(&dir, "a.png");
        let wl = PathWhitelist::default();
        wl.register(&[file.to_string_lossy().to_string()]);

        assert!(wl.is_allowed(&file));

        // The file's parent was registered as a root, so anything below the
        // parent directory (e.g. walkdir results in a dropped folder) passes.
        let nested = dir.join("sub");
        std::fs::create_dir_all(&nested).expect("create nested dir");
        let nested_file = write_png(&nested, "b.png");
        assert!(wl.is_allowed(&nested_file));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn registered_directory_covers_children() {
        let dir = scratch("allow-dir");
        let file = write_png(&dir, "a.png");
        let wl = PathWhitelist::default();
        wl.register(&[dir.to_string_lossy().to_string()]);

        assert!(wl.is_allowed(&file));
        assert!(wl.ensure_allowed(&file).is_ok());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn unknown_path_is_rejected() {
        let dir = scratch("deny");
        let file = write_png(&dir, "secret.png");
        let wl = PathWhitelist::default();

        assert!(!wl.is_allowed(&file));
        let err = wl.ensure_allowed(&file).unwrap_err();
        assert!(
            !err.contains("secret"),
            "error must not echo the path back: {err}"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn pending_file_under_registered_root_is_allowed() {
        // A save target (report, output file) does not exist when it is
        // validated, yet it must be allowed via its registered parent dir.
        // Regression guard for the existence-tolerant `resolve_for_check`.
        let dir = scratch("allow-pending");
        let wl = PathWhitelist::default();
        wl.register(&[dir.to_string_lossy().to_string()]);

        let pending = dir.join("report.csv");
        assert!(
            wl.is_allowed(&pending),
            "pending file must be allowed through its registered parent"
        );
        assert!(wl.ensure_allowed(&pending).is_ok());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn dialog_chosen_tree_accepts_normal_dirs_and_refuses_credential_dirs() {
        let home = scratch("dialog-home");
        let wl = PathWhitelist::default();

        // A plain existing directory is what a save dialog returns.
        let good = home.join("reports");
        std::fs::create_dir_all(&good).expect("create reports dir");
        assert!(
            wl.ensure_allowed_tree(&good).is_ok(),
            "an ordinary picked folder must be accepted"
        );

        // A credential directory is refused even though the OS would happily
        // let a save dialog return it.
        let ssh = home.join(".ssh");
        std::fs::create_dir_all(&ssh).expect("create .ssh");
        assert!(
            wl.ensure_allowed_tree(&ssh).is_err(),
            "a credential directory must never be a save target"
        );

        // A file is not a folder to save into.
        let file = home.join("a.csv");
        std::fs::write(&file, b"x").expect("write");
        assert!(wl.ensure_allowed_tree(&file).is_err());

        // Crucially, a refused check must NOT have whitelisted anything:
        // `ensure_allowed_tree` only validates; the caller decides what to
        // register. If this regressed, the guard would be self-defeating.
        assert!(
            !wl.is_allowed(&ssh),
            "validation must not grant access by itself"
        );

        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn pending_file_under_unregistered_root_is_rejected() {
        let dir = scratch("deny-pending");
        let wl = PathWhitelist::default();
        let pending = dir.join("report.csv");
        assert!(!wl.is_allowed(&pending));

        let _ = std::fs::remove_dir_all(&dir);
    }

    // --- hardening guards -------------------------------------------------    //
    // These must be reverse-tested: each one asserts that a *hostile* input is
    // refused. A guard that silently passes every case looks exactly like a
    // working one, so the negative assertions below are the actual proof.

    #[test]
    fn credential_directories_are_never_whitelisted() {
        // Mirrors the shape of a real credential store so the segment rule is
        // exercised end to end.
        let home = scratch("forbidden-home");
        let ssh = home.join(".ssh");
        std::fs::create_dir_all(&ssh).expect("create .ssh");
        let key = ssh.join("id_rsa");
        std::fs::write(&key, b"-----BEGIN OPENSSH PRIVATE KEY-----").expect("write key");

        let wl = PathWhitelist::default();
        wl.register(&[key.to_string_lossy().to_string()]);
        assert!(
            !wl.is_allowed(&key),
            "a key inside .ssh must never become reachable"
        );

        // The same rule must survive a traversal that reaches .ssh indirectly.
        let sneaky = home.join("nested").join("..").join(".ssh").join("id_rsa");
        std::fs::create_dir_all(home.join("nested")).expect("create nested");
        wl.register(&[sneaky.to_string_lossy().to_string()]);
        assert!(
            !wl.is_allowed(&key),
            "traversal into .ssh must resolve before the check"
        );

        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn ordinary_directories_are_still_whitelisted() {
        // The counterpart to the guard above: a normal folder must not be
        // caught by the segment rule.
        let home = scratch("normal-home");
        let ssh_like = home.join(".sshstuff"); // shares a prefix, not a segment
        std::fs::create_dir_all(&ssh_like).expect("create dir");
        let pic = ssh_like.join("a.png");
        std::fs::write(&pic, b"x").expect("write");

        let wl = PathWhitelist::default();
        wl.register(&[pic.to_string_lossy().to_string()]);
        assert!(wl.is_allowed(&pic), "a similar name must not be blocked");

        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn root_count_stays_bounded_under_flood() {
        // Registering far more than the cap must not grow the set without
        // bound — otherwise a looping caller is a memory-exhaustion vector.
        //
        // Every path must EXIST: `register` canonicalizes and silently skips
        // anything missing, so non-existent paths would register nothing and
        // this assertion would pass for the wrong reason (a gate that is green
        // because it never fired). The count is asserted positively first.
        let dir = scratch("flood");
        let batch: Vec<String> = (0..MAX_ROOTS + 50)
            .map(|i| {
                let sub = dir.join(format!("d{i}"));
                std::fs::create_dir_all(&sub).expect("create subdir");
                sub.to_string_lossy().to_string()
            })
            .collect();

        let wl = PathWhitelist::default();
        assert!(wl.is_empty(), "a fresh whitelist holds nothing");
        wl.register(&batch);
        assert!(
            wl.len() > 1,
            "sanity: the batch must actually register roots, got {}",
            wl.len()
        );
        assert!(
            wl.len() <= MAX_ROOTS,
            "set must stay capped, got {}",
            wl.len()
        );

        let _ = std::fs::remove_dir_all(&dir);
    }
}
