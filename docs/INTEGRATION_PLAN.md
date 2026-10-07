# 图像工具集大成版 · 整合功能规划（INTEGRATION PLAN）

> 基座：image-crunch（Tauri 2 + SolidJS + Rust `image`/`webp`/rayon）— 已改名为 **TuCui（图淬）**，下文沿用旧名指基座本身
> 目标：吸收各调研项目的优点与特色功能，规避已知缺点，形成功能协调、无冗余的集大成版本。

## 一、调研项目功能映射

| 来源项目 | 采纳 ✅ | 规避 ❌（及理由） |
|---|---|---|
| **image-crunch**（基座） | Tauri 2 架构、rayon 批量+进度事件、i18n 框架、magic-bytes 类型识别 | ❌ 取消不生效（本轮实现真取消）；❌ 无预览（本轮补齐）；❌ 输出静默覆盖（本轮加冲突策略）；❌ 动图丢帧（本轮 GIF→GIF 保留） |
| **Converseen** | 百分比批量缩放、旋转/翻转 | ❌ ImageMagick 重依赖（改用 Rust `image` 原生管线，安装包小、无外部进程） |
| **Caesium** | 质量滑杆、「仅当更小才保存」、输出目录选择 | ❌ 付费版功能墙（全部能力免费内置） |
| **Imagine** | 压缩前后并排对比、PNG 色彩量化 | ❌ 仅 Windows（本版跨 Win/Mac/Linux） |
| **Squoosh** | 滑块式前后对比、**真实编码体积**估算（非公式近似）、按格式高级选项思路 | ❌ 无批量、仅浏览器（本版批量+桌面） |
| **XnConvert** | 批处理动作管线思想（缩放→旋转→翻转→水印→量化）、文件名模板、图片水印、输出冲突策略 | ❌ 闭源；❌ 500+ 格式与复杂动作编排（本版 8 核心格式 + 固定管线，UI 保持简洁） |
| **SnapTune** | 文件类型识别产品思路（已用 magic bytes 深化，含 HEIC/AVIF/JXL） | ❌ Next.js 云端架构（本版纯本地） |
| **PicGenie** | —（功能与 Imagine/Caesium 重叠，无独有可采纳项） | — |

## 二、整合后功能全景

| 模块 | 功能 | 状态 |
|---|---|---|
| 格式转换 | jpg/png/gif/bmp/tiff/webp 互转；WebP 有损/无损 | ✅ 基座已有 |
| 图片类型识别 | magic bytes 识别真实格式（含 HEIC/AVIF/JXL）+ 扩展名不符提示 + 中文界面 | ✅ 上轮完成 |
| 对比预览 | 选中文件 → 滑块对比原图/效果图；**真实体积估算**（按当前选项全分辨率编码） | ✅ 本轮新增 |
| 尺寸调整 | 像素框 / 百分比双模式；适配模式：拉伸(stretch)、等比包含(contain)、等比覆盖(cover) | ✅ 本轮新增 |
| 变换 | 旋转 90/180/270、水平/垂直翻转 | ✅ 本轮新增 |
| 水印 | **图片水印**：位置（4角+居中）、不透明度、相对缩放、边距 | ✅ 本轮新增 |
| 文字水印 | 内嵌**得意黑 Smiley Sans**（SIL OFL 1.1，TTF 2.6MB）：单行文字、字号、颜色、不透明度、位置、边距；中日英字符全覆盖 | ✅ 本轮新增 |
| 高级压缩 | PNG/GIF 色彩量化（2–256 色，NeuQuant）；GIF→GIF 动图逐帧保留 | ✅ 本轮新增 |
| 输出控制 | 文件名模板 `{name}/{width}/{height}/{format}`；冲突策略：覆盖/自动重命名/跳过；「仅当更小才保存」 | ✅ 本轮新增 |
| 批处理体验 | 真取消（逐文件检查取消标志）、进度、逐文件结果、桌面通知 | ✅ 真取消本轮新增 |
| **上传预处理** | 独立模式：网站要求一键达标——证件照预设（196×250·≤50KB）、资料预设（≤300KB·尺寸不变）、自定义（宽/高/KB/适配模式）；魔法字节诊断真实格式；二分质量搜索保最高可行清晰度；统一 baseline RGB JPEG（.jpg），修复假扩展名/CMYK/渐进式/EXIF 异常；冲突自动重命名 `原文件名_upload.jpg`；>10 张软提示 | ✅ 本轮新增 |
| WebP/动图编码输出 | `image`/`webp` 生态暂不支持动画 WebP 编码 | 🔜 待生态 |
| HEIC/AVIF/JXL 解码 | 需 libheif/libjxl 原生依赖，当前仅识别不解码 | 🔜 待评估 |
| EXIF 保留 | `image` crate 不读写 EXIF，需引入 kamadak-exif | 🔜 规划 |

## 三、架构决策（与现架构协调、避免冗余）

1. **固定处理管线**：解码 → 缩放 → 旋转 → 翻转 → 水印 → 量化 → 编码。
   相比 XnConvert 的自由动作编排：覆盖 95% 实际场景，UI 零学习成本，无执行顺序歧义。
2. **水印统一为 RGBA stamp + 同一 overlay 路径**：图片水印按基准宽度缩放，文字水印用内嵌得意黑（SIL OFL 1.1 允许内嵌分发，TTF 2.6MB 一次性 include_bytes）渲染后经透明边裁剪对齐；两者共享位置/不透明度/边距合成逻辑，零重复代码。
3. **编码统一走内存**：`encode_to_memory` 同时服务「写盘」与「预览体积估算」，估算即真实值，一条代码路径，无近似公式。
4. **命名/冲突逻辑收进 Processor**：`process_image(input, output_dir, options)` 内部完成模板展开与冲突解决（含磁盘探测），批量命令只做调度，职责单一。
5. **取消 = 全局 AtomicBool**：单窗口单批处理场景足够；batch 循环逐文件检查，取消后已处理文件保留统计。
6. **兼容性**：新增字段全部 `#[serde(default)]`，旧前端/旧测试不破坏；`ProcessingResult` 新增 `skipped` 字段。
7. **水印加载一次/批**：批处理预载水印 `Arc<DynamicImage>`，避免千张图重复读盘解码。

## 四、已知限制（诚实声明）

- 动图：GIF→GIF 逐帧保留；GIF→其他格式取首帧；动画 WebP 输出暂不支持。
- PNG 量化输出为 8-bit RGB（非调色板索引 PNG），体积显著下降但非理论最优（`image` crate 暂不暴露索引 PNG 编码）。
- 输入识别支持 HEIC/AVIF/JXL 的「类型识别」，但这些格式暂不可作为转换输入（依赖未引入）。
- 「等比包含(contain)」按适配框等比缩放，不做信箱式画布填充。
- 文字水印支持多行（回车换行，CRLF 自动归一，空行保留行距）；行高取字体自然度量（em 框 + line_gap），字符缺失时优雅降级为无水印。

## 五、本轮交付物

- 后端：`processor.rs`（管线重写）、`image_commands.rs`（preview_image / cancel_processing / 批量调度重构）、Cargo.toml（+color_quant）
- 前端：`types` / `store` 扩展、`SettingsPanel` 分区化 + 4 个新分区组件、`PreviewPanel`、`FileList` 选中态、三语 i18n
- 测试：后端管线/命名/冲突/量化等单元测试，前端 store 测试扩充

## 六、本轮（2026-09-27）P1 修复交付与封锁项结论

> 身份已统一为 **CuiLianTu / cuiliantu**（Cargo 包名 `cuiliantu` / `cuiliantu_lib`，二进制 `cuiliantu-cli`）。
> 硬约束：**零原生依赖、Win/Mac/Linux 一致构建**（rustls 替代 native-tls、Tauri bundle targets `"all"`）。以下决策均以此为准。

### 已交付（按推荐完成修复）

| 项 | 实现 | 验证 |
|---|---|---|
| **CLI 批量处理** | 新增 `src/bin/cli.rs`（clap derive），复用 GUI 同一套 `process_batch_with` 管线；支持格式/质量/无损/缩放(像素·百分比)/旋转/翻转/量化/水印(文·图)/冲突策略/仅更小才存/目标大小二分/并行度 `--jobs` | `cargo test` + 真实二进制冒烟：3 图 → 输出 3 个 WebP，统计与 CSV 正确 |
| **子目录镜像** | `common_base_dir` 求公共父目录，`process_batch_with` 按相对路径重建输出树；逐文件 `create_dir_all` 兜底（**修复了镜像子目录未建导致写盘 os error 3 的缺陷**） | 新增回归测试 `mirror_subdirs_recreates_input_tree` + 冒烟实测 `out/2024/`、`out/2025/` 镜像成功 |
| **CSV 报告** | `write_report_csv`（RFC 4180 转义）同时服务 GUI `export_report` 命令与 CLI `--report` | 测试 `write_report_csv_produces_escaped_rows`（含逗号字段引号）通过 |
| **EXIF/ICC 保留** | `kamadak-exif`（纯 Rust）重嵌入：JPEG 输出时读源 EXIF + ICC，强制 `Orientation=1`（管线已把真实旋转烘进像素，防二次旋转），best-effort 失败回退原始字节 | 测试 `keep_metadata_reattaches_exif_with_normalized_orientation`（orientation 6→1 且可重解码）通过 |

### 封锁项（P1 未实现，按推荐不硬做）

1. **HEIC / AVIF / JXL 解码**
   - 结论：**与"识别已做、解码未做"最不匹配的短板，但无法实现**。三者解码全部依赖原生编解码库（libheif / libaom / libjxl），与"零原生依赖、三端一致构建"硬成果冲突。
   - 验证：`avif-decode` 会拉入 `libaom-sys`（原生 libaom C 库），已 `cargo remove` 撤回。
   - 若未来要做：feature-gated 启用（`[features] heif = ["dep:ravif-heif"]` 之类），且只对 Linux/zig-cross 或经 `vcpkg`/`pkg-config` 显式配置的平台开放，不在默认 bundle 内。GUI 端 `detect_file_type` 已能识别这三类容器（HEIC/AVIF/JXL），输入侧继续"仅识别、不解码"，保持诚实。
2. **动画 WebP 输出**
   - 结论：**纯 Rust 生态无动画 WebP 编码器**（`webp` crate 仅静态图）。属真·生态封锁，不实现。
   - 现状：GIF→GIF 逐帧保留；其他格式取首帧；动画 WebP 输出明确不支持（在 UI/CLI 文档中标注）。

### 编译 / 测试状态（2026-09-27）

- `cargo check --bin cuiliantu-cli`：通过，0 警告。
- `cargo test`：40 个用例全过（新增 `keep_metadata_reattaches_exif_with_normalized_orientation`、`common_base_dir_finds_shared_parent`、`write_report_csv_produces_escaped_rows`、`batch_finishes_and_logs_every_failed_emit`、`mirror_subdirs_recreates_input_tree`）。
- CLI 真实冒烟：`input/` 三张嵌套 PNG → `--format webp --mirror --report` → 输出树镜像、CSV 列齐全、WebP 魔数 `RIFF` 正确。

