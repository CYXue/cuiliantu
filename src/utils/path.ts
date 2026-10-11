// Shared path helpers for components that display file names.
// Paths come from Tauri and are OS-dependent: Windows uses "\\" while
// macOS/Linux use "/". Splitting on only one separator returns the whole
// path as a single element on the other OS, so always split on both.
export function basename(path: string): string {
  const segments = path.split(/[/\\]/);
  return segments[segments.length - 1] || path;
}
