use serde::{Deserialize, Serialize};
use std::time::Duration;

const GITHUB_REPO: &str = "CYXue/cuiliantu";
const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Information about an available update
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateInfo {
    /// Whether an update is available
    pub update_available: bool,
    /// Current version of the app
    pub current_version: String,
    /// Latest version available
    pub latest_version: String,
    /// URL to the release page
    pub release_url: String,
}

/// GitHub release response (partial)
#[derive(Debug, Deserialize)]
struct GitHubRelease {
    tag_name: String,
    html_url: String,
}

/// Check for updates from GitHub releases
#[tauri::command]
pub async fn check_for_updates() -> Result<UpdateInfo, String> {
    let url = format!(
        "https://api.github.com/repos/{}/releases/latest",
        GITHUB_REPO
    );

    // Bound the request so a hung/unreachable network cannot stall the startup
    // update check (the UI awaits this call and would otherwise spin forever).
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .map_err(|e| format!("Failed to create HTTP client: {}", e))?;
    let response = client
        .get(&url)
        .header("User-Agent", "CuiLianTu-App")
        .send()
        .await
        .map_err(|e| format!("Failed to fetch release info: {}", e))?;

    if response.status() == reqwest::StatusCode::NOT_FOUND {
        // No releases yet
        return Ok(UpdateInfo {
            update_available: false,
            current_version: CURRENT_VERSION.to_string(),
            latest_version: CURRENT_VERSION.to_string(),
            release_url: format!("https://github.com/{}/releases", GITHUB_REPO),
        });
    }

    if !response.status().is_success() {
        return Err(format!("GitHub API returned status: {}", response.status()));
    }

    let release: GitHubRelease = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse release info: {}", e))?;

    // Remove 'v' prefix if present for comparison
    let latest_version = release.tag_name.trim_start_matches('v').to_string();
    let current = CURRENT_VERSION.trim_start_matches('v');

    let update_available = is_newer_version(&latest_version, current);

    Ok(UpdateInfo {
        update_available,
        current_version: CURRENT_VERSION.to_string(),
        latest_version,
        release_url: sanitize_release_url(&release.html_url),
    })
}

/// Only trust GitHub release pages. Anything else — a hijacked redirect or a
/// mis-configured endpoint — must not be opened by the frontend's `openUrl`,
/// and falls back to the canonical releases listing instead.
fn sanitize_release_url(url: &str) -> String {
    if url.starts_with("https://github.com/") {
        url.to_string()
    } else {
        format!("https://github.com/{}/releases", GITHUB_REPO)
    }
}

/// Get the current app version
#[tauri::command]
pub fn get_current_version() -> String {
    CURRENT_VERSION.to_string()
}

/// Compare semantic versions to check if `latest` is newer than `current`.
///
/// Both sides are parsed as `major.minor.patch` plus an optional pre-release
/// suffix (`-rc.1`, `-beta.2`). Two rules matter for correctness here:
///
/// 1. A pre-release is **older** than the release it precedes, so
///    `0.3.0-rc.1` must NOT be treated as equal to `0.3.0`. The old
///    `filter_map(..ok())` dropped the non-numeric `-rc.1` segment and left
///    `[0, 3, 0]`, which made a release candidate look like the final release
///    and could push users onto a pre-release build.
/// 2. Unparsable input yields `None`, and a version we cannot understand never
///    counts as an upgrade — the safe direction, since the alternative is
///    showing a bogus update prompt.
fn parse_version(v: &str) -> Option<(u64, u64, u64, Option<String>)> {
    let v = v.trim().trim_start_matches('v');
    // A leading `v` is stripped by the caller too; splitting on the pre-release
    // marker first keeps `-rc.1` out of the numeric segments.
    let (core, prerelease) = match v.split_once('-') {
        Some((core, pre)) => (core, Some(pre.to_ascii_lowercase())),
        None => (v, None),
    };
    let mut nums = core.split('.');
    let major = nums.next()?.parse::<u64>().ok()?;
    let minor = nums.next()?.parse::<u64>().ok()?;
    let patch = nums.next().unwrap_or("0").parse::<u64>().ok()?;
    Some((major, minor, patch, prerelease))
}

/// `true` when `latest` is strictly newer than `current`.
fn is_newer_version(latest: &str, current: &str) -> bool {
    let (Some((lmaj, lmin, lpat, lpre)), Some((cmaj, cmin, cpat, cpre))) =
        (parse_version(latest), parse_version(current))
    else {
        // Unknown version on either side: do not prompt an update we cannot
        // reason about.
        return false;
    };

    if (lmaj, lmin, lpat) != (cmaj, cmin, cpat) {
        return (lmaj, lmin, lpat) > (cmaj, cmin, cpat);
    }

    // Same core version: a release beats any pre-release of that release.
    match (cpre, lpre) {
        (None, None) => false,
        // current is final, latest is a pre-release of the same version.
        (None, Some(_)) => false,
        // current is a pre-release, latest is the final release.
        (Some(_), None) => true,
        (Some(cur), Some(lat)) => {
            // A pre-release is dot-separated: `rc.1`, `beta.2`. Compare the
            // full identifier list per semver §11.4 — numeric identifiers
            // compare numerically, rank above alphanumeric ones, and a longer
            // identifier list wins when all preceding fields are equal
            // (`rc` < `rc.1`).
            let rank = |s: &str| -> (u8, u64) {
                match s.parse::<u64>() {
                    Ok(n) => (2, n),
                    Err(_) => (1, 0),
                }
            };
            let mut cur_parts = cur.split('.');
            let mut lat_parts = lat.split('.');
            loop {
                match (cur_parts.next(), lat_parts.next()) {
                    (None, None) => return false, // identical pre-release
                    // `latest` still has segments and every shared segment was
                    // equal → it is the more specific (later) identifier.
                    (None, Some(_)) => return true,
                    (Some(_), None) => return false,
                    (Some(c), Some(l)) => {
                        let (ck, cn) = rank(c);
                        let (lk, ln) = rank(l);
                        if lk != ck {
                            return lk > ck;
                        }
                        if ln != cn {
                            return ln > cn;
                        }
                        // Alphanumeric segment: fall back to ASCII order.
                        if ck == 1 && l != c {
                            return l > c;
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_version_comparison() {
        assert!(is_newer_version("0.2.0", "0.1.0"));
        assert!(is_newer_version("1.0.0", "0.9.9"));
        assert!(is_newer_version("0.1.1", "0.1.0"));
        assert!(!is_newer_version("0.1.0", "0.1.0"));
        assert!(!is_newer_version("0.1.0", "0.2.0"));
        assert!(!is_newer_version("0.0.9", "0.1.0"));
    }

    // --- regression guards for the prerelease bug -------------------------
    //
    // The old parser was `split('.').filter_map(parse::<u32>.ok())`, which
    // silently dropped the `-rc.1` segment: `0.3.0-rc.1` parsed to `[0,3,0]`
    // and compared *equal* to the final `0.3.0`, so a release candidate could
    // be pushed to users as if it were the stable release.

    #[test]
    fn prerelease_is_older_than_the_final_release() {
        // The core bug: these used to compare equal.
        assert!(!is_newer_version("0.3.0-rc.1", "0.3.0"));
        assert!(!is_newer_version("0.3.0-rc.1", "0.3.0-rc.1"));
        // Moving from an rc to the final release IS an update.
        assert!(is_newer_version("0.3.0", "0.3.0-rc.1"));
        assert!(is_newer_version("0.3.0", "0.3.0-rc.9"));
    }

    #[test]
    fn prerelease_ordering_follows_semver() {
        // rc.1 < rc.2 < rc.10 (numeric identifiers compare numerically, so
        // rc.10 is NOT "less than" rc.2 the way a string compare would say).
        assert!(is_newer_version("1.0.0-rc.2", "1.0.0-rc.1"));
        assert!(is_newer_version("1.0.0-rc.10", "1.0.0-rc.2"));
        assert!(!is_newer_version("1.0.0-rc.1", "1.0.0-rc.2"));
        // A numeric identifier ranks above an alphanumeric one (semver 11.4).
        assert!(is_newer_version("1.0.0-2", "1.0.0-rc.1"));
    }

    #[test]
    fn unparsable_versions_never_prompt_an_update() {
        // Unknown input must fail *closed*: a bogus "update" prompt is worse
        // than staying quiet.
        assert!(!is_newer_version("not-a-version", "0.1.0"));
        assert!(!is_newer_version("0.2.0", "garbage"));
        assert!(!is_newer_version("", "0.1.0"));
        // A major-only version is still comparable (patch defaults to 0).
        assert!(is_newer_version("1.0", "0.9.9"));
        assert!(!is_newer_version("1.0", "1.0.0"));
    }

    #[test]
    fn leading_v_prefix_is_ignored() {
        assert!(is_newer_version("v0.2.0", "0.1.0"));
        assert!(is_newer_version("v0.3.0", "v0.3.0-rc.1"));
        assert!(!is_newer_version("v0.3.0-rc.1", "0.3.0"));
    }

    #[test]
    fn release_url_is_constrained_to_github() {
        assert_eq!(
            sanitize_release_url("https://github.com/CYXue/cuiliantu/releases/tag/v1"),
            "https://github.com/CYXue/cuiliantu/releases/tag/v1"
        );
        // A hijacked redirect or a mis-configured endpoint must fall back to
        // the canonical listing rather than being handed to `openUrl`.
        for hostile in [
            "https://evil.example.com/releases",
            "http://github.com/CYXue/cuiliantu/releases",
            // Prefix look-alike: `github.com.evil.tld` starts with the string
            // but is a different host.
            "https://github.com.evil.tld/x",
            "",
        ] {
            assert_eq!(
                sanitize_release_url(hostile),
                "https://github.com/CYXue/cuiliantu/releases",
                "must not trust: {hostile}"
            );
        }
    }
}
