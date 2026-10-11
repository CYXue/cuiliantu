/**
 * Shared class-string helpers.
 *
 * The segmented toggle rows (resize mode, fit mode, rotation, watermark kind)
 * hand-rolled the identical active/inactive class pair in four files; this is
 * the single version. (`EditSection`'s chip toggles use a visibly different
 * bordered style and keep their own helper.)
 */
export function segmentedBtnClass(active: boolean): string {
  return `flex-1 text-xs font-medium py-1.5 rounded-lg transition-all duration-200 ${
    active
      ? "bg-indigo-500 text-white shadow-sm"
      : "text-slate-500 hover:bg-slate-100"
  }`;
}
