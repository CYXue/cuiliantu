# CuiLianTu（淬炼图）

<p align="center">
  <img src="./public/icon.png" alt="CuiLianTu" width="128" height="128">
</p>

<p align="center">
  强大的桌面端批量图片优化与格式转换工具。
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

## 功能特性

- **批量处理** — 一次处理数百张图片
- **拖放操作** — 直接拖入文件或文件夹
- **格式转换** — JPEG、PNG、GIF、BMP、TIFF、WebP 互转
- **可定制选项**
  - 质量调节（0-100%）
  - 尺寸调整（宽/高，contain / cover / stretch）
  - 旋转、翻转、量化（减色）
  - 元数据保留
  - 有损/无损压缩
- **目标文件体积** — 二分自动调优 JPEG 质量，直到文件满足 KB 预算（如「上传表单要求 ≤50 KB」）
- **预设模板** — 内置证件照尺寸（小一寸 / 一寸 / 大一寸 / 二寸 / 小二寸 / 社保照）、web、社交、无损等模板，支持保存自定义模板并排序
- **水印** — 文字或图片水印，可调大小、不透明度、旋转角度与边距
- **实时预览** — 转换前即可看到每个滑块（质量、尺寸、水印）的效果；未加载文件时同样可用
- **统计信息** — 查看压缩率（总体、平均、中位数）
- **桌面通知** — 处理完成后弹出系统通知
- **深色模式** — 浅色 / 深色 / 跟随系统
- **多语言** — English、简体中文、日本語
- **跨平台** — macOS 与 Windows
- **隐私优先** — 全部本地运行，不上传、无需账号

## 安装

### 普通用户

无需构建！从 [GitHub Releases](https://github.com/CYXue/cuiliantu/releases) 或 [AtomGit Releases](https://atomgit.com/CYXue/cuiliantu/releases) 下载对应平台的安装包即可。

| 平台 | 文件 | 说明 |
|------|------|------|
| macOS（Apple Silicon） | `CuiLianTu_*_aarch64.dmg` | 适用于 M1/M2/M3 系列 |
| macOS（Intel） | `CuiLianTu_*_x64.dmg` | 适用于 Intel 芯片 |
| Windows | `CuiLianTu_*_x64-setup.exe` | 安装程序（推荐，双击安装，含卸载项） |
| Windows | `CuiLianTu_*_x64.msi` | MSI 安装包（适合企业/管理员部署） |
| Windows | `CuiLianTu_*_x64_portable.zip` | **便携版**：无需安装，解压后双击 `CuiLianTu.exe` 直接使用（zip 内附《使用说明》） |

#### macOS 说明

应用未做代码签名，macOS Gatekeeper 可能会阻止首次启动。按以下步骤打开：

1. 打开 DMG 文件，将应用拖入「应用程序」文件夹
2. 右键点击应用，选择「打开」
3. 在安全弹窗中点击「打开」

也可以在终端移除隔离属性：
```bash
xattr -cr /Applications/CuiLianTu.app
```

### 开发者

想修改或参与贡献，请参阅下方的[开发](#开发)部分。

## 使用方法

1. **添加图片** — 将图片或文件夹拖入拖放区
2. **配置参数**
   - 选择输出格式（推荐 WebP）
   - 调节质量（80% 是较好的平衡点）
   - 按需启用尺寸调整
   - 选择元数据与压缩选项
3. **选择输出目录** — 指定转换后图片的保存位置
4. **开始转换** — 点击「开始转换」，查看进度

## 开发

### 环境准备

工具链版本各自只在一处维护——请从仓库对应文件读取，不要依赖本表格（表格难免滞后）：

| 工具 | 唯一权威来源 | 当前值 |
|------|--------------|--------|
| Node | `.node-version` | 22.22.3 |
| pnpm | `package.json` 的 `packageManager` | 9.15.4 |
| Rust | CI 中的 `dtolnay/rust-toolchain@stable` | stable |

- [Node.js](https://nodejs.org/) — 版本见 `.node-version`
- [pnpm](https://pnpm.io/) — `corepack enable` 会自动使用 `packageManager`
- [Rust](https://www.rust-lang.org/tools/install) — stable，含 `rustfmt` 与 `clippy`

#### macOS

```bash
# 安装 Xcode 命令行工具
xcode-select --install
```

#### Windows

安装 [Visual Studio Build Tools](https://visualstudio.microsoft.com/visual-cpp-build-tools/)，勾选「使用 C++ 的桌面开发」工作负载。

### 从源码构建

```bash
# 克隆仓库
git clone https://github.com/CYXue/cuiliantu.git
# 或（AtomGit 镜像）
git clone https://atomgit.com/CYXue/cuiliantu.git
cd cuiliantu

# 安装依赖
pnpm install

# 以开发模式运行
pnpm tauri dev

# 生产构建
pnpm tauri build
```

### 常用脚本

```bash
# 启动开发服务器
pnpm tauri dev

# 生产构建
pnpm tauri build

# TypeScript + Vite 生产构建
pnpm build

# 前端门禁（与 CI 一致）
pnpm lint          # biome check — 格式 + lint
pnpm typecheck     # tsc --noEmit
pnpm test:run      # vitest run
pnpm test:coverage # vitest run --coverage（强制 vite.config.ts 中的覆盖率阈值）

# Rust 门禁
cd src-tauri && cargo fmt --check
cd src-tauri && cargo clippy --all-targets -- -D warnings
cd src-tauri && cargo test

# 一次跑完所有门禁，带计时（使用本地工具链版本）
./bin/agent-check

# 打包 Windows 便携版 zip（解压即用，需先 tauri build）
pnpm portable

# 切换到开发图标（带 DEV 角标）
pnpm icons:dev

# 恢复生产图标
pnpm icons:prod
```

### 无界面 CLI

`cuiliantu-cli` 与 GUI 共用完全相同的处理管线——同样的格式支持、尺寸调整、水印、量化、目录树镜像与 CSV 报告——适合终端与脚本场景：

```bash
cd src-tauri && cargo run --bin cuiliantu-cli -- --help

# 输入路径为位置参数；-o/--output 必填
cd src-tauri && cargo run --bin cuiliantu-cli -- \
  ~/pics -o ~/out --format webp --quality 80 --report report.csv
```

## 技术栈

- **框架**：[Tauri](https://tauri.app/) v2
- **后端**：Rust（`image`、`webp`、`imageproc`、`ab_glyph`、`rayon`）
- **前端**：[SolidJS](https://www.solidjs.com/) + TypeScript
- **样式**：Tailwind CSS v4
- **状态管理**：Solid signals / store
- **国际化**：i18next

## 项目结构

```
cuiliantu/
├── src/                    # SolidJS 前端
│   ├── components/         # UI 组件（settings/ 存放设置面板各分区）
│   ├── i18n/               # 国际化（en / ja / zh-CN）
│   ├── store/              # Solid store（signals / createStore）
│   ├── types/              # TypeScript 类型（与 Rust 结构体对应）
│   └── utils/              # 共享工具（path、sanitize、slider、format）
├── src-tauri/              # Rust 后端
│   ├── src/
│   │   ├── commands/       # Tauri 命令（含 path_guard.rs，IPC 白名单）
│   │   ├── image/          # 图像处理（processor.rs、edit.rs、formats.rs）
│   │   └── bin/            # 无界面 CLI 前端（cuiliantu-cli）
│   ├── assets/fonts/       # 内嵌水印字体（Smiley Sans，OFL 协议）
│   ├── icons/              # 应用图标
│   ├── capabilities/       # Tauri 权限配置
│   └── tauri.conf.json     # Tauri 配置
├── bin/agent-check         # 本地「完成定义」检查脚本
├── docs/                   # 调研笔记与审计文档
├── public/                 # 静态资源
├── scripts/                # 构建脚本
└── .github/
    └── workflows/          # CI/CD 工作流
```

## 参与贡献

欢迎贡献！提交 Pull Request 即可：

1. Fork 本仓库
2. 创建功能分支（`git checkout -b feature/amazing-feature`）
3. 提交改动（`git commit -m 'Add some amazing feature'`）
4. 推送分支（`git push origin feature/amazing-feature`）
5. 发起 Pull Request

## 许可证

MIT License — 详见 [LICENSE](LICENSE)。

## 致谢

- [Tauri](https://tauri.app/) — 桌面应用框架
- [image-rs](https://github.com/image-rs/image) — Rust 图像处理库
- [webp](https://crates.io/crates/webp) — WebP 编码库
