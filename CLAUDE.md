# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Build and Development Commands

```bash
# Install dependencies
pnpm install

# Run in development mode (starts both Vite dev server and Tauri)
pnpm tauri dev

# Build for production
pnpm tauri build

# Type checking
pnpm tsc --noEmit

# Run Rust tests
cd src-tauri && cargo test

# Run Rust clippy
cd src-tauri && cargo clippy
```

## Architecture

CuiLianTu（淬炼图）is a Tauri v2 desktop app for batch image optimization and format conversion.

### Frontend (SolidJS + TypeScript)
- **Entry**: `src/main.tsx` → `src/App.tsx`
- **State**: Solid `createStore` in `src/store/useAppStore.ts` (signals + store) manages files, processing options, progress, and results
- **Components**: `src/components/` - DropZone, FileList, SettingsPanel, ResultsPanel, ActionButtons, LanguageSwitcher, PreviewPanel, UpdateNotification
- **i18n**: `src/i18n/` with **three** locales — `en`, `ja`, `zh-CN`

### Backend (Rust)
- **Entry**: `src-tauri/src/lib.rs` registers Tauri commands
- **Commands** (`src-tauri/src/commands/`):
  - `get_image_files` - Scan paths (files/dirs) for supported images
  - `detect_file_types` - Batch-detect real formats from magic bytes (handles HEIC/AVIF/JXL)
  - `process_batch` - Parallel batch processing with progress events
  - `preview_image` / `preview_image_data` - Before/after preview with exact size
  - `cancel_processing` - Stop an in-flight batch
  - `export_report` - Write the collected batch results as a CSV report
  - `save_temp_image` - Park a pasted screenshot in the temp folder
  - `register_allowed_paths` - Whitelist dialog/drop/paste paths (defense-in-depth)
  - `check_for_updates` / `get_current_version` - GitHub release check
- **Image Processing**: `src-tauri/src/image/processor.rs` handles resize and format conversion using `image` and `webp` crates
- **Formats**: `src-tauri/src/image/formats.rs` defines supported input/output formats (JPEG, PNG, GIF, BMP, TIFF, WebP)

### Frontend-Backend Communication
- Frontend calls Rust via `@tauri-apps/api` invoke
- Rust emits `processing-progress` and `processing-complete` events during batch processing
- TypeScript types in `src/types/index.ts` mirror Rust structs for type safety

### Key Dependencies
- **Rust**: `image` (image processing), `webp` (WebP encoding), `rayon` (parallel processing), `walkdir` (directory traversal), `tauri` v2
- **Frontend**: `solid-js` (UI), `@tauri-apps/api` + plugins (dialog/opener/notification), `i18next` (i18n), `cropperjs` (crop UI), `tailwindcss` v4 (styling, devDependency)

## Code Conventions

### Comment language
Code comments, doc comments and config-file comments are written in **English**
across the whole repo (Rust + TypeScript + workflows). User-visible strings
never live in source code — they go through i18n (`src/i18n/locales/*`). The
only sanctioned CJK in source is intentional content: the brand name
(CuiLianTu / 淬炼图), the embedded font name (得意黑), CJK test strings that
exercise CJK rasterization, and the sample-image text that showcases CJK
rendering.

### Facts vs. guesses
When stating technical facts:
- **Verified**: confirmed directly in code or docs → state it plainly
- **Inferred**: derived from logs or context → label it "inferred"
- **Unverified**: no way to check → label it "unverified"

**Forbidden**: presenting guesses as established facts.

### External services
1. Check connectivity first
2. Report failures immediately
3. No silent failures

### Task progress
- One step at a time; confirm each step's completion
- Report intermediate results on multi-step tasks
- Consult the user instead of guessing past a blocker
- State progress and remaining work before ending a session

## Repository-Specific Rules

### The gates

| Stage | Command | Scope |
|---|---|---|
| lint | `biome check .` | format + lint (no Prettier / ESLint) |
| typecheck | `tsc --noEmit` | types |
| test | `vitest run` | frontend tests |
| build | `vite build` | frontend build |
| rustfmt | `cargo fmt --check` | only when src-tauri changed |
| clippy | `cargo clippy --all-targets -- -D warnings` | same |
| rusttest | `cargo test` | same |

`NODE_ENV` is set per stage. Do not fix it for whole scripts
(see the comment at the top of `bin/agent-check`).
Additionally `vite.config.ts` forces `NODE_ENV=test` under vitest, so
calling `pnpm test` directly yields the same result.

### Version pinning

Tool versions are written in **exactly one place**. Never specify them
separately for CI and local.

| Target | Single source | CI side |
|---|---|---|
| Node | `.node-version` | `node-version-file: '.node-version'` |
| pnpm | `packageManager` in package.json | `pnpm/action-setup` (no `version:`) |

CI used to hard-code pnpm 9 / Node 20, which drifted from the local
pnpm 12 / Node 22 and broke CI only. Never keep a value in two places.

CI (`.github/workflows/ci.yml`) runs the same checks.
**Never change only one side.** When local and CI drift, agents wander in a
"CI passes but my machine is red" state.

<!-- daigo-lab-ops:completion-criteria:start -->
<!-- Auto-generated. daigo-lab-ops/docs/completion-criteria.md is the single
     source of truth. Do not edit by hand; regenerate with `lab sync`. -->

## Definition of done for agents

### Definition of done

1. This repository's `bin/agent-check` returns `STATUS: PASS`
2. The change stays within what was asked for
3. The PR is opened from a branch other than main / master

### Do not

- **Never push directly to the default branch.** Always branch and open a PR.
- **Never merge.** `git merge` and `gh pr merge` are a human's job.
- **Never edit the gate to make it pass.** If the gate needs to be relaxed,
  propose that as its own PR and explain why.
- **Never silence a lint rule to get green.** Fix what it reports.

### When opening a PR

- Do not put a Claude session URL (`claude.ai/code/session_...`) or a
  `Claude-Session:` line in the PR body or in any commit message
- **Always name the repository and include the URL when referring to a PR.**
  `#24` alone does not identify anything when several repositories are in play
- Stacked PRs: before merging the base PR, re-target the one stacked on top of
  it to the default branch first (`gh pr edit <n> --base main`). Merging the
  base deletes its branch, and that takes the stacked PR with it. Most of these
  repositories delete the branch on merge automatically, so this is a step you
  have to take, not an option you can decline

<!-- daigo-lab-ops:completion-criteria:end -->
