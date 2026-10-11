// Strips absolute filesystem paths from error messages before they reach the
// UI. The app is "private by design", so internal directory structure must not
// leak into user-visible text. Windows drive paths (`C:\...`), Windows UNC
// paths (`\\server\share\...`) and Unix absolute paths (`/home/...`) are
// collapsed to just the final file name.
export function sanitizeError(message: string): string {
  if (!message) return message;
  // Windows drive path: `C:\foo\bar\` — segments may contain spaces.
  let out = message.replace(/[A-Za-z]:\\(?:[^\\:"<>|]*\\)*/g, "");
  // Windows UNC path: `\\server\share\foo\bar\` — same segment rules as a
  // drive path. Drag-dropped files can live on network shares, so this must
  // not leak either.
  out = out.replace(/\\\\[^\\/:*?"<>|]+\\(?:[^\\:"<>|]*\\)*/g, "");
  // Unix absolute path rooted at a well-known directory: `/home/a b/pic/`.
  // Interior spaces are allowed only under these known roots, so ordinary
  // slash-separated prose ("options /verbose /quiet") is never mangled.
  out = out.replace(
    /\/(?:home|Users|root|tmp|private|var|opt|usr|etc|mnt|media|srv|Applications|Library|Volumes)\/(?:[^/]+\/)+/g,
    "",
  );
  // Unix absolute path (generic): `/foo/bar/` — space-free segments only, so
  // this pass cannot swallow words that merely sit between two slashes.
  out = out.replace(/\/(?:[^/\s]+\/)+/g, "");
  return out;
}
