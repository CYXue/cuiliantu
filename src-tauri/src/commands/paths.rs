//! Path helpers shared by the batch commands.
//!
//! Split out of the former `image_commands.rs` monolith: these are pure
//! lexical/canonicalization utilities with no Tauri or processing dependency.

use std::path::{Path, PathBuf};

/// Best-effort canonicalization of a single path.
///
/// Resolves symlinks and makes the path absolute (via `canonicalize`) so two
/// logically-identical inputs — different separators, a symlinked folder, or a
/// relative path — collapse to the same canonical ancestor during the
/// shared-parent search. When `canonicalize` fails (the file is missing, lives
/// on a network share, or lacks read permission) we fall back to the original
/// path, which keeps the function working for the common case.
pub(super) fn normalize_path(path: &Path) -> PathBuf {
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
pub(super) fn starts_with_ci(path: &Path, prefix: &Path) -> bool {
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
pub(super) fn strip_prefix_ci(path: &Path, prefix: &Path) -> Option<PathBuf> {
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
pub(super) fn common_base_dir(paths: &[String]) -> Option<PathBuf> {
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Create a unique scratch directory for one test.
    fn scratch_dir(name: &str) -> PathBuf {
        static TEST_DIR_COUNTER: std::sync::atomic::AtomicU32 =
            std::sync::atomic::AtomicU32::new(0);
        let id = TEST_DIR_COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!("clt-{}-{}-{}", name, std::process::id(), id));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("failed to create scratch dir");
        dir
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
        // The expectation must go through the same normalization as
        // `common_base_dir`: on CI the temp dir contains an 8.3 short name
        // (`RUNNER~1`), which `canonicalize` resolves to the long form.
        // Comparing against the raw `temp_dir()` form fails there.
        let expected = normalize_path(&dir.join("sub"));
        assert_eq!(common_base_dir(&inputs), Some(expected));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
