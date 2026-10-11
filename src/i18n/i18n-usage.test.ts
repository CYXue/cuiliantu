// i18n key-usage scan: every key shipped in the locale files must be
// referenced by source code.
//
// Why: three locale files used to carry keys nobody read (14 at the time of
// the first audit), and the key-consistency test between locales cannot see
// "present everywhere, used nowhere". This scan fails the suite when a key
// loses its last reference, so removals happen in the same commit.
//
// Matching is deliberately simple: a key counts as used when it appears as a
// quoted literal OR as a template-literal prefix (`prefix.${...}) anywhere in
// src/. Comments are NOT stripped — a key mentioned in a comment passes the
// scan, which errs on the side of keeping a key (a false "alive" is cheaper
// than a false "dead" that would delete a translation still in use).
import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";
import en from "./locales/en.json";

// Keys constructed dynamically (`prefix.${...}`). Each entry must have a
// comment naming the construction site.
const DYNAMIC_PREFIXES: string[] = [
  // ResizeSection: `settings.fit${Mode}` for stretch/contain/cover.
  "settings.fit",
];

function flatten(node: unknown, prefix: string, out: string[]): void {
  for (const [k, v] of Object.entries(node as Record<string, unknown>)) {
    const key = prefix ? `${prefix}.${k}` : k;
    if (v && typeof v === "object") flatten(v, key, out);
    else out.push(key);
  }
}

function collectSourceFiles(dir: string, out: string[]): void {
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const full = join(dir, entry.name);
    if (entry.isDirectory()) {
      if (entry.name === "locales" || entry.name === "__tests__") continue;
      collectSourceFiles(full, out);
    } else if (
      /\.(ts|tsx)$/.test(entry.name) &&
      !/\.(test|spec)\./.test(entry.name)
    ) {
      out.push(full);
    }
  }
}

describe("i18n key usage", () => {
  it("every locale key is referenced somewhere in src/", () => {
    const keys: string[] = [];
    flatten(en, "", keys);

    const files: string[] = [];
    collectSourceFiles(join(import.meta.dirname, ".."), files);
    const corpus = files.map((f) => readFileSync(f, "utf-8")).join("\n");

    const unused = keys.filter((key) => {
      if (DYNAMIC_PREFIXES.some((p) => key.startsWith(p))) return false;
      // Quoted exact reference, or the start of a dynamic template key.
      return (
        !corpus.includes(`"${key}"`) &&
        !corpus.includes(`'${key}'`) &&
        !corpus.includes("`" + key) &&
        !corpus.includes(`"${key}.`)
      );
    });

    expect(unused).toEqual([]);
  });
});
