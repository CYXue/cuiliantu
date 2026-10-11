# 代码优化审查清单（OPTIMIZATION REVIEW）

> 来源：外部 AI 全量代码审查（前端 44 条 + 后端 36 条 = 80 条），本地逐条抽验 10 条全部属实、行号精确。
> 本文档是修正后的本地版：标注已完成项、修正原文的错误建议、过滤"建议保持现状"类的注水条目。
> 行号基准：提交 `0e282b9`（除已注明完成的项，其余行号仍有效）。

## 一、已完成（2026-10-09，提交 `0e282b9`，agent-check 七阶段 PASS）

| 原编号 | 项 | 实测修正 |
|---|---|---|
| 后端 H1 | image crate 裁剪 features | **比原文复杂**：只改 `image` 自己无效——`imageproc 0.25` 的 default 硬拉 `image/default`，`webp 0.3` 的 default（=img）也带 image。三处一起裁（imageproc 加 `default-features=false, ["rayon"]`，webp 关 default）后 `cargo tree` 实证 image 只剩 `bmp,gif,jpeg,png,tiff,webp` |
| 后端 H5 | render_preview 整图 clone | before 缩略图先编码，再 move 全图进 pipeline，消除每次预览 ~48MiB 拷贝 |
| 前端 H5 | cropperjs 动态导入 | `import type` 保留类型 + 运行时动态 import js/css；`initCropper` await 后校验 ref 未变（防卸载后挂死节点）。产物确认独立异步 chunk `cropper.esm-*.js` |
| 前端 H7/H8 | 删 ZapIcon、setFileType | 全仓 grep 零残留；测试改走 setFileTypes |
| 前端 H9 | templateImage 硬编码中文 | 改走 `i18n.t()`（命令式调用）；**缓存按语言失效**（文字烤进像素，切语言必须重绘）；新键 `preview.templateSample` / `preview.templateHintCanvas` |
| 前端 H6 | main.tsx 全局 MutationObserver | **原建议"直接移除"有坑，实际做法**：先确认 observer 只监听 childList（只兜"无内联样式滑块的挂载首帧"，不管程序化改值）+ 全部 5 个滑块已带响应式 `sliderFillStyle` 内联样式，双重确认后拆除。input 委托 + App.css `--value` 渐变兜底保留 |

## 一b、已完成 · 第二批（2026-10-09，提交 `baecb88`，agent-check 七阶段 PASS）

| 原编号 | 项 | 备注 |
|---|---|---|
| 后端 H2 | edit.rs 色调函数原地化 | 6 个色调函数改 `&mut RgbaImage`，apply_color_edit 一次 `to_rgba8` 串联全部操作（N 连击从 N 次全图拷贝 → 1 次转换）；apply_sepia 并入 map_rgb（原 L6 一并解决）；grayscale 改从同一 buffer 计算 |
| 后端 L6 | apply_sepia 复用 map_rgb | 随 H2 一并完成 |
| 后端 H3 | 批处理进度事件节流 | `processing-progress` 按 ~1% 粒度（+首尾文件）发射；`processing-result` 逐文件保留。下游确认：进度条可承受阶梯，文件行最终状态由 result 事件兜底 |
| 后端 H6 | header 双解析合并 | 炸弹检查移入 `decode_reader_oriented`（用已打开 decoder 的 dimensions），每文件 header 只解析一次；GIF 动画路径保留独立 probe |
| 后端 H7 | 水印显式 RgbaImage | `into_rgba8()` 消费输入（RGBA8 变体零拷贝），静态分发 + 保证 alpha 混合一致 |
| 后端 M10 | GIF 帧双 clone → into_rgba8 | 随 H2 批顺手完成 |
| 后端 M6(部分) | apply_quantize 改 into_rgba8 | **其余保留**：encode_to_memory/encode_jpeg 接 `&DynamicImage`（借用 API，preview 还要复用处理后图像），无法消费 self；需拆两层 API 才能做，暂缓 |
| 前端 H1 | FileList 计数 createMemo | 单次遍历 + memo，替代 4 处全量 filter |
| 前端 H4 | optionsKey memo 化 | 序列化快照缓存；细粒度依赖枚举方案被否（新增字段漏注册 = 预览不刷新的静默失效，比 stringify 开销更危险） |
| 前端 H10 | base64 O(n²) → 一次 join | 分块 push + 单次 join |
| 前端 H11 | 预览图 decoding="async" | 两张 img 加异步解码提示 |
| 前端 H12 | 监听器预拆 | handleStart 开头 `unsubscribeRef?.()`，start→cancel→start 不再短暂叠加监听 |
| 中 M15 | PreviewPanel 竞态测试 ×4 | 慢旧响应不覆盖新结果 / 旧失败不误报 failed / 当前失败正常呈现 / 路径命令载荷正确 |

## 一c、已完成 · 第三批（2026-10-09，backlog 全量清空，agent-check PASS）

| 原编号 | 项 | 备注 |
|---|---|---|
| 后端 H4 | CliEmitter 双重序列化 | trait 改为 `BatchEvent` typed enum（Progress/Result/Complete）；CLI 端 `match` 零序列化，GUI 端每事件恰好一次 serde |
| 后端 M13/M14 | 两大文件拆分 | image_commands.rs(1470) → batch/paths/detect/preview/report 五模块 + 薄 re-export 壳；processor.rs(2606) → watermark/encode/metadata/naming 四子模块（类型与测试留原处，跨文件 impl 块） |
| 后端 M1 | hits_forbidden_dir 分配 | OnceLock 预计算小写匹配模式 |
| 后端 M2 | 字体重复解析 | OnceLock<FontRef> 全进程缓存 |
| 后端 M3 | 目标体积二分重编码 | `to_rgb8()` 提出循环，~7 次全图转换 → 1 次（新增 encode_jpeg_rgb） |
| 后端 M5 | output_filename 链式 replace | 单遍展开；顺带修复 stem 含字面占位符时的二次展开语义 |
| 后端 M8 | detect 双 open | 复用 64KB header buffer 作 ImageReader 源，每文件一次 open |
| 前端 M3 | SliderRow/RangeRow 合并 | settings/shared.tsx 的统一 RangeRow（两个漂移 API 收敛为一个） |
| 前端 M4 | 4 处分段按钮样式 | utils/classNames.ts `segmentedBtnClass`（EditSection chip 风格不同，保留自身 helper） |
| 前端 M5 | DropZone picker 重复 | `handlePick(dialogOptions)` 工厂合并文件/文件夹两个 handler |
| 前端 M6 | 三处 legacy 迁移 | utils/storage.ts `migrateLegacyStorage(newKey, legacyKeys, validate?)`，templates 的 JSON 校验走 validate 钩子 |
| 前端 M7 | 两组 radio 重复 | settings/shared.tsx `<RadioGroup>`（Metadata/Compression） |
| 前端 M10 | store 全量 map | updateFileStatus/setFileTypes/resetFileStatuses 改 `produce` 就地修改（100 测试无回归） |
| 前端 M11 | class 模板串 | FileList 行容器/图标容器改 `classList` 指令 |
| 前端 M12/M13 | select 断言 | utils/parse.ts `parseSelectValue`（OutputFormat/ConflictPolicy/WatermarkPosition 四处运行时校验） |
| 前端 M14 | 魔法数字 | constants.ts：PREVIEW_DEBOUNCE_MS / TEXT_SYNC_DEBOUNCE_MS / STAGGER_DELAY_MS / BASE64_CHUNK_SIZE / MAX_DIMENSION（镜像后端 MAX_RESIZE_DIMENSION） |
| 前端 M1 | SettingsPanel 590 行 | 模板管理（264 行）抽出 settings/TemplateSection.tsx，面板 590→308 行 |
| 前端 M2 | PreviewPanel 498 行 | 裁剪状态机抽 components/useCropper.ts（含 loadCropper 懒加载与竞态防护），面板 ~350 行 |
| 前端 M17 | handleStart 118 行 | 三通道监听提取模块级 `setupRunListeners`，run 生命周期自上而下可读 |
| 前端 M18 | DropZone 420 行 | 拖放/粘贴管线抽 components/dropInteractions.ts（useTauriDragDrop / usePasteHandler），420→286 行 |
| 前端 M16 | 死 i18n 键 | 删 16 个（报告 14 + 扫描新抓 settings.resizePercent/settings.watermark）；新增 i18n-usage.test.ts 引用扫描测试（含 DYNAMIC_PREFIXES 白名单）防回潮 |

## 二、对原报告的修正结论

1. **H6 类"看着冗余实为补丁"的代码不能照单直删**。拆之前必须找到它存在的原因（本例：滑块填充兜底，项目规约要求所有滑块带内联样式后它才降级为纯开销）。
2. **80 条 ≠ 80 个值得动手的事**：低优先级 29 条里多条原文即"保持现状/影响极小/可不动"，已在第三节过滤。
3. 依赖裁剪类建议（features、双依赖）必须用 `cargo tree -f "{p} {f}" -i <crate>` 实证最终 feature 集，不能只看自己的 Cargo.toml。

## 三、待办 backlog

> **状态：高/中优先级已全部落地（三批），本节仅存两项有据缓办项。**

### 缓办（有明确理由）

- [ ] **前端 H2**：FileList 虚拟化。**降级说明**：行内容含条件分支/错误换行 → 行高不固定，虚拟化需逐行测量或引入 `@tanstack/solid-virtual` 依赖；典型批量几十至几百文件时 Solid 行模板的 DOM 量尚可。等真实大文件夹反馈再动
- [ ] **后端 M6（剩余）**：encode_to_memory/encode_jpeg 借用 API 下的单次 `to_rgb8` clone 无法用 into_* 消除（preview 需复用处理后的图像），需拆"消费/借用"两层 API——一次性 clone vs 全链路签名改动，除非 profiling 显示热点否则不动。（M3 已完成：二分循环内的重复转换已提出循环外，新增 `encode_jpeg_rgb`）

### 低优先级（一览，多数可不动）

| 项 | 说明 | 建议 |
|---|---|---|
| 前端 L1-L4 | 文件列表 ARIA list 语义、行按钮语义过载、DropZone aria-label、对比滑块 aria-label | 顺手可做 |
| 前端 L5 | theme.ts matchMedia 监听器 HMR 叠加 | 加防重入标志 |
| 前端 L7 | getTemplateImageDataUrl 收回 export | 顺手可做 |
| 前端 L9 | vite manualChunks | cropperjs 已异步化，收益减半 |
| 前端 L10 | useTranslation 每组件注册 languageChanged（~15 个） | 改全局 lang signal |
| 后端 L3 | update_commands 每次新建 reqwest Client | 启动一次，可不动 |
| 后端 L9 | webp crate 与 image 内置 webp 并存 | 需验证编码质量等价后再删 |
| 后端 L12 | output exists() 与 write 间 TOCTOU | 桌面单用户低危 |
| 后端 L14 | ProcessError 全 String 丢结构化上下文 | 可加 DecodeError/EncodeError 变体 |
| 其余 | L6 并入 H2；L8 CSS 拆分、L13 测试外移、L15 遍历剪枝、L2/L10/L11（原文即"可不动"） | 暂不处理 |

## 四、无需改动

- **安全维度**：path_guard 白名单 + 凭证目录拒绝 + 解压炸弹防护 + IPC 边界 clamp + CSV 公式注入防护均已到位（外部审查同结论）。
- **utils/path.ts basename 尾斜杠语义**：有测试锁定的有意行为，保持。

## 维护说明

完成一项就把对应条目移入第一节并附提交号；新增发现追加到对应优先级。行号漂移时以符号名定位为准。
