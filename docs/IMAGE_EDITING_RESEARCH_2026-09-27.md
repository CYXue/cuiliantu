# 图片修改能力调研报告（2026-09-27）

> 主题：围绕**图片修改（编辑）**，先盘点本地参考项目覆盖度，再补充调研代码托管平台（GitHub）与技术社区（掘金 / CSDN / 教程站），覆盖：主流算法与实现原理、常用开源库特性、实际表现与适用场景、优缺点与对比分析。
> 硬约束（沿袭项目既定成果）：**零原生依赖、Win/Mac/Linux 一致构建**；技术栈 Tauri 2 + Rust（`image` 0.25.9 / `imageproc` 0.25.1 / `webp`）+ SolidJS；固定处理管线；GUI 与 CLI 共享同一管线。
> 所有"实现位置"均经**本地锁版源码核实**（cargo registry 内 image-0.25.9 / imageproc-0.25.1），非凭文档记忆。

---

## 一、结论速览（TL;DR）

1. **本地参考项目调研对"压缩/格式/批量"已充分，但对"图片修改"覆盖很薄**——六个参考项目的编辑向内容只有缩放/旋转/翻转/水印，色调、滤镜、锐化、裁剪交互、自动增强全部空白。因此本次以外网调研为主。
2. **最大发现：一大批编辑功能零新依赖即可落地**。`image` 0.25.9 内置裁剪/任意色调三件套/锐化/模糊/灰度/反色/抖动，`imageproc` 0.25.1（已因水印在依赖树中）提供中值滤波/直方图均衡/任意角旋转/投影变换/边缘/形态学。P0 清单合计约 **2~3 天工作量，不改 Cargo.toml**。
3. **修正一项旧结论：AVIF 输出已可用**。Cargo.lock 已含 `ravif 0.12.0 + rav1e 0.8.1`（随 `image` 默认 feature 编译进来，纯 Rust AV1 编码器）。此前"AVIF 全封锁"应修正为：**解码封锁（需 libaom-sys 原生库）、编码已具备**——落一个输出冒烟即可确认。
4. **photon-rs（96+ 编辑函数、30+ 预设滤镜）功能面最广，但不建议直接引入**：它锁定 `image ^0.24.8 + imageproc ^0.23`，会拖出第二份 image 依赖树（体积翻倍 + 类型不兼容）；且发布节奏低（0.3.2→0.3.3 隔两年）。**价值 = 免费的算法参考实现**（各滤镜参数配方直接读它的源码抄）。
5. **AI 类（背景去除 / 超分）封锁结论维持**：Rust 生态全部经 ONNX Runtime（`ort` 会拉预编译原生库），唯一纯 Rust 逃生门 RTen 是实验性。模型 4.7~171MB 也与产品定位冲突。
6. **集成路线推荐**：Rust 后端权威管线（编辑参数并入 `ProcessingOptions`，预览自动继承）+ 前端只做交互（裁剪框选用 Cropper.js 类方案；实时滑杆反馈可用 WebGL 做可选加速层）。

---

## 二、本地参考项目对"图片修改"的覆盖度评估

复检 `docs/INTEGRATION_PLAN.md`（六节）与 2026-09-25/26/27 工作日志，逐参考项目核对编辑向功能：

| 参考项目 | 图片修改类覆盖 | 评估 |
|---|---|---|
| image-crunch（基座） | 无（只有转换/压缩） | 编辑空白 |
| Converseen | 缩放、旋转/翻转、百分比重缩放 | 已吸收 |
| Caesium | 无编辑（纯压缩） | — |
| Imagine | 无编辑 | — |
| Squoosh | 无编辑（纯编码对比） | — |
| XnConvert | 裁剪（固定模式）、旋转、文本水印 | 水印已吸收；交互式裁剪未吸收 |
| SnapTune / PicGenie | AI 修图方向，已淘汰 | — |

**结论**：本地素材在"编辑"维度不足，调研重心转向外网 ✅（符合预期触发条件）。

当前项目**已有**编辑能力：缩放（像素/百分比 × stretch/contain/cover）、旋转 90° 步进、水平/垂直翻转、量化（PNG/GIF 调色板）、文/图水印、目标大小二分。
**缺口**：自由/比例裁剪、任意角度旋转、色调调整（亮度/对比度/色相/饱和度/伽马）、滤镜（灰度/反色/复古/双色调）、锐化与模糊、自动增强（自动反差/白平衡）、圆角、边框、像素化/局部打码、AI 类。

---

## 三、主流算法与实现原理（按类别）

### 3.1 几何变换

| 算法 | 原理要点 | 现成实现（已锁版本） |
|---|---|---|
| 裁剪 | 区域拷贝，零重新采样 | `DynamicImage::crop / crop_imm`（dynimage.rs:502/508） |
| 任意角旋转 | **反向映射**：对输出每像素按逆变换找源坐标 + 双线性插值；画布扩容 `W' = W·|cosθ|+H·|sinθ|` | `imageproc::geometric_transformations::{rotate_about_center, rotate, warp}`（:278/:302/:376），`Projection::rotate(theta)` |
| 透视/投影 | 3×3 单应矩阵；4 对控制点 SVD 求解 | `Projection::from_control_points` + `warp` |
| 内容感知缩放 | Seam carving：动态规划找最小能量接缝逐条删除 | `imageproc::seam_carving::shrink_width`（官方自认 "pretty rough"，P2 观望） |

### 3.2 色调 / 色彩调整

| 算法 | 原理要点 | 现成实现 |
|---|---|---|
| 亮度 | 每通道加常数后 clamp | `brighten(i32)`（dynimage.rs:1072 / colorops:156） |
| 对比度 | `(v-128)*factor+128` 围绕中点缩放 | `adjust_contrast(f32)`（:1061） |
| 色相旋转 | RGB 色相旋转矩阵（Y'IQ 导出） | `huerotate(i32)`（:1084，-180..180） |
| 饱和度 | 与灰度插值：`v' = gray + (v-gray)*s`；或 HSL/Lab | **需自写 ~30 行**（矩阵法，无依赖）；photon `correction` 可抄参数 |
| 伽马 | 256 项 LUT 查表 `255*(v/255)^γ` | 自写 ~20 行（LUT 一次构建 O(1) 每像素） |
| 直方图均衡 | 累积分布 CDF 重映射 | `imageproc::contrast::equalize_histogram`（:264） |
| 直方图匹配 | 按目标图 CDF 映射（风格迁移用） | `match_histogram`（:384） |
| 自动反差 | 百分位（1%/99%）黑白糖点拉伸 | 自写 ~40 行（直方图 + 线性拉伸） |
| 灰度世界白平衡 | 三通道均值拉平 | 自写 ~30 行 |

⚠️ **色彩空间注意**（CSDN image-rs 实战文 + imageproc 文档共同确认）：`imageops` 直接在 **sRGB** 上运算（快、惯用，亮度/对比度有轻微 gamma 误差，日常可接受）；`imageproc` 假设 **linear RGB**（文档明示，不转换会有色伪影）。对批处理工具建议：色调类用 `imageops`（所见即所得的轻量路线），不做"专业色彩管理"承诺。

### 3.3 卷积滤波

| 算法 | 原理要点 | 现成实现 |
|---|---|---|
| 高斯模糊 | 二维高斯 = 两次一维卷积（可分离），O(w·h) 与核径无关 | `blur(sigma)`（:980）、`blur_advanced`（:1000）、`fast_blur`（:1016）、`imageproc::filter::gaussian_blur_f32`（:309） |
| USM 锐化 | `out = orig + amount·(orig − blur(orig))`，阈值控制只锐化强边缘 | `unsharpen(sigma, threshold)`（:1034） |
| 任意卷积核 | 3×3 滑窗 | `filter3x3(&[f32])`（:1048）——锐化/浮雕/边缘核自由定义 |
| 中值滤波 | 邻域取中位数，去椒盐噪点且保边 | `imageproc::filter::median_filter(x_radius, y_radius)`（median.rs:109） |
| 边缘检测 | Sobel/Scharr/Canny | `imageproc::{gradients, edges::canny}` |
| 像素化/马赛克 | 块内下采样→最近邻上采样 | 自写 ~30 行（块均值） |
| 形态学 | 膨胀/腐蚀/开闭 | `imageproc::morphology` |

### 3.4 风格化

| 算法 | 原理要点 | 现成实现 |
|---|---|---|
| 灰度 | Rec.601 感知权重 0.299/0.587/0.114 | `grayscale()`（:831） |
| 反色 | `255-v` | `invert()`（:863） |
| 复古 Sepia | 固定 3×3 色彩矩阵 | `filter3x3` 或自写矩阵 ~15 行 |
| 双色调/阈值 | 亮度映射到双色 / 二值化 | photon 有配方；`imageproc::contrast::threshold` |
| 抖动 | Floyd–Steinberg 误差扩散（GIF/PNG8 降色保层次） | `imageops::colorops::dither`（colorops.rs:491） |
| 暗角 / 颗粒 | 径向衰减乘子 / 噪声叠加 | 自写各 ~30 行 |

### 3.5 合成与画布

| 算法 | 原理要点 | 现成实现 |
|---|---|---|
| 圆角 | alpha 蒙版 + 距离场抗锯齿（角部 1px 渐变） | 自写 ~50 行；**输出需 PNG/WebP 保留 alpha**（JPEG 无 alpha 需垫底色，UI 要提示） |
| 边框/画布扩展 | 新画布 + 居中粘贴 + 填充色 | `imageops` 拼 + `RgbaImage::from_pixel` |
| 水印/标注叠加 | RGBA stamp 合成 | ✅ 已实现（得意黑 + imageproc drawing） |

### 3.6 AI 类（封锁，原理存档）

- **背景去除**：U2-Net/ISNet/BiRefNet → ONNX 推理出 0-1 显著性 mask → 上采样回原尺寸 → 写入 alpha。真实案例（Convertify 系列）给出完整管线与两个坑：模型体积虚标（isnet 标称 44MB 实际 171MB）、`ort` 错误类型非 `Send+Sync` 需在边界 stringify。
- **生态现状**：rembg-rs / outline-core / rmbg 全部依赖 `ort`（构建期拉预编译 ONNX Runtime 原生库）；**唯一纯 Rust 路线 = RTen 后端（outline-core 提供）**，但官方标注实验性、算子覆盖窄、推理慢。加上模型文件 4.7MB（u2netp）~171MB，与"轻量本地工具"定位冲突。
- **超分**（Real-ESRGAN/waifu2x）：模型更大、CPU 推理秒级/张，同属封锁。
- **结论**：❌ 不实现；远期可选 feature-gated（`rten` + u2netp）做"低精度快速抠图"实验分支。

---

## 四、开源库对比

### 4.1 Rust 纯净系（与零原生约束兼容）

| 库 | 版本（核实） | 编辑能力 | 性能 | 优点 | 缺点 / 风险 | 建议 |
|---|---|---|---|---|---|---|
| `image` | 0.25.9（已依赖） | crop、brighten、adjust_contrast、huerotate、blur/fast_blur、unsharpen、filter3x3、grayscale、invert、flip、rotate90 步进、dither | 中（imageops 未做 SIMD 专门优化） | 零成本现成；API 稳定；模板/预览/CLI 全兼容 | 色调粒度粗（无饱和度/伽马）；sRGB 直算 | ✅ 主力，P0 直接用 |
| `imageproc` | 0.25.1（已依赖） | 中值、高斯(f32)、直方图均衡/匹配、自适应阈值、任意角旋转/投影、边缘、形态学、seam carving、绘图 | 中；rayon 并行版可选 | 已在树（水印引入）；CV 算法全 | linear-RGB 假设；seam carving 粗糙 | ✅ 补 image 缺口 |
| `fast_image_resize` | 4.2.1 | 仅缩放 | **SIMD：Lanczos3 RGB8 4928×3279→852×567：image 211ms → avx2 13.2ms（≈16×）；libvips 39.8ms 也被超过** | 纯 Rust；SSE4.1/AVX2/NEON/WASM SIMD128；含 sRGB↔linear mapper | 只管缩放；多一个依赖 | ⚠️ P2 性能项：批量缩放成为瓶颈时引入 |
| `photon-rs` | 0.3.3（2025-05-10） | **96+ 函数**：30+ 预设滤镜、HSL/HSV/LCh 校色、duotone、solarize、oil、blend×10、噪声、seam carving | 宣称接近原生（WASM）；无权威基准 | 功能广度第一；纯 Rust；npm WASM 双发行 | **锁 `image ^0.24.8 + imageproc ^0.23` → 引入即双版本 image 树**；发布节奏低（两年一发）；~600 下载/周 | ❌ 不直接引入；✅ 当算法/参数配方参考 |
| `palette` | 0.7.x | 色彩空间转换（Oklab/Lab/LCh） | — | 感知均匀校色（饱和度/色相在 Oklab 做最自然） | 纯数学库，需自己接 image 缓冲 | ⚠️ 仅当追求专业级饱和度/色相时引入 |
| `resvg` | 2.x | SVG → PNG 光栅化 | 好 | 纯 Rust；SVG 贴纸/矢量水印 | 依赖树较大（usvg/roxmltree） | ⚠️ P2：矢量贴纸水印 |

### 4.2 重量级参考系（**不引入**，理由：原生依赖/体积，违背硬约束）

| 工具 | 定位 | 对本项目价值 |
|---|---|---|
| **libvips** | 流式、低内存、C；`sharp`(Node) 的底座 | 性能对标线：FIR 基准中 Lanczos3 39.8ms；**默认 features 含 JPEG 有效载荷** — 只当"我们能追到什么水平"的参照 |
| **ImageMagick** | 编辑功能最全的 CLI 标杆（`-brightness-contrast -modulate -unsharp -vignette`…） | **操作词汇表**：它的选项命名/默认值是交互设计参考 |
| **OpenCV**（opencv-rust） | CV 全能 + AI 推理 | 太重；C++ 工具链 ❌ |
| **Pillow / Pillow-SIMD** | Python 生态标准 | `ImageEnhance` 的接口设计（Brightness/Contrast/Color/Sharpness 四滑杆）值得 UI 抄 |
| **skia / cairo** | 矢量绘制合成 | 超出位图工具范畴 |

### 4.3 前端 webview 路线（交互层用，不做落盘权威）

| 库 | 技术 | 能力 | 实测数据（社区） | 评估 |
|---|---|---|---|---|
| **glfx.js**（evanw，MIT） | WebGL shader | brightness/contrast、hue/saturation、curves、unsharp mask、lens blur、tilt-shift、vignette、swirl、perspective、noise、sepia、hexagonal pixelate | 掘金/百度云文章：1080p 30fps 实时；2000×2000 <50ms | ✅ 可选"滑杆即时反馈"层；⚠️ 库较老（维护停滞），可只抄它的 shader 当参考自写 |
| **Cropper.js** | Canvas 2D | 交互式裁剪框（比例锁定/旋转/翻转 UI） | 成熟通用 | ✅ 裁剪交互首选（框架无关，SolidJS 直接用） |
| CamanJS / Filterous | CPU JS | 预设滤镜 | 老、慢 | ❌ 不用 |
| **wasm-vips** | WASM | libvips 全家 | WASM 体积大（数十 MB） | ❌ 不用 |
| 自写 WebGL/Canvas2D 近似预览 | — | 低精度实时预览 | 掘金实测 CPU 单遍 1080p 20~50ms，`@input` 滑杆可接受 | ✅ 成本最低的起步方案 |

### 4.4 AI 类

| 方案 | 运行时 | 纯 Rust？ | 评估 |
|---|---|---|---|
| rembg | Python + ORT | ❌ | 参考管线 |
| rembg-rs / rmbg / outline-core | ort（构建期拉 onnxruntime 原生库） | ❌ | ❌ 违背零原生 |
| outline-core `backend-rten` | **RTen（纯 Rust）** | ✅ 实验性 | 远期唯一逃生门；慢、算子窄、需 u2netp 小模型 |
| Real-ESRGAN / waifu2x | ORT/专用运行时 | ❌ | ❌ 封锁 |

---

## 五、集成路线决策矩阵

| 维度 | 路线 A：Rust 后端权威（编辑并入管线） | 路线 B：前端 WASM/GPU 权威（photon-wasm / glfx 出图） |
|---|---|---|
| 预览=落盘一致性 | ✅ 同一实现（preview_image 已全管线复用） | ❌ 双实现漂移风险，或预览即权威但编码/EXIF 弱 |
| GUI/CLI 一致 | ✅ CLI 免费获得（`process_batch_with` 同管线） | ❌ CLI 需另实现 |
| EXIF/ICC 保留 | ✅ 已建好的重嵌入直接复用 | ❌ 前端编码丢元数据 |
| 性能 | 批处理并行 ✅；单图滑杆 20~50ms（1080p，掘金实测够用） | 滑杆实时性最好（GPU），但大图传输出 WASM 有拷贝成本 |
| 体积 | 零新增（P0） | +WASM 数 MB~数十 MB |
| 维护面 | 一个管线 | 前后端两套算法 |
| **结论** | ✅ **主路线** | 仅交互层：Cropper.js（裁剪框）＋可选 WebGL 近似预览 |

**管线插入顺序**（编入 `apply_pipeline`，参数走 `ProcessingOptions`，全部 `#[serde(default)]` 向后兼容模板/CSV/CLI）：

```
decode → EXIF 方向校正 → crop → 任意角旋转 → resize
→ 色调（亮度→对比度→饱和度→色相→伽马）
→ 风格（灰度/反色/复古/自动反差） → 锐化/模糊 → 圆角/边框
→ 水印（最上层叠加，不被调色） → 量化 → encode
```

理由：几何在缩放前保证采样精度；调色在水印前（水印文字不该被调）；量化最后（调色板映射）。

---

## 六、推荐采纳清单（优先级 × 复杂度 × 匹配度）

### P0 —— 零新依赖，纯"接线"（建议最先做）

| # | 功能 | 实现（已核实的落点） | 自写量 | 复杂度 |
|---|---|---|---|---|
| 1 | 裁剪（手动框 + 比例预设：1:1/4:3/16:9/证件照） | `crop_imm`；前端 Cropper.js 交互，后端收像素矩形 | 0 行 | 低 |
| 2 | 任意角度旋转（±180°，画布自动扩） | `imageproc::rotate_about_center`（Bilinear 插值） | 0 行 | 低 |
| 3 | 亮度 / 对比度 | `brighten(i32)` / `adjust_contrast(f32)`（-100..100 映射） | 0 行 | 低 |
| 4 | 色相 | `huerotate(i32)`（-180..180） | 0 行 | 低 |
| 5 | 锐化 | `unsharpen(sigma, threshold)` | 0 行 | 低 |
| 6 | 模糊 | `blur(sigma)` / `fast_blur`（预览用 fast） | 0 行 | 低 |
| 7 | 灰度 / 反色 / 复古 | `grayscale` / `invert` / sepia=`filter3x3` 固定核 | ~10 行 | 低 |
| 8 | 抖动（GIF/PNG8 降色保层次） | `imageops::colorops::dither`（量化已有 color_quant，互补） | 0 行 | 低 |
| 9 | 自动反差增强 | 百分位拉伸（自写） | ~40 行 | 中 |

### P1 —— 小量自写 / 评估后引入

| # | 功能 | 实现 | 量 | 备注 |
|---|---|---|---|---|
| 10 | 饱和度滑杆 | 自写矩阵插值（~30 行）；进阶走 Oklab(`palette`) | 30 行 | UI 四滑杆对齐 Pillow `ImageEnhance` 习惯 |
| 11 | 中值降噪 | `imageproc::filter::median_filter` | 0 行 | 截图/照片去噪点 |
| 12 | 伽马 | LUT（~20 行） | 20 行 | |
| 13 | 白平衡（灰度世界） | 自写 ~30 行 | 30 行 | |
| 14 | 像素化（隐私打码） | 块下采样上采样 ~30 行；区域版需前端选区交互 | 30+ 行 | 截图用户刚需 |
| 15 | 圆角 | 距离场 alpha 蒙版 ~50 行；输出 PNG/WebP 才有 alpha | 50 行 | JPEG 需垫底色提示 |
| 16 | 边框/画布 | 拼接 ~40 行 | 40 行 | |
| 17 | **AVIF 输出解锁** | `ImageFormat::Avif`（ravif+rav1e 已在树）| 落一个冒烟验证 | ⚠️ 编码慢（rav1e CPU），默认不主推，UI 放"实验"标签 |
| 18 | 缩放提速（可选） | `fast_image_resize` 4.2.1（SIMD 16×） | 依赖+适配 | 批量缩放成为性能瓶颈再做 |

### P2 —— 观望/远期

- Seam carving 内容感知缩放（imageproc 自认粗糙；收益窄）
- SVG 贴纸水印（resvg，依赖树大）
- LUT `.cube` 导入（摄影向，超出当前用户面）
- 区域编辑（局部模糊/马赛克选区，需较大前端交互投入）

### ❌ 封锁（维持既有结论）

- HEIC / AVIF / JXL **解码**（原生编解码库）；动画 WebP 输出（纯 Rust 无编码器）
- AI 背景去除 / AI 超分（ONNX Runtime 原生依赖 + 模型体积；逃生门 RTen+u2netp 仅实验性）

---

## 七、社区检索记录（平台 × 结论）

| 平台 | 命中 | 要点 |
|---|---|---|
| GitHub | silvia-odwyer/photon、Cykooz/fast_image_resize、image-rs/imageproc、WarRaft/rembg-rs、wyh2001/outline、evanw/glfx.js | 依赖版本锁定（photon↔image 0.24）、SIMD 基准、RTen 逃生门、WebGL 滤镜清单 |
| crates.io / docs.rs | photon-rs 0.3.3（2025-05-10）、fast_image_resize 4.2.1 | 发布节奏、下载量、feature 矩阵 |
| 掘金 | 《glfx.js 图像特效库》、《Rust 图像处理第 3 节-亮度/对比度调节器》 | WebGL 实时数据；**u8 溢出回绕坑**（先转 f32 再 clamp，否则 272→16 出鬼影条纹）；CPU 1080p 单遍 20~50ms |
| CSDN | 《Rust 图像处理实战：深入解析 image-rs 核心架构》 | imageops blur/unsharpen 用法；卷积计算量 = 核尺寸×图像尺寸；image-rs sRGB/CMYK/Lab 局限 |
| 教程站（LogRocket / Level Up / ataiva / rust-patterns） | image crate 编辑方法全集、卷积数学、CV 生态 2025 综述 | `adjust_contrast(f32)`/`brighten(i32)`/`blur(sigma)` 接口惯例；release 模式性能提醒 |
| 中文社区综述（hqwc.cn） | Rust 图像库选型共识 | **image-rs + imageproc 起步 > photon 快速滤镜 > opencv-rust（重）> cairo/skia（矢量）** —— 与本报告路线 A 一致 |

（Stack Overflow 侧未命中独立高票问答，编辑类问题多被上述教程/文档覆盖，如实记录。）

---

## 八、给后续实现的一句话指引

先做 P0（1~9 项）：全部是"把已核实 API 接进 `apply_pipeline` + `ProcessingOptions`/i18n/模板/CLI 透传"，无新依赖、无管线重构；UI 按 Pillow `ImageEnhance` 的四滑杆（亮度/对比度/饱和度/锐度）+ 滤镜 chips（原图/灰度/反色/复古）布局；预览自动继承现有 `preview_image` 管线，无需新预览通道。

---

## 九、P0 落地记录（2026-09-27，同日完成）

P0 全部 9 项已实现并通过验证（设计偏差如实标注）：

| # | 功能 | 状态 | 实现说明 |
|---|---|---|---|
| 1 | 裁剪 | ✅（两种齐备） | **①比例中心裁剪**（`crop_ratio`，预设 1:1/4:3/3:2/16:9/9:16，XnConvert 同款批量交互）；**②交互拖框裁剪**（`crop_rect: [fx,fy,fw,fh]` 归一化 0..1，预览面板拖框 → Cropper.js `cropend` 回写 options → 批处理按比例映射到每张图；CLI `--crop-rect "0.25,0.25,0.5,0.5"`；实测 400×200 出 200×100、200×400 出 100×200 精确） |
| 2 | 任意角旋转 | ✅ | `edit::rotate_with_expansion`：垫透明画布（max(原,扩容) 尺寸，中心重合）→ `imageproc::rotate_about_center`（Bilinear）→ 中心裁出扩容 bbox；90/180/270 走无损快路径；**实测 200×200@30° → 274×274 = ceil(200·(cos30+sin30)) 精确匹配** |
| 3 | 亮度 | ✅ | `edit::brighten_rgb`（RGB-only 自写，见下方"关键修正"），-100..100 → ±255 |
| 4 | 对比度 | ✅ | `edit::contrast_rgb`（`imageops::contrast` 同款曲线 `((100+c)/100)²` 但 alpha 恒定） |
| 5 | 色相 | ✅ | `edit::hue_rotate_rgb`（CSS/SVG feColorMatrix hueRotate 矩阵，行和=1 → 灰色/0° 精确恒等） |
| 6 | 锐化 | ✅ | `unsharpen(0.5 + amount/100·2.5, 2)`，滑杆 0..100 |
| 7 | 模糊 | ✅ | `blur(sigma)`，滑杆 0..20（后端 clamp 0..50） |
| 8 | 灰度/反色/复古 | ✅ | 灰度 `grayscale()`（转回 RGBA8 保下游合成）、反色/复古为 RGB-only 自写（sepia 经典矩阵） |
| 9 | 自动反差 | ✅ | `edit::auto_contrast_stretch`：luma 直方图 1%/99% 百分位 → 全通道线性拉伸，alpha 保留，已满范围恒等 |

**关键修正（实现中发现）**：`imageops::brighten/contrast/huerotate/invert` 内部 `pixel.map` 遍历**包括 alpha 的所有通道**——半透明 PNG 的透明度会被重新缩放。因此色调四件套全部自写为 **RGB-only**（alpha bit-exact），blur/unsharpen 保留 image 版（alpha 模糊/锐化是正确的边缘羽化语义）。另：`clamp8` 必须 round 而非截断（199.99998→200），否则恒等通道也会偏 1。

**管线接入**：`apply_pipeline` 顺序 = crop → 细旋转 → resize → 90° 旋转 → 翻转 → **色调/风格/锐化/模糊** → 水印 → 量化。编辑参数全部进 `ProcessingOptions`（`#[serde(default)]`），预览/模板/CSV/CLI 自动继承；CLI 新增 `--crop-ratio/--rotate-degrees/--brightness/--contrast/--hue/--sharpen/--blur/--grayscale/--invert/--sepia/--auto-contrast` 11 个旗标。

**附带修复**：`applyTemplate` 改走 `mergeDefaults(tpl.options)`（新增纯函数）——旧 localStorage 模板缺新字段时回落出厂默认，不再产生 `undefined`。

**交互拖框裁剪（同日追加）**：预览面板新增"拖框裁剪"模式（`cropperjs@1.6.3`，纯前端依赖，无原生）。框以归一化分数存入 `crop_rect`，拖动时不重编码（预览冻结），退出模式后管线重跑即刻看到裁剪效果；换选文件自动退出；EditSection 显示激活提示。批处理语义 = 单图拖框、按比例映射全批（与 CLI `--crop-rect` 同一路径）。管线顺序：`crop_rect → crop_ratio → 细旋转 → …`。

**验证（全绿）**：cargo test **48/48**（+crop_fraction 5 断言用例）；tsc 0 错；vitest **34/34**；biome 改动文件 0 错；vite build ✅；CLI 冒烟：比例框在两种尺寸图上输出精确、CSV 全 success。
