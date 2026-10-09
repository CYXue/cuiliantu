// A built-in sample image used for live effect preview before any file is
// selected. It is drawn once on a canvas and cached as a PNG data URL, so the
// preview panel can show what rotate / resize / flip / watermark / quality do
// without the user having to load their own picture first.

let cachedTemplate: string | null = null;

function drawTemplate(): string {
  const w = 480;
  const h = 360;
  const canvas = document.createElement("canvas");
  canvas.width = w;
  canvas.height = h;
  const ctx = canvas.getContext("2d");
  if (!ctx) return "";

  // Vibrant gradient background (so quality / compression are visible)
  const grad = ctx.createLinearGradient(0, 0, w, h);
  grad.addColorStop(0, "#6366f1");
  grad.addColorStop(0.5, "#8b5cf6");
  grad.addColorStop(1, "#ec4899");
  ctx.fillStyle = grad;
  ctx.fillRect(0, 0, w, h);

  // Grid (makes rotation / flip obvious)
  ctx.strokeStyle = "rgba(255,255,255,0.22)";
  ctx.lineWidth = 1;
  for (let x = 0; x <= w; x += 40) {
    ctx.beginPath();
    ctx.moveTo(x, 0);
    ctx.lineTo(x, h);
    ctx.stroke();
  }
  for (let y = 0; y <= h; y += 40) {
    ctx.beginPath();
    ctx.moveTo(0, y);
    ctx.lineTo(w, y);
    ctx.stroke();
  }

  // Solid color circles (makes color quantization visible)
  const colors = [
    "#ef4444",
    "#f59e0b",
    "#10b981",
    "#3b82f6",
    "#f43f5e",
    "#14b8a6",
  ];
  colors.forEach((c, i) => {
    ctx.fillStyle = c;
    ctx.beginPath();
    ctx.arc(70 + i * 60, 300, 20, 0, Math.PI * 2);
    ctx.fill();
  });

  // Diagonal line (rotation visibility)
  ctx.strokeStyle = "#ffffff";
  ctx.lineWidth = 6;
  ctx.beginPath();
  ctx.moveTo(40, 40);
  ctx.lineTo(w - 40, h - 40);
  ctx.stroke();

  // Label text
  ctx.fillStyle = "#ffffff";
  ctx.textAlign = "center";
  ctx.textBaseline = "middle";
  ctx.font = "bold 38px system-ui, sans-serif";
  ctx.fillText("示例 SAMPLE", w / 2, 80);
  ctx.font = "18px system-ui, sans-serif";
  ctx.fillText("拖动 / 旋转 / 水印 — drag · rotate · watermark", w / 2, 120);

  return canvas.toDataURL("image/png");
}

/**
 * Returns the built-in template image as a `data:image/png;base64,...` URL.
 * Drawn lazily and cached for the session.
 */
export function getTemplateImageDataUrl(): string {
  if (cachedTemplate === null) {
    cachedTemplate = drawTemplate();
  }
  return cachedTemplate;
}

/**
 * Returns just the base64 payload (no `data:` prefix), ready to pass to the
 * `preview_image_data` command.
 */
export function getTemplateImageBase64(): string {
  const url = getTemplateImageDataUrl();
  const idx = url.indexOf("base64,");
  return idx >= 0 ? url.slice(idx + "base64,".length) : url;
}
