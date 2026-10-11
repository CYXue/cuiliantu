//! Canvas export flow: save dialog → whitelist registration → compose IPC.
//!
//! Kept out of the toolbar component so the pipeline reads top-to-bottom and
//! the dialog-cancel path stays explicit.

import { invoke } from "@tauri-apps/api/core";
import { save } from "@tauri-apps/plugin-dialog";
import { backgroundHexOf, canvasActions, store } from "../../store/canvasStore";
import { exportRectOf } from "../../utils/canvas";
import { basename } from "../../utils/path";
import { sanitizeError } from "../../utils/sanitize";

export type CanvasExportFormat = "png" | "jpeg" | "webp";

interface ComposeResultPayload {
  output_path: string;
  width: number;
  height: number;
  output_size: number;
}

const FORMAT_EXTENSIONS: Record<CanvasExportFormat, string[]> = {
  png: ["png"],
  jpeg: ["jpg", "jpeg"],
  webp: ["webp"],
};

/**
 * Export the canvas (selection rect or auto bounding box) to a file the user
 * picks through the save dialog. The picked path is registered with the
 * session whitelist right before the compose call — registration and
 * validation stay separate steps, same discipline as the CSV report export.
 */
export async function exportCanvas(
  format: CanvasExportFormat,
  quality: number,
  t: (key: string) => string,
): Promise<void> {
  if (store.exporting) return;

  const visible = store.layers.filter((l) => l.visible);
  if (visible.length === 0) {
    canvasActions.setExportError(t("canvas.errors.noLayers"));
    return;
  }
  const rect = exportRectOf(store.layers, store.selection);
  if (!rect) {
    canvasActions.setExportError(t("canvas.errors.noLayers"));
    return;
  }

  const ext = format === "jpeg" ? "jpg" : format;
  let destination: string | null;
  try {
    destination = await save({
      defaultPath: `cuiliantu-canvas.${ext}`,
      filters: [
        { name: format.toUpperCase(), extensions: FORMAT_EXTENSIONS[format] },
      ],
    });
  } catch (error) {
    console.error("Failed to open save dialog:", error);
    canvasActions.setExportError(sanitizeError(String(error)));
    return;
  }
  // Dialog cancelled — not an error, just nothing to do.
  if (!destination) return;

  try {
    await invoke("register_allowed_paths", { paths: [destination] });
  } catch (error) {
    console.error("Failed to register export destination:", error);
    canvasActions.setExportError(sanitizeError(String(error)));
    return;
  }

  canvasActions.setExporting(true);
  try {
    const result = await invoke<ComposeResultPayload>("compose_canvas", {
      request: {
        layers: visible.map((l) => ({
          path: l.path,
          x: l.x,
          y: l.y,
          width: l.width,
          height: l.height,
          rotation_degrees: l.rotation,
          flip_h: l.flipH,
          flip_v: l.flipV,
          opacity: l.opacity,
          crop: l.crop ?? null,
        })),
        background: backgroundHexOf(store.backgroundMode, store.customColor),
        export_rect: [rect.x, rect.y, rect.width, rect.height],
        output_path: destination,
        format,
        quality: Math.round(quality),
      },
    });
    canvasActions.setExportResult({
      name: basename(result.output_path),
      width: result.width,
      height: result.height,
      sizeBytes: result.output_size,
    });
  } catch (error) {
    console.error("Canvas export failed:", error);
    canvasActions.setExportError(sanitizeError(String(error)));
  } finally {
    canvasActions.setExporting(false);
  }
}
