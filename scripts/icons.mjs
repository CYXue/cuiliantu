#!/usr/bin/env node
// Cross-platform icon tooling. Replaces the previous bash scripts, which
// could not run from cmd.exe/PowerShell on Windows (npm scripts on Windows do
// not execute .sh) and additionally required ImageMagick plus the macOS-only
// `iconutil`.
//
// Usage:
//   node scripts/icons.mjs dev       Switch to DEV icons (backs up prod first)
//   node scripts/icons.mjs prod      Restore production icons
//   node scripts/icons.mjs generate  (Re)build DEV icons from prod icons
//
// `dev` / `prod` are pure Node (fs only) and run everywhere.
// `generate` shells out to ImageMagick when available (prefers `magick`;
// avoids the Windows `C:\Windows\System32\convert.exe` name clash) and to
// `iconutil` on macOS only. Missing tools are skipped with a clear message
// instead of failing the whole run.

import {
  closeSync,
  copyFileSync,
  cpSync,
  existsSync,
  mkdirSync,
  openSync,
  readdirSync,
  readSync,
  rmSync,
} from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { spawnSync } from "node:child_process";

const projectDir = dirname(dirname(fileURLToPath(import.meta.url)));
const iconsDir = join(projectDir, "src-tauri", "icons");
const devIconsDir = join(projectDir, "src-tauri", "icons-dev");
const prodIconsDir = join(projectDir, "src-tauri", "icons-prod");

const log = (msg) => console.log(msg);
const die = (msg) => {
  console.error(msg);
  process.exitCode = 1;
};

// ---------------------------------------------------------------------------
// ImageMagick helpers
// ---------------------------------------------------------------------------

/** Find a working ImageMagick CLI. Prefers `magick` (IM7): on Windows the
 * bare `convert` resolves to C:\Windows\System32\convert.exe (the disk
 * conversion utility), so it is only used when it answers `magick -version`. */
function findImageMagick() {
  for (const candidate of ["magick", "convert"]) {
    const probe = spawnSync(candidate, ["-version"], { encoding: "utf8" });
    if (!probe.error && probe.status === 0 && /ImageMagick/i.test(probe.stdout)) {
      return candidate;
    }
  }
  return null;
}

/** Width of a PNG read straight from its IHDR chunk (no `identify` needed).
 * Returns 0 for anything that is not a readable square-capable PNG. */
function pngWidth(file) {
  try {
    const buf = Buffer.alloc(24);
    const fd = openSync(file, "r");
    readSync(fd, buf, 0, 24, 0);
    closeSync(fd);
    if (buf.readUInt32BE(12) !== 0x49484452) return 0; // "IHDR"
    return buf.readUInt32BE(16);
  } catch {
    return 0;
  }
}

function runMagick(magick, args, label) {
  const result = spawnSync(magick, args, { stdio: "inherit" });
  if (result.error || result.status !== 0) {
    log(`  ! ${label} failed (see output above); continuing.`);
    return false;
  }
  return true;
}

/** Stamp a red "DEV" ribbon onto one icon (same output as the old bash). */
function addRibbon(magick, input, output, size) {
  const ribbonHeight = Math.floor(size / 4);
  const fontSize = Math.floor(size / 6);
  const ribbonY = size - ribbonHeight;
  return runMagick(
    magick,
    [
      input,
      "-fill",
      "rgba(239, 68, 68, 0.9)",
      "-draw",
      `polygon 0,${ribbonY} ${size},${ribbonY} ${size},${size} 0,${size}`,
      "-fill",
      "white",
      "-font",
      "Helvetica-Bold",
      "-pointsize",
      String(fontSize),
      "-gravity",
      "South",
      "-annotate",
      `+0+${Math.floor(ribbonHeight / 4)}`,
      "DEV",
      output,
    ],
    `ribbon ${input}`,
  );
}

// ---------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------

/** Copy the active icon set's images into `iconsDir`. */
function copyIconSet(fromDir, label) {
  let copied = 0;
  for (const file of readdirSync(fromDir)) {
    const isIcon =
      file.endsWith(".png") || file === "icon.icns" || file === "icon.ico";
    if (!isIcon) continue;
    copyFileSync(join(fromDir, file), join(iconsDir, file));
    copied += 1;
  }
  log(`${label} icons active (${copied} files copied).`);
}

function cmdDev() {
  if (!existsSync(devIconsDir)) {
    die("icons-dev/ not found. Run `node scripts/icons.mjs generate` first.");
    return;
  }
  if (!existsSync(prodIconsDir)) {
    log("Backing up production icons...");
    cpSync(iconsDir, prodIconsDir, { recursive: true });
  }
  log("Switching to development icons...");
  copyIconSet(devIconsDir, "Development");
}

function cmdProd() {
  if (!existsSync(prodIconsDir)) {
    log("Production icons backup not found. Icons are already production versions.");
    return;
  }
  log("Restoring production icons...");
  copyIconSet(prodIconsDir, "Production");
}

function cmdGenerate() {
  const magick = findImageMagick();
  if (!magick) {
    log("! ImageMagick not found (looked for `magick` / `convert`).");
    log("  PNG ribbons and icon.ico will be skipped. Install ImageMagick to generate them.");
  }

  mkdirSync(devIconsDir, { recursive: true });

  let processed = 0;
  for (const file of readdirSync(iconsDir)) {
    if (!file.endsWith(".png")) continue;
    const input = join(iconsDir, file);
    const size = pngWidth(input);
    if (size <= 0) {
      log(`  ! skipping ${file}: not a readable PNG.`);
      continue;
    }
    log(`Processing ${file} (${size}x${size})...`);
    if (magick && addRibbon(magick, input, join(devIconsDir, file), size)) {
      processed += 1;
    }
  }

  // macOS only: bundle the iconset into an .icns with iconutil.
  if (process.platform === "darwin") {
    const iconutil = spawnSync("iconutil", ["--help"], { encoding: "utf8" });
    if (!iconutil.error && iconutil.status === 0) {
      log("Generating icon.icns...");
      const iconset = join(devIconsDir, "icon.iconset");
      mkdirSync(iconset, { recursive: true });
      const sizes = [
        [16, "icon_16x16.png"],
        [32, "icon_16x16@2x.png"],
        [32, "icon_32x32.png"],
        [64, "icon_32x32@2x.png"],
        [128, "icon_128x128.png"],
        [256, "icon_128x128@2x.png"],
        [256, "icon_256x256.png"],
        [512, "icon_256x256@2x.png"],
        [512, "icon_512x512.png"],
        [1024, "icon_512x512@2x.png"],
      ];
      const source = join(devIconsDir, "icon.png");
      if (magick && existsSync(source)) {
        for (const [px, name] of sizes) {
          runMagick(magick, [source, "-resize", `${px}x${px}`, join(iconset, name)], name);
        }
        const pack = spawnSync(
          "iconutil",
          ["-c", "icns", iconset, "-o", join(devIconsDir, "icon.icns")],
          { stdio: "inherit" },
        );
        if (pack.error || pack.status !== 0) {
          log("  ! iconutil failed; icon.icns not updated.");
        }
        rmSync(iconset, { recursive: true, force: true });
      } else {
        log("  ! icon.png missing; icon.icns not generated.");
      }
    } else {
      log("! iconutil not available; icon.icns skipped.");
    }
  }

  // Windows .ico bundle.
  if (magick) {
    const source = join(devIconsDir, "icon.png");
    if (existsSync(source)) {
      log("Generating icon.ico...");
      runMagick(
        magick,
        [source, "-define", "icon:auto-resize=256,128,64,48,32,16", join(devIconsDir, "icon.ico")],
        "icon.ico",
      );
    } else {
      log("  ! icon.png missing; icon.ico not generated.");
    }
  }

  log(`Development icons generated in ${devIconsDir} (${processed} PNGs stamped).`);
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

const command = process.argv[2];
switch (command) {
  case "dev":
    cmdDev();
    break;
  case "prod":
    cmdProd();
    break;
  case "generate":
    cmdGenerate();
    break;
  default:
    die("Usage: node scripts/icons.mjs <dev|prod|generate>");
}
