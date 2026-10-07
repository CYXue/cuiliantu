# 代码审查报告 — image-crunch

> 注：本项目已改名为 **TuCui（图淬）**，本报告中出现的 `image-crunch` / `Image Crunch` 均为改名前的记录，未作替换。

审查日期：2026-09-26
审查范围：整个 `image-crunch` 项目（Rust 后端 `src-tauri/src` 共 9 个 `.rs`、前端 `src` 共 24 个 `.ts/.tsx`、配置文件 `tauri.conf.json` / `Cargo.toml` / `capabilities/default.json` / `vite.config.ts` / `package.json`）。
审查重点：硬编码（路径/端口/密钥/URL/配置值/超时）、魔法数字、输入校验缺失、资源未释放、错误处理缺失、命令/SQL 拼接、敏感信息明文、环境假设。

---

## 一、总体结论

| 等级 | 数量 | 关键项 |
|------|------|--------|
| 🔴 高危 | 0 | 未发现 RCE / 命令注入 / SQL 注入 / 密钥泄露 |
| 🟠 中危 | 6 | CSP 关闭、原作者标识/仓库、任意路径+符号链接、网络无超时、水印参数未校验、resize=0 未兜底 |
| 🟡 低危 | 9 | 残留写死颜色、路径遍历、大文件全量读内存、i18n 监听器泄漏、魔法数字、日志泄露路径、get_image_info 不一致、通知标题硬编码、rustfmt |
| 🟢 通过 | — | 无命令/SQL 拼接、无敏感信息明文、前端监听清理良好、生产代码无 `unwrap/expect/panic/unsafe`、签名匹配有边界检查 |

**未发现**：命令注入（`std::process::Command` 不存在）、SQL 拼接（无数据库）、`eval` / `innerHTML` 动态注入、密钥/密码/token 明文存储。

---

## 二、中危发现（建议优先修复）

### F1. 内容安全策略被完全关闭（中高危）
- **位置**：`src-tauri/tauri.conf.json:24-26`
  ```json
  "security": { "csp": null }
  ```
- **风险**：`csp: null` 意味着 WebView 不施加任何内容安全策略。当前前端没有 `innerHTML`/`eval`，直接 XSS 风险低；但应用会向 GitHub API 发起网络请求并通过 `openUrl` 打开外部链接（`UpdateNotification.tsx:42` 的 `release_url` 来自网络响应）。一旦未来引入任何动态 HTML 或第三方脚本，将无任何兜底。
- **建议**：至少配置 `default-src 'self' 'unsafe-inline'`（Tauri v2 内联脚本需 `unsafe-inline` 或 nonce）；如启用外部资源，显式白名单 `https://api.github.com`。

### F2. 原作者标识 / 仓库未清理，更新检查指向他人仓库（中危 · 合规 + 功能）
- **位置**：
  - `src-tauri/tauri.conf.json:3,5` — `"productName": "Image Crunch"`、`"identifier": "com.daigotanaka.image-crunch"`
  - `src-tauri/Cargo.toml:5,8` — `authors = ["daigotanaka0714"]`、`repository = "https://github.com/daigotanaka0714/image-crunch"`
  - `src-tauri/src/commands/update_commands.rs:3` — `const GITHUB_REPO: &str = "daigotanaka0714/image-crunch";`
  - `src/components/ActionButtons.tsx:52` — `sendNotification({ title: "Image Crunch", ... })`
- **风险**：① 闭源/上架时产品标识与作者不一致，违反 MIT「保留版权」但需更新归属的意图；② **更新检查会查询原作者的 GitHub releases**，用户自己发布后永远收不到更新通知；③ 通知标题硬编码产品名，改名时易遗漏。
- **建议**：用统一常量替换。将 `GITHUB_REPO` / `productName` / `identifier` / `authors` / `repository` / 通知标题收敛到一处配置（如 `tauri.conf.json` 的 `identifier` + 一个 `APP_NAME` Rust 常量）。此问题此前已记录在待办（用户尚未提供反向域名）。

### F3. Rust 命令接受任意路径 + `get_image_files` 跟随符号链接（中危 · 路径/权限）
- **位置**：
  - `src-tauri/src/commands/image_commands.rs:80-112`（`get_image_files`）第 94-95 行：
    ```rust
    WalkDir::new(path).follow_links(true)  // 跟随符号链接
    ```
  - `process_batch` / `process_single_image` / `get_image_info` / `detect_file_type` / `preview_image` 均直接 `std::fs::{read,write,metadata,open}` 操作前端传入的任意 `path` / `output_dir`。
- **风险**：① Rust 命令绕过了 Tauri 的 `fs` 权限 scope（`capabilities/default.json` 里的 `fs:default` 只约束 JS 侧 `tauri-plugin-fs`，不约束 Rust 侧 `std::fs`），等于任意路径读写；② `follow_links(true)` 会使指向 `/etc`、`C:\Windows` 等的符号链接被递归遍历并加入处理队列（DoS / 隐私读取）。walkdir 虽有循环检测，但仍会进入链接目标。
- **建议**：默认 `follow_links(false)`；如必须支持，加递归深度上限。对 `output_dir` 增加白名单/落盘前校验（例如拒绝绝对系统目录）。当前产品定位是本地信任前端，可先以注释明确「命令假定前端输入可信」，并关闭符号链接跟随。

### F4. 更新检查网络请求无超时（中危 · 可用性）
- **位置**：`src-tauri/src/commands/update_commands.rs:37-43`
  ```rust
  let client = reqwest::Client::new();
  let response = client.get(&url).send().await ...   // 无 .timeout()
  ```
- **风险**：启动时 `UpdateNotification.onMount` 调用 `check_for_updates`（`UpdateNotification.tsx:22-37`）。网络不可达/挂起时 `await` 会一直阻塞，UI 卡在「检查更新中」（直到底层 TCP 超时，可能数分钟）。`finally` 无法在 await 卡死时执行。
- **建议**：`reqwest::Client::builder().timeout(Duration::from_secs(10)).build()`。

### F5. 水印参数后端未校验（中危 · 输入校验）
- **位置**：`src-tauri/src/image/processor.rs:613-615`
  ```rust
  let factor = opacity as f32 / 100.0;
  for px in overlay.pixels_mut() {
      px.0[3] = ((px.0[3] as f32) * factor).round() as u8;  // opacity 未 clamp(0,100)
  }
  ```
- **风险**：前端 UI 滑块限定 `min=1 max=100`，但 `WatermarkConfig` 经 `invoke` 直接反序列化；若 `opacity` 传入 >100（如 255），`factor=2.55`，`255*2.55=650 → as u8` 截断为 `138`，颜色异常且非预期。`scale_percent` / `margin_percent` 亦无校验（`margin_percent` 过大只会把水印推到画面外，影响较小）。`quality`、`resize_percent` 等其它字段同理依赖前端约束。
- **建议**：在 `prepare_watermark` / `apply_pipeline` 入口统一 clamp：`opacity.clamp(0,100)`、`scale_percent.clamp(1,100)`、`margin_percent.clamp(0,100)`。

### F6. 像素模式 resize 的 width/height=0 无兜底（中危 · 输入校验）
- **位置**：`src-tauri/src/image/processor.rs:526-548`（`apply_resize` 的 `Pixels` 分支）
  ```rust
  (Some(w), Some(h)) => match options.fit_mode {
      FitMode::Stretch => img.resize_exact(w, h, filter),  // w/h 可能为 0
  ```
- **风险**：`ResizeSection.tsx` 的 number input `onInput` 直接 `parseInt(...)`，空串转 `null`，但输入 `0` 会传 `Some(0)`。后端对 `resize_percent` 有 `.max(1)` 兜底（line 522-523），**像素模式却无 `.max(1)`**，`resize_exact(0, 0)` 在 image crate 下行为未定义，可能 panic 或产生空图。
- **建议**：后端 `apply_resize` 像素分支加 `let w = w.max(1); let h = h.max(1);`；前端 number input 显式 `min="1"`。

---

## 三、低危发现

### F7. 残留写死颜色未走主题变量（低危 · 维护一致性）
- **位置**：`src/App.tsx:22`（`bg-[#f5f6f8] dark:bg-[#0b0d12]`）、`DropZone.tsx:185,234,250`（`dark:bg-[#161b22]`）、`LanguageSwitcher.tsx:12`、`ThemeToggle.tsx:20`。
- **风险**：上轮深色模式改造已在 `App.css` 定义 `--surface-app` / `--surface-card` 等变量，但部分组件仍用字面量，导致深色配色与主题变量可能轻微脱节，且日后改主题需多处改。功能正常。
- **建议**：改用 `dark:bg-[var(--surface-card)]` 等，与深色模式变量体系统一。

### F8. filename_pattern 路径遍历（低危）
- **位置**：`src-tauri/src/image/processor.rs:824-837`（`output_filename`）
  ```rust
  .chars().map(|c| if c == '/' || c == '\\' { '_' } else { c })  // 仅过滤 / 和 \
  ```
- **风险**：仅过滤 `/` `\`，不处理 `..`。若原文件名恰为 `..`（如 `..png`），stem=`..`，pattern=`{name}` → 输出名 `..` → `dir.join("..")` 指向父目录（在 `AutoRename`/`Skip` 策略下可能写到父目录）。Windows 保留名（CON/AUX/NUL/PRN/COM1）亦未处理。触发需文件名恰为 `..`，概率低。
- **建议**：过滤连续 `.`、拒绝 `..`；或使用 `sanitize_filename` 风格白名单仅保留安全字符。

### F9. 嗅探时整文件读入内存（低危 · 资源）
- **位置**：`detect_file_type`（`image_commands.rs:375` `std::fs::read(path)`）、`get_image_info`（`image_commands.rs:320` `image::open`）。
- **风险**：为判断真实格式，把整个文件（可能数百 MB 的 TIFF/raw）读入内存。仅头部若干字节即可判断 magic bytes。
- **建议**：`detect_format` 只读前 12–32 字节（`std::fs::read` 改为 `read_to_end` 限制，或 `File::open`+`read_exact` 固定长度）。

### F10. i18n `languageChanged` 监听器每次 `useTranslation()` 累积（低危 · 泄漏）
- **位置**：`src/i18n/index.ts:42-45`
  ```ts
  i18n.on("languageChanged", (l) => { setLang(l); localStorage.setItem(STORAGE_KEY, l); });
  ```
- **风险**：每个调用 `useTranslation()` 的组件都新注册一个监听器，无 `off`。组件多时累积（动作轻量，仅 setLang + 写 localStorage，但属泄漏模式）。
- **建议**：在 `useTranslation` 内 `onCleanup(() => i18n.off("languageChanged", handler))`，或模块级只订阅一次。

### F11. 魔法数字散布（低危 · 可维护性）
- `processor.rs:192` `font_size.clamp(8, 400)` 与 `WatermarkSection.tsx:241-242` 的 `min=8 max=400` **双份定义**，改其一易遗漏；`pad = 4u32`（processor.rs:202）。
- `SettingsPanel.tsx:39,47,51` `50 * 1024` / `300 * 1024` 预设魔法数字。
- `PreviewPanel.tsx:23,59,115` 初始 `slider=50`、防抖 `300ms`、`height:"320px"`。
- `templateImage.ts:9-10,28,46,62-65` `w=480/h=360`、`x+=40`、`arc(70+i*60,300,20)`、`38px`/`18px` 绘制常量。
- `image_commands.rs:422` `PREVIEW_MAX_SIDE = 512`（已 const，较好）。
- **建议**：共享边界（字体 8–400、质量 1–100）抽为常量/类型级校验；UI 尺寸常量集中。

### F12. 错误日志泄露完整文件路径（低危 · 隐私）
- **位置**：`DropZone.tsx:46,75,152,172`、`ActionButtons.tsx:156`、`PreviewPanel.tsx:53` 等多处 `console.error("...", path, error)`。
- **风险**：本地桌面应用影响有限；但若日志被外部采集（崩溃上报等），会暴露用户目录结构。
- **建议**：生产构建剥离详细路径，或仅记录文件名（`basename`）。

### F13. `get_image_info` 已注册但前端未调用，且内部不含 magic-byte 嗅探（低危 · 死代码/不一致）
- **位置**：`src-tauri/src/commands/image_commands.rs:316-334`（`get_image_info` 用 `image::open`，不含 `with_guessed_format`）。
- **风险**：与 `detect_file_type` / `render`（均用 `with_guessed_format`，如 processor.rs:489）行为不一致——对扩展名与真实格式不符的文件，`get_image_info` 会失败。该命令前端从未调用（`grep` 确认仅 `preview_image_data` 被 PreviewPanel 使用），属于冗余/潜在陷阱。
- **建议**：删除该命令，或改为 `with_guessed_format` 以与管线一致。

### F14. `preview_image` / `preview_image_data` 缩进不符 rustfmt（极低 · 整洁）
- **位置**：`image_commands.rs:446,475` 两个函数体用 2 空格，文件其余为 4 空格。
- **风险**：无功能影响，仅格式；`cargo fmt` 未覆盖。
- **建议**：运行 `cargo fmt`。

### F15. 通知标题硬编码产品名（低危 · 与 F2 同源）
- **位置**：`src/components/ActionButtons.tsx:52` `sendNotification({ title: "Image Crunch", ... })`。
- **风险**：见 F2；改名时需同步。
- **建议**：抽为 `APP_NAME` 常量或读 `app.name()`。

---

## 四、各类别专项结论

| 关注点 | 结论 |
|--------|------|
| **硬编码路径** | 无绝对路径硬编码；所有路径来自用户输入/对话框。✅ |
| **硬编码端口** | `tauri.conf.json:8` `devUrl: localhost:1420`、`vite.config.ts:18` `port:1420`/`1421` 为 Tauri 标准约定，非缺陷。✅ |
| **硬编码密钥/token** | 无。GitHub 调用为未认证公开 API。✅ |
| **硬编码 URL** | GitHub API/Release URL 为外部更新源，合理但绑定原作者仓库（F2）。⚠️ |
| **硬编码配置值** | 质量默认 80、线程数 `div_ceil(2).clamp(2,8)`（image_commands.rs:149）等已用常量/计算，良好。✅ |
| **魔法数字** | 见 F11，可维护性层面。⚠️ |
| **输入校验** | F5/F6 为实质缺陷（opacity、width/height=0）；其余前端有基本校验。⚠️ |
| **资源释放** | 后端文件句柄正常 drop；前端 timer/订阅/observer 均有 cleanup（good）；F10 为轻微泄漏。✅（除 F10） |
| **错误处理** | 后端用 `Result` 全程 `?`，生产路径无 `unwrap/expect/panic/unsafe`（仅 `lib.rs:28` 启动入口有 `.expect`，可接受）；前端 `try/catch` 覆盖主要 invoke 调用。✅ |
| **命令/SQL 拼接** | 无 SQL、无 shell 执行、路径拼接均用 `Path/PathBuf::join`。✅ |
| **敏感信息明文** | 仅 localStorage 存语言/主题偏好，非敏感。✅ |
| **环境假设** | `basename()` 已同时处理 `/` 与 `\\`（DropZone/FileList/WatermarkSection）；测试用 `std::env::temp_dir()` 可移植；模板图依赖 `document.createElement`（Tauri 桌面恒有，但 jsdom 测试环境无 canvas 支持，属已知限制）。✅ |

---

## 五、修复优先级建议

1. **立即（中危）**：F4 加请求超时（一行） → F2 统一标识/仓库（合规+更新可用性） → F5/F6 后端参数 clamp（防 panic/异常） → F3 关闭符号链接跟随 + 输出目录校验。
2. **加固**：F1 配置 CSP；F8 filename 过滤 `..`。
3. **清理（低危，可批量）**：F7 颜色变量化、F10 i18n 监听器 off、F11 常量收敛、F12 日志脱敏、F13 删除冗余命令、F14 `cargo fmt`、F15 通知标题常量。

> 注：F2（原作者标识）此前已在项目待办中（用户尚未提供反向域名），本次审查再次确认其仍存在于 5 处。

---

## 六、修复状态（2026-09-26 落地）

| 编号 | 状态 | 落地方式 |
|------|------|----------|
| F1 | ✅ 已修 | `tauri.conf.json` 配置 `csp`（生产）+ `devCsp`（开发）：生产 `script-src 'self'`、`connect-src ipc: http://ipc.localhost`；开发额外放行 Vite 1420 / HMR ws / unsafe-eval |
| F2 | ⏸️ 暂缓 | **用户决定后续再弄**。遗留 5 处：`tauri.conf.json`（productName/identifier）、`Cargo.toml`（authors/repository）、`update_commands.rs`（GITHUB_REPO）、窗口标题；待提供反向域名后统一收敛 |
| F3 | ✅ 已修 | `get_image_files` 改 `follow_links(false)`，并注明命令层绕过 fs scope、假定前端输入可信 |
| F4 | ✅ 已修 | `reqwest::Client::builder().timeout(Duration::from_secs(10))` |
| F5 | ✅ 已修 | `opacity.clamp(0,100)`、`(*scale_percent).clamp(1,100)`、`margin_percent.clamp(0,100)` |
| F6 | ✅ 已修 | 像素 resize 分支 `w.max(1)` / `h.max(1)` |
| F7 | ✅ 已修 | App/DropZone/LanguageSwitcher/ThemeToggle 的 `#161b22`、`#f5f6f8`、`#0b0d12` 字面量改为 `var(--surface-card)` / `var(--surface-app)` |
| F8 | ✅ 已修 | `output_filename` 先 `replace("..", "_")` 再过滤路径分隔符 |
| F10 | ✅ 已修 | `useTranslation` 内 `onCleanup(() => i18n.off(...))` |
| F11 | ✅ 部分修 | 字体 8–400 双份定义收敛为 `WatermarkSection` 的 `FONT_SIZE_MIN/MAX` 常量 + 两端同步注释；其余散布常量（预览尺寸/防抖等）保持现状 |
| F12 | ✅ 已修 | `DropZone` 类型检测失败日志改记 `basename(path)`（其余日志本就不含路径） |
| F13 | ✅ 已修 | 删除 `get_image_info` 命令与 `ImageInfo` 结构体，`lib.rs` 同步移除注册 |
| F14 | ✅ 已修 | `preview_image` / `preview_image_data` 缩进重排为 4 空格 |
| F15 | ✅ 已修 | 通知标题改用 `getName()`（读 `tauri.conf.json` productName），F2 改名后自动跟随 |
| F9 | ⏳ 未修 | 嗅探整文件读内存 —— 影响仅为大文件时内存峰值，暂留 |

**验证结果**：`cargo check` ✅ / `cargo test` 29 通过 ✅ / `pnpm typecheck` ✅ / `pnpm test:run` 14 通过 ✅ / `tauri.conf.json` JSON 合法 ✅。
**待人工确认**：CSP 需跑一次 `pnpm tauri dev` 冒烟验证（HMR 与 IPC 正常、控制台无 CSP violation 警告）。
