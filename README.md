# CuiLianTu（淬炼图）

<p align="center">
  <img src="./public/icon.png" alt="CuiLianTu" width="128" height="128">
</p>

<p align="center">
  A powerful desktop application for batch image optimization and format conversion.
</p>

<p align="center">
  <a href="https://github.com/CYXue/cuiliantu/releases">
    <img src="https://img.shields.io/github/v/release/CYXue/cuiliantu" alt="Release">
  </a>
  <a href="https://github.com/CYXue/cuiliantu/actions/workflows/ci.yml">
    <img src="https://github.com/CYXue/cuiliantu/actions/workflows/ci.yml/badge.svg" alt="CI">
  </a>
  <a href="https://atomgit.com/CYXue/cuiliantu/releases">
    <img src="https://img.shields.io/badge/release-atomgit-cuiliantu-blue" alt="AtomGit Release">
  </a>
  <a href="./LICENSE">
    <img src="https://img.shields.io/badge/license-MIT-green" alt="License">
  </a>
</p>

## Features

- **Batch Processing** - Process hundreds of images at once
- **Drag & Drop** - Simply drop files or folders
- **Format Conversion** - Convert between JPEG, PNG, GIF, BMP, TIFF, and WebP
- **Customizable Options**
  - Quality adjustment (0-100%)
  - Resize (width/height, contain / cover / stretch)
  - Rotate, flip, and quantize
  - Metadata preservation
  - Lossy/Lossless compression
- **Target File Size** - Auto-tune JPEG quality by binary search until the file fits a KB budget (e.g. "≤50 KB for an upload form")
- **Presets** - Built-in templates (ID photo sizes: 小一寸 / 一寸 / 大一寸 / 二寸 / 小二寸 / 社保照, web, social, lossless) plus your own saved and reorderable templates
- **Watermark** - Text or image watermark with size, opacity, rotation and margin
- **Live Preview** - See the effect of every slider (quality, size, watermark) before converting; works even with no file loaded
- **Statistics** - View reduction rates (overall, average, median)
- **Desktop Notifications** - Get notified when processing completes
- **Dark Mode** - Light / dark / follow system
- **Multi-language** - English, 简体中文 and 日本語
- **Cross-platform** - macOS and Windows
- **Private by design** - Everything runs locally; no upload, no account

## Installation

### For Users

No build required! Download the installer for your platform from [GitHub Releases](https://github.com/CYXue/cuiliantu/releases) or [AtomGit Releases](https://atomgit.com/CYXue/cuiliantu/releases).

| Platform | File | Notes |
|----------|------|-------|
| macOS (Apple Silicon) | `CuiLianTu_*_aarch64.dmg` | For M1/M2/M3 Macs |
| macOS (Intel) | `CuiLianTu_*_x64.dmg` | For Intel Macs |
| Windows | `CuiLianTu_*_x64-setup.exe` | Installer |
| Windows | `CuiLianTu_*_x64.msi` | MSI package |

#### macOS Notes

The app is not code-signed, so macOS Gatekeeper may block it from launching. Follow these steps to open the app:

1. Open the DMG file and drag the app to the Applications folder
2. Right-click (or Control-click) the app and select "Open"
3. Click "Open" in the security dialog

Alternatively, remove the quarantine attribute via Terminal:
```bash
xattr -cr /Applications/CuiLianTu.app
```

### For Developers

If you want to modify or contribute to the project, see the [Development](#development) section below.

## Usage

1. **Add Images** - Drag and drop images or folders into the drop zone
2. **Configure Settings**
   - Select output format (WebP recommended)
   - Adjust quality (80% is a good balance)
   - Enable resize if needed
   - Choose metadata and compression options
3. **Select Output Directory** - Choose where to save converted images
4. **Start Conversion** - Click "Start Conversion" and watch the progress

## Development

### Prerequisites

Toolchain versions live in exactly one place each — read them from the repo
rather than from this table, which would inevitably drift:

| Tool | Single source of truth | Current value |
|---|---|---|
| Node | `.node-version` | 22.22.3 |
| pnpm | `packageManager` in `package.json` | 9.15.4 |
| Rust | `dtolnay/rust-toolchain@stable` in CI | stable |

- [Node.js](https://nodejs.org/) — see `.node-version`
- [pnpm](https://pnpm.io/) — `corepack enable` picks up `packageManager`
- [Rust](https://www.rust-lang.org/tools/install) — stable, plus `rustfmt` and `clippy`

#### macOS

```bash
# Install Xcode Command Line Tools
xcode-select --install
```

#### Windows

Install [Visual Studio Build Tools](https://visualstudio.microsoft.com/visual-cpp-build-tools/) with the "Desktop development with C++" workload.

### Build from Source

```bash
# Clone the repository
git clone https://github.com/CYXue/cuiliantu.git
# 或（AtomGit 镜像）
git clone https://atomgit.com/CYXue/cuiliantu.git
cd cuiliantu

# Install dependencies
pnpm install

# Run in development mode
pnpm tauri dev

# Build for production
pnpm tauri build
```

### Scripts

```bash
# Start development server
pnpm tauri dev

# Build for production
pnpm tauri build

# TypeScript + Vite production build
pnpm build

# Frontend gates (the same ones CI runs)
pnpm lint          # biome check — format + lint
pnpm typecheck     # tsc --noEmit
pnpm test:run      # vitest run
pnpm test:coverage # vitest run --coverage (enforces the thresholds in vite.config.ts)

# Rust gates
cd src-tauri && cargo fmt --check
cd src-tauri && cargo clippy --all-targets -- -D warnings
cd src-tauri && cargo test

# Everything at once, with timings (uses the local toolchain versions)
./bin/agent-check

# Switch to dev icons (with DEV ribbon)
pnpm icons:dev

# Switch to production icons
pnpm icons:prod
```

### Head-less CLI

`cuiliantu-cli` runs the exact same pipeline as the GUI — same formats,
resize, watermark, quantize, tree mirroring and CSV report — for terminals and
scripts:

```bash
cd src-tauri && cargo run --bin cuiliantu-cli -- --help

# inputs are positional; -o/--output is required
cd src-tauri && cargo run --bin cuiliantu-cli -- \
  ~/pics -o ~/out --format webp --quality 80 --report report.csv
```

## Tech Stack

- **Framework**: [Tauri](https://tauri.app/) v2
- **Backend**: Rust (`image`, `webp`, `imageproc`, `ab_glyph`, `rayon`)
- **Frontend**: [SolidJS](https://www.solidjs.com/) + TypeScript
- **Styling**: Tailwind CSS v4
- **State Management**: Solid signals / store
- **i18n**: i18next

## Project Structure

```
cuiliantu/
├── src/                    # SolidJS frontend
│   ├── components/         # UI components (settings/ holds the panel sections)
│   ├── i18n/               # Internationalization (en / ja / zh-CN)
│   ├── store/              # Solid store (signals / createStore)
│   ├── types/              # TypeScript types mirroring the Rust structs
│   └── utils/              # Shared helpers (path, sanitize, slider, format)
├── src-tauri/              # Rust backend
│   ├── src/
│   │   ├── commands/       # Tauri commands (incl. path_guard.rs, the IPC allow-list)
│   │   ├── image/          # Image processing (processor.rs, edit.rs, formats.rs)
│   │   └── bin/            # Head-less CLI front-end (cuiliantu-cli)
│   ├── assets/fonts/       # Embedded watermark font (Smiley Sans, OFL)
│   ├── icons/              # App icons
│   ├── capabilities/       # Tauri permissions
│   └── tauri.conf.json     # Tauri configuration
├── bin/agent-check         # Local "definition of done" runner
├── docs/                   # Research notes and audits
├── public/                 # Static assets
├── scripts/                # Build scripts
└── .github/
    └── workflows/          # CI/CD workflows
```

## Contributing

Contributions are welcome! Please feel free to submit a Pull Request.

1. Fork the repository
2. Create your feature branch (`git checkout -b feature/amazing-feature`)
3. Commit your changes (`git commit -m 'Add some amazing feature'`)
4. Push to the branch (`git push origin feature/amazing-feature`)
5. Open a Pull Request

## License

MIT License - see [LICENSE](LICENSE) for details.

## Acknowledgments

- [Tauri](https://tauri.app/) - Desktop app framework
- [image-rs](https://github.com/image-rs/image) - Rust image processing library
- [webp](https://crates.io/crates/webp) - WebP encoding library
