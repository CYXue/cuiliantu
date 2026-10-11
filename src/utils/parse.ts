/**
 * Runtime-checked select parsing.
 *
 * `<select>.value` is just a string; casting it to a union type trusts the
 * DOM (or a crafted payload) blindly. Pairing the cast with the exact list of
 * legal values turns it into a checked parse that falls back to the first
 * legal value instead of smuggling an unexpected string into the store.
 */
export function parseSelectValue<T extends string>(
  raw: string,
  valid: readonly T[],
): T {
  return (valid as readonly string[]).includes(raw) ? (raw as T) : valid[0];
}
