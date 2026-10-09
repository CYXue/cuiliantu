// Shared byte formatting for results and preview panels.
export function formatBytes(bytes: number): string {
  // Guard the degenerate inputs (negative sizes, NaN) so the function can
  // never render "NaN undefined" — sizes are normally >= 0, but a caller
  // bug should not turn into garbled UI text.
  if (!Number.isFinite(bytes) || bytes <= 0) return "0 B";
  const k = 1024;
  const sizes = ["B", "KB", "MB", "GB"];
  const i = Math.min(
    sizes.length - 1,
    Math.floor(Math.log(bytes) / Math.log(k)),
  );
  return `${parseFloat((bytes / k ** i).toFixed(1))} ${sizes[i]}`;
}
