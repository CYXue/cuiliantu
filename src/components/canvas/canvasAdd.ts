//! Adding source images to the canvas: whitelist registration → image-file
//! expansion → per-file display thumbnails (best-effort) → store layers.

import { invoke } from "@tauri-apps/api/core";
import { canvasActions } from "../../store/canvasStore";
import type { CanvasLayerInput } from "../../types/canvas";
import { basename } from "../../utils/path";

interface LayerThumbnailPayload {
  base64_png: string;
  natural_width: number;
  natural_height: number;
}

/**
 * Convert a base64 data URL into a blob URL.
 *
 * Chromium/WebView2 rejects any data: URL longer than 2 MB
 * (`url::kMaxURLChars`), and a 2048px PNG thumbnail base64-encodes to
 * several MB — canvas layers rendered as blank images because of it. Blob
 * URLs have no length limit and keep megabytes of base64 text out of the
 * DOM. The store revokes them when a layer is removed.
 */
async function dataUrlToBlobUrl(dataUrl: string): Promise<string> {
  const blob = await (await fetch(dataUrl)).blob();
  return URL.createObjectURL(blob);
}

/** Layer layers default to the source's natural pixel size (canvas units =
 * export pixels, so 1:1 is the least surprising scale). */
export async function addImagesFromPaths(paths: string[]): Promise<number> {
  if (paths.length === 0) return 0;
  // Whitelist first: every disk-touching command verifies against the
  // session whitelist (same flow as the batch drop zone).
  await invoke("register_allowed_paths", { paths });
  // Directories are expanded to their image files; plain files pass through.
  const imagePaths = await invoke<string[]>("get_image_files", { paths });

  const inputs: CanvasLayerInput[] = [];
  for (const path of imagePaths) {
    try {
      const thumb = await invoke<LayerThumbnailPayload>(
        "canvas_layer_thumbnail",
        { path },
      );
      inputs.push({
        path,
        name: basename(path),
        src: await dataUrlToBlobUrl(
          `data:image/png;base64,${thumb.base64_png}`,
        ),
        width: thumb.natural_width,
        height: thumb.natural_height,
        naturalWidth: thumb.natural_width,
        naturalHeight: thumb.natural_height,
      });
    } catch (error) {
      // One unreadable image must not block the rest of the drop.
      console.error("Failed to load canvas layer:", basename(path), error);
    }
  }
  canvasActions.addLayers(inputs);
  if (inputs.length > 0) canvasActions.requestFit();
  return inputs.length;
}
