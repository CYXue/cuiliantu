//! Window-level drag-and-drop and clipboard-paste wiring for the drop zone.
//!
//! Extracted from `DropZone` (which mixed five interactions in one component):
//! these hooks own the *event plumbing* (Tauri window events, paste events,
//! the temp-parking of pathless pasted screenshots), while the component owns
//! the *pipeline* (`processDroppedPaths`) and its UI feedback.
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { onCleanup, onMount } from "solid-js";

import {
  ACCEPTED_IMAGE_EXTENSIONS,
  BASE64_CHUNK_SIZE,
} from "../utils/constants";
import { basename } from "../utils/path";

// MIME type → file extension for pasted images without a real path.
const PASTE_MIME_EXT: Record<string, string> = {
  "image/png": "png",
  "image/jpeg": "jpg",
  "image/webp": "webp",
  "image/gif": "gif",
  "image/bmp": "bmp",
};

// ArrayBuffer → base64, chunked so String.fromCharCode never overflows the
// stack on multi-megabyte screenshots. Chunks land in an array joined once:
// repeated `+=` would reallocate the whole string per chunk (O(n²) on a
// multi-MB paste).
function arrayBufferToBase64(buffer: ArrayBuffer): string {
  const bytes = new Uint8Array(buffer);
  const chunks: string[] = [];
  for (let i = 0; i < bytes.length; i += BASE64_CHUNK_SIZE) {
    chunks.push(
      String.fromCharCode(...bytes.subarray(i, i + BASE64_CHUNK_SIZE)),
    );
  }
  return btoa(chunks.join(""));
}

/** Park a pathless pasted screenshot on the backend; keep Explorer paths. */
async function pastedFilePath(file: File): Promise<string | null> {
  const injected = (file as File & { path?: string }).path;
  if (typeof injected === "string" && injected.length > 0) {
    return injected;
  }
  const ext = PASTE_MIME_EXT[file.type] ?? "png";
  try {
    const base64 = arrayBufferToBase64(await file.arrayBuffer());
    return await invoke<string>("save_temp_image", {
      dataBase64: base64,
      ext,
    });
  } catch (err) {
    console.error("Failed to save pasted image:", basename(file.name), err);
    return null;
  }
}

/**
 * Subscribe to the window's Tauri drag-and-drop events. `onActiveChange`
 * mirrors over/leave for the visual highlight; `onDrop` receives the final
 * path list. Safe to call during component setup (cleans up on unmount).
 */
export function useTauriDragDrop(opts: {
  onDrop: (paths: string[]) => void;
  onActiveChange: (active: boolean) => void;
}): void {
  onMount(() => {
    let unlisten: (() => void) | undefined;
    // Whether the component was unmounted before `onDragDropEvent` resolved.
    // At cleanup time `unlisten` is still unassigned; without this flag the
    // subscription established later would never be torn down.
    let disposed = false;

    const setupDragDrop = async () => {
      // Check if running in Tauri environment
      if (typeof window === "undefined" || !("__TAURI__" in window)) {
        console.warn(
          "Not running in Tauri environment, drag and drop disabled",
        );
        return;
      }

      try {
        const currentWindow = getCurrentWindow();
        const stop = await currentWindow.onDragDropEvent((event) => {
          // "enter" is deliberately ignored, matching the original
          // handler: the highlight waits for the first "over" tick.
          if (event.payload.type === "over") {
            opts.onActiveChange(true);
          } else if (event.payload.type === "drop") {
            opts.onActiveChange(false);
            opts.onDrop(event.payload.paths);
          } else if (event.payload.type === "leave") {
            opts.onActiveChange(false);
          }
        });

        // Unmounted while awaiting establishment: tear down right here.
        if (disposed) {
          stop();
          return;
        }
        unlisten = stop;
      } catch (error) {
        console.error("Failed to set up drag and drop:", error);
      }
    };

    void setupDragDrop();

    onCleanup(() => {
      disposed = true;
      if (unlisten) {
        unlisten();
      }
    });
  });
}

/**
 * Listen for clipboard image pastes. `enabled` gates the handler (no paste
 * processing mid-batch); `onPastedPaths` receives every successfully parked
 * or resolved path, batched into one callback.
 */
export function usePasteHandler(opts: {
  enabled: () => boolean;
  onPastedPaths: (paths: string[]) => void;
}): void {
  onMount(() => {
    const handlePaste = (e: ClipboardEvent) => {
      if (opts.enabled()) return;
      const files = Array.from(e.clipboardData?.files ?? []);
      const images = files.filter(
        (f) =>
          f.type.startsWith("image/") ||
          ACCEPTED_IMAGE_EXTENSIONS.includes(
            f.name.split(".").pop()?.toLowerCase() ?? "",
          ),
      );
      if (images.length === 0) return;
      e.preventDefault();
      void (async () => {
        const paths: string[] = [];
        for (const f of images) {
          const p = await pastedFilePath(f);
          if (p) paths.push(p);
        }
        if (paths.length > 0) opts.onPastedPaths(paths);
      })();
    };

    window.addEventListener("paste", handlePaste);
    onCleanup(() => window.removeEventListener("paste", handlePaste));
  });
}
