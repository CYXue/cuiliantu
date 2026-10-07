# 跨平台兼容性审查报告 — image-crunch

> 注：本项目已改名为 **TuCui（图淬）**，本报告中出现的 `image-crunch` / `Image Crunch` 均为改名前的记录，未作替换。

审查日期：2026-09-26
审查范围：Rust 后端（`src-tauri/src` 全部 `.rs` + `Cargo.toml` + `tauri.conf.json` + `capabilities/`）、前端（`src` 全部 `.ts/.tsx`）、构建配置（`vite.config.ts` / `package.json` / `biome.json` / `index.html` / `tsconfig*`）、开发脚本（`scripts/`）、版本库配置（`.gitattributes`）。
审查方法：逐文件通读 + 全局模式扫描（`cfg(target_os)`、`process.platform`、`C:\`/`/Users/`/`/home/` 绝对路径、`localhost`/`127.0.0.1`、`.sh` 脚本引用、`\r\n` 处理）。

---

## 一、总览

| 等级 | 数量 | 关键项 |
|------|------|--------|
| 🔴 高 | 0 | — |
| 🟠 中 | 4 | Windows 保留设备名、`.sh` 脚本 Windows 不可执行、bundle targets 混列三平台、reqwest native-tls 的 Linux OpenSSL 依赖 |
| 🟡 低 | 3 | 端口四处耦合、NSIS 缺中文、biome 换行约定未显式化 |
| ℹ️ 信息 | 2 | GITHUB_REPO 外网依赖（=F2 遗留，暂缓）、pnpm 依赖 corepack |
| ✅ 通过 | — | 无 OS 判断分支、无硬编码绝对路径、路径拼接全用 `Path::join`、`basename()` 双分隔符兼容、换行解析 CRLF 兼容、测试用 `temp_dir()`、`TAURI_DEV_HOST` 已支持 |

**已全部修复（4 中 + 3 低）；2 项信息级记录在案。**

---

## 二、分类问题清单（含严重程度、行号、修复状态）

### A. 地址与端口

#### CP-1 🟡 低 · dev 端口四处耦合（已通过文档化收口）
- **位置**：
  - `src-tauri/tauri.conf.json:8` — `"devUrl": "http://localhost:1420"`
  - `vite.config.ts:18` — `port: 1420`；`:25` — HMR `port: 1421`
  - `src-tauri/tauri.conf.json:29-36` — `devCsp` 中 `ws://localhost:1420` 等（本轮 F1 引入）
- **为何影响跨平台/协作**：均为 **dev-only** 取值，不锁运行环境（生产走 Tauri 自定义协议）。但同一端口散布 4 处，换端口时漏改任意一处会导致 dev 起不来或 HMR/CSP violation，且这类错误只在特定平台/流程下暴露。
- **处置**：保持 Tauri 惯例端口 1420/1421（`strictPort` 即官方要求）；**已将「改 dev 端口需同步 4 处」写入自查清单**。远程/移动调试已支持：`vite.config.ts:6,20-27` 读取 `TAURI_DEV_HOST` 环境变量 ✅。
- **不建议**改为环境变量注入 devUrl——Tauri 生成 context 时需要确定值，环境变量反而引入"忘了设就构建错"的新坑。

#### CP-2 ℹ️ 信息 · 更新检查绑定外部 GitHub 仓库
- **位置**：`src-tauri/src/commands/update_commands.rs:3` — `GITHUB_REPO = "daigotanaka0714/image-crunch"`
- **说明**：外网 API 依赖与平台无关，不是跨平台缺陷；但属上次审查 F2（原作者标识，用户决定暂缓），此处仅登记。发版前必须改为自己的仓库，否则更新检查永远查错仓库。

### B. 文件路径

#### CP-3 🟠 中 · 输出文件名未防 Windows 保留设备名（已修）
- **位置**：`src-tauri/src/image/processor.rs:849-856`（`output_filename` 的清洗段）
- **为何影响跨平台**：Windows 把 `CON`、`PRN`、`AUX`、`NUL`、`COM1-9`、`LPT1-9` 视为设备名，**即使带扩展名**（`NUL.png` 就是 NUL 设备）。源文件名恰为 `nul.png`/`con.jpg` 时，`{name}` 模板输出 `NUL.png` → 写入静默丢失/失败，且仅在 Windows 上复现，Linux/macOS 开发时无法察觉。
- **修复**：新增 `is_windows_reserved_device_name()`（processor.rs:928-938），对最终名字的设备段（首个 `.` 之前）做大小写不敏感匹配，命中则追加 `_`（`NUL.png` → `NUL_.png`）；含新增单测 `windows_reserved_output_names_get_an_suffix`。原 `..` 折叠与分隔符过滤（F8）保持不变。

#### CP-4 🟡 低 · 换行符约定未全链路固定（已补齐）
- **位置**：`.gitattributes`（已存在，本轮补 `*.ttf/*.woff/*.woff2 binary`）、`biome.json:17-21`
- **为何影响跨平台**：Windows 的 `core.autocrlf=true` 会把工作区检出为 CRLF；biome 默认 `lineEnding=lf`，于是 Windows 贡献者每次保存都被格式化工具整文件改写 → diff 噪音、CI 与本地不一致。仓库虽有 `.gitattributes`（`* text=auto eol=lf`），但 formatter 配置未显式声明约定，且缺字体二进制条目（TTF 若被误判文本损坏后果严重）。
- **修复**：`biome.json` 显式 `"lineEnding": "lf"`；`.gitattributes` 补齐 `.ttf/.woff/.woff2 binary`。
- **代码层通过项**：文本水印解析 `text.split('\n')` + `trim_end_matches('\r')`（processor.rs:661-663），CRLF 输入不产生残留 `\r` 字形 ✅。

#### ✅ 路径处理通过项（grep 全量验证）
- **无硬编码绝对路径**：`C:\`、`C:/`、`/Users/`、`/home/`、`/tmp/`、`/var/`、`/etc/` 全仓零命中（排除 node_modules）。
- **路径拼接**：Rust 全程 `Path/PathBuf::join`（`resolve_output_path`、`output_dir.join`），无手拼分隔符。
- **文件名显示**：前端 `basename()` 用 `split(/[/\\]/)` 同时兼容两种分隔符（DropZone.tsx:26、WatermarkSection.tsx:104 等多处实现一致）。
- **测试**：`std::env::temp_dir()` + 进程 ID 组合（processor.rs:904、image_commands.rs tests），无固定路径。
- **资源内嵌**：水印字体 `include_bytes!("../../assets/fonts/SmileySans-Oblique.ttf")` 为编译期相对路径，产物不依赖文件系统布局。

### C. 操作系统相关

#### CP-5 🟠 中 · npm 图标脚本调用 `.sh`，Windows 直接失效（已修）
- **位置**：`package.json:19-21`（原 `"icons:dev": "./scripts/use-dev-icons.sh"` 等三条）
- **为何影响跨平台**：npm/pnpm 在 Windows 用 `cmd.exe` 执行 scripts，**无法运行 bash 脚本**（除非装 Git Bash 并手动 sh 调用）。三个脚本还叠加重度平台依赖：`cp -r`、`BASH_SOURCE`、ImageMagick `convert`/`identify`、macOS-only `iconutil`。在 Windows 上 `icons:dev/prod/generate` 三条命令全部报错——开发工作流级锁死。
- **修复**：重写为 **`scripts/icons.mjs`**（Node 22，零第三方依赖），`package.json` 改为 `node scripts/icons.mjs dev|prod|generate`；删除 3 个 `.sh`。
  - `dev`/`prod`：纯 `fs.cpSync/copyFileSync` 实现，三平台原生可跑；
  - `generate`：PNG 尺寸直接解析 IHDR 头（免掉 `identify` 依赖）；ImageMagick 优先探测 `magick`（IM7）并校验版本输出——**规避 Windows `C:\Windows\System32\convert.exe` 同名陷阱**；`iconutil` 仅 `process.platform === "darwin"` 且存在时调用；缺工具时打日志跳过而非中断。
  - 行为与原 bash 脚本对齐（备份/恢复逻辑、ribbon 参数、ico auto-resize 档位）。
- **验证**：`node --check` 语法通过；`icons.mjs prod`/缺参用法提示实测正常（本机 Windows）。

#### CP-6 🟠 中 · bundle targets 混列三平台目标（已修）
- **位置**：`src-tauri/tauri.conf.json:50`（原 `"targets": ["dmg", "app", "msi", "nsis", "deb", "appimage"]`）
- **为何影响跨平台**：显式列表把 macOS（dmg/app）、Windows（msi/nsis）、Linux（deb/appimage）目标写死在同一份配置，单一宿主机构建时对不支持的目标要么报错要么产出空跑，CI 换平台就翻车。
- **修复**：改为 `"targets": "all"` —— Tauri 官方推荐写法，构建时自动选择当前平台支持的全部目标（macOS→dmg/app，Windows→msi/nsis，Linux→deb/appimage）。

#### CP-7 🟡 低 · NSIS 安装器语言缺中文（已修）
- **位置**：`src-tauri/tauri.conf.json:69`（原 `["English", "Japanese"]`）
- **为何影响跨平台**：应用 UI 支持zh-CN/i18n 三语，Windows 安装器却不提供中文选项——不是"锁死"，但是平台交付物与产品语言能力不一致。
- **修复**：加入 `"SimpChinese"`（NSIS 官方语言名，已核实 Tauri 文档/社区用法），保持 `displayLanguageSelector: true`。

#### ✅ OS 相关通过项
- **无平台分支硬编码**：`cfg(target_os)` / `cfg(windows)` / `cfg(unix)` / `process.platform` / `navigator.platform` 前后端均零命中（`scripts/icons.mjs` 中的 `process.platform === "darwin"` 是本轮新增的显式守卫，符合规范）。
- **`main.rs:2`** `windows_subsystem = "windows"`（release 隐藏控制台）是 Windows 标准做法，非缺陷。
- **`tauri.conf.json:63`** macOS `entitlements` 在其他平台被忽略，无害。
- **拖拽/通知/打开链接**均走 Tauri 官方插件（dialog/fs/notification/opener），插件内部处理平台差异。

### D. 构建与依赖

#### CP-8 🟠 中 · reqwest 默认 native-tls：Linux 构建环境锁（已修）
- **位置**：`src-tauri/Cargo.toml:49`（原 `reqwest = { version = "0.12", features = ["json"] }`）
- **为何影响跨平台**：reqwest 默认特性启用 native-tls——Windows 用 SChannel、macOS 用 Security.framework、**Linux 用 OpenSSL，要求构建机预装 pkg-config + libssl-dev**。同一份代码在三平台的 TLS 行为与构建前置不同，Linux CI/容器/干净机器上直接编译失败，是典型"环境假设"。
- **修复**：`default-features = false, features = ["json", "rustls-tls"]` —— 纯 Rust TLS（ring），三平台零系统库依赖、行为一致。**验证**：`cargo check` 27s 通过（hyper-rustls/reqwest 重新编译成功），`cargo test` 30/30 通过。

#### CP-9 ℹ️ 信息 · 构建入口依赖 pnpm + corepack
- **位置**：`package.json:6` `"packageManager": "pnpm@9.15.4"`；`tauri.conf.json:7,9` `beforeDevCommand`/`beforeBuildCommand` 直接调 `pnpm`
- **说明**：`packageManager` 字段 + corepack（`corepack enable`）即可固定 pnpm 版本，Node 22 自带 corepack，属可接受约定。建议在 README/CI 注明 `corepack enable` 前置步骤；不建议为此引入更多抽象。

#### ✅ 依赖通过项
- `image`/`webp`/`ab_glyph`/`imageproc`/`rayon`/`walkdir`/`thiserror` 均为纯 Rust 或 cc 可编译 crate，MSVC/gcc/clang 三平台无碍。
- 线程数 `available_parallelism()`（image_commands.rs:143）运行时探测，无硬编码核数。
- 无 `std::process::Command`、无 shell 拼接、无 `env::var` 默认值假设、无时区/本地时间硬编码（应用不消费时间戳）。

---

## 三、跨平台自查清单（Checklist）

**地址与端口**
- [x] 生产代码无硬编码 IP/域名/端口；`localhost:1420/1421` 仅存在于 devUrl/vite/devCsp（dev-only）
- [ ] 改 dev 端口时同步 4 处：`vite.config.ts` server.port、hmr.port、`tauri.conf.json` devUrl、devCsp
- [x] 远程调试走 `TAURI_DEV_HOST` 环境变量（vite.config.ts 已支持）
- [x] 外部 API 域名集中为常量（update_commands.rs），发版前替换为自己的仓库（F2）

**文件路径**
- [x] 路径拼接一律 `Path::join` / `path.join`，禁止手拼 `/` 或 `\`
- [x] 展示文件名用兼容双分隔符的 `basename()`（`split(/[/\\]/)`）
- [x] 输出文件名防路径遍历（`..` 折叠）+ Windows 保留设备名（追加 `_`）
- [x] 测试用 `std::env::temp_dir()`，无固定路径
- [x] 大资源用 `include_bytes!` 内嵌，不依赖运行时文件布局

**换行与编码**
- [x] `.gitattributes`：`* text=auto eol=lf` + 全部二进制类型标 `binary`（含字体）
- [x] formatter 显式声明换行约定（biome `lineEnding: "lf"`；rustfmt 默认 LF）
- [x] 解析外部文本时兼容 CRLF（split `\n` + trim `\r`）
- [x] 文件读写均为二进制语义（Rust `fs::read/write`），无隐式编码转换

**操作系统相关**
- [x] 平台差异必须显式声明：Rust 用 `#[cfg(target_os)]`，JS 用 `process.platform`，默认路径兜底
- [x] npm scripts 禁止直接调 `.sh`/`.bat`/`.ps1` —— 统一 `node scripts/*.mjs`
- [x] 调外部命令先探测可用性，且避开 Windows 同名陷阱（`convert.exe` vs ImageMagick）
- [x] bundle targets 用 `"all"`，不混列各平台专属目标
- [x] 安装器语言与产品 i18n 能力对齐（NSIS 已含 SimpChinese）

**构建与依赖**
- [x] 网络库优先纯 Rust TLS（rustls），避免 Linux OpenSSL 系统库依赖
- [x] 不硬编码 CPU 核数/内存等运行环境值（用 `available_parallelism()` 等）
- [x] pnpm 版本由 `packageManager` 字段 + corepack 固定，CI 需先 `corepack enable`
- [x] 无 `std::process::Command` / shell 拼接 / 环境变量默认值假设
- [ ] （建议）CI 在 Windows/macOS/Linux 三平台各跑一次 `cargo test` + `pnpm typecheck` + `pnpm test:run`

---

## 四、验证记录（2026-09-26）

| 验证项 | 结果 |
|--------|------|
| `cargo check`（rustls 依赖切换后） | ✅ 27s 通过 |
| `cargo test` | ✅ 30/30（含新增 `windows_reserved_output_names_get_an_suffix`） |
| `pnpm typecheck`（tsc --noEmit） | ✅ 0 错误 |
| `pnpm test:run`（vitest） | ✅ 14/14 |
| `node --check scripts/icons.mjs` | ✅ 语法通过 |
| `node scripts/icons.mjs prod` 冒烟（Windows 实跑） | ✅ 正确 no-op + 提示 |
| `tauri.conf.json` / `biome.json` / `package.json` JSON 合法性 | ✅ |
| 全局 grep：绝对路径 / OS 判断 / `.sh` 引用残留 | ✅ 零残留 |

**遗留人工确认**：① `pnpm tauri dev` 冒烟（含 CSP，见上一份报告）；② macOS/Linux 侧建议至少跑一次 `cargo test` 与 `pnpm tauri build` 确认 `targets: "all"` 与 rustls 行为（本机为 Windows，已实测 Windows 侧）。
