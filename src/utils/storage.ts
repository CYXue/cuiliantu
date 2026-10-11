/**
 * One-shot localStorage migration helper.
 *
 * Three features (i18n language, theme, option templates) independently
 * carried the same pre-rename-key migration loop. This is the single
 * implementation: copy the newest legacy value to the current key, drop every
 * legacy entry, and return the migrated value (or null when there was
 * nothing to move). `validate` lets callers reject corrupt payloads — a
 * failed validation leaves the legacy data untouched and returns null.
 */
export function migrateLegacyStorage(
  newKey: string,
  legacyKeys: readonly string[],
  validate?: (raw: string) => boolean,
): string | null {
  if (typeof localStorage === "undefined") return null;
  for (const key of legacyKeys) {
    const legacy = localStorage.getItem(key);
    if (!legacy) continue;
    if (validate && !validate(legacy)) return null;
    try {
      localStorage.setItem(newKey, legacy);
      for (const k of legacyKeys) localStorage.removeItem(k);
    } catch (error) {
      console.warn("Failed to migrate legacy storage:", error);
    }
    return legacy;
  }
  return null;
}
