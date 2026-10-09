import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { open } from "@tauri-apps/plugin-dialog";
import { createSignal, onCleanup, onMount, Show } from "solid-js";
import { useTranslation } from "../i18n";
import { appActions, store } from "../store/useAppStore";
import type { FileItem, FileTypeDetectResult } from "../types";
import { ACCEPTED_IMAGE_EXTENSIONS } from "../utils/constants";
import { basename } from "../utils/path";
import { sanitizeError } from "../utils/sanitize";
import { FileIcon, FolderIcon, UploadCloudIcon } from "./Icons";

const ACCEPTED_EXTENSIONS = ACCEPTED_IMAGE_EXTENSIONS;

// MIME type → file extension for pasted images without a real path.
const PASTE_MIME_EXT: Record<string, string> = {
  "image/png": "png",
  "image/jpeg": "jpg",
  "image/webp": "webp",
  "image/gif": "gif",
  "image/bmp": "bmp",
};

// ArrayBuffer → base64, chunked so String.fromCharCode never overflows the
// stack on multi-megabyte screenshots.
function arrayBufferToBase64(buffer: ArrayBuffer): string {
  const bytes = new Uint8Array(buffer);
  let binary = "";
  const CHUNK = 0x8000;
  for (let i = 0; i < bytes.length; i += CHUNK) {
    binary += String.fromCharCode(...bytes.subarray(i, i + CHUNK));
  }
  return btoa(binary);
}

export function DropZone() {
  const { t } = useTranslation();
  const [isDropped, setIsDropped] = createSignal(false);
  const [isDragActive, setIsDragActive] = createSignal(false);

  const isProcessing = () => store.processingState === "processing";
  // Once files exist the zone shrinks to a single-row bar to save
  // vertical space; dropping keeps working on the full area either way.
  const hasFiles = () => store.files.length > 0;

  // Detect real file types through the batch API and reflect them in the
  // store. The whole drop is one IPC round-trip (instead of one per file).
  // Best-effort: a detection failure never blocks adding the files.
  const detectTypes = async (paths: string[]) => {
    try {
      const results = await invoke<FileTypeDetectResult[]>(
        "detect_file_types",
        {
          paths,
        },
      );
      appActions.setFileTypes(
        results
          .map((r) => r.info)
          .filter((i): i is NonNullable<typeof i> => i != null),
      );
      // Log the file name only: full paths in the console can leak the
      // user's directory structure if logs are ever collected elsewhere.
      for (const r of results) {
        if (r.error) {
          console.error(
            "Failed to detect file type:",
            basename(r.path),
            r.error,
          );
        }
      }
    } catch (err) {
      // Whole-batch IPC failure: nothing was applied.
      console.error("Failed to detect file types:", err);
    }
  };

  const processDroppedPaths = async (paths: string[]) => {
    if (isProcessing()) return;

    try {
      // Whitelist the dropped/picked paths first: every disk-touching
      // command verifies its paths against this session whitelist.
      await invoke("register_allowed_paths", { paths });

      // Get all image files (including from directories)
      const imagePaths = await invoke<string[]>("get_image_files", { paths });

      const fileItems: FileItem[] = imagePaths.map((path) => ({
        path,
        name: basename(path),
        size: 0, // Will be populated when processing
        status: "pending" as const,
      }));

      appActions.addFiles(fileItems);

      // Reflect real file types in the display at drop time
      void detectTypes(imagePaths);

      // Trigger drop animation
      setIsDropped(true);
      setTimeout(() => setIsDropped(false), 300);
    } catch (error) {
      console.error("Failed to process dropped files:", error);
      // Silent failure is not an option: a dropped folder that adds nothing
      // must tell the user why (whitelist rejection, IPC error, ...).
      appActions.setError(
        `${t("errors.addFilesFailed")}: ${sanitizeError(String(error))}`,
      );
    }
  };

  // Set up Tauri drag and drop event listener
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
          if (event.payload.type === "over") {
            setIsDragActive(true);
          } else if (event.payload.type === "drop") {
            setIsDragActive(false);
            void processDroppedPaths(event.payload.paths);
          } else if (event.payload.type === "leave") {
            setIsDragActive(false);
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

    // ---- Paste support -----------------------------------------------------
    // A pasted screenshot has no file path (it lives only in the clipboard):
    // its bytes go to the backend, which parks them in the temp folder and
    // returns a path the normal pipeline accepts. Files copied from Explorer
    // carry a Tauri-injected `path` and are used as-is.
    const pastedFilePath = async (file: File): Promise<string | null> => {
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
    };

    const handlePaste = (e: ClipboardEvent) => {
      if (isProcessing()) return;
      const files = Array.from(e.clipboardData?.files ?? []);
      const images = files.filter(
        (f) =>
          f.type.startsWith("image/") ||
          ACCEPTED_EXTENSIONS.includes(
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
        if (paths.length > 0) await processDroppedPaths(paths);
      })();
    };

    window.addEventListener("paste", handlePaste);
    onCleanup(() => window.removeEventListener("paste", handlePaste));
  });

  // Handle click to open file picker
  const handleSelectFiles = async (e: MouseEvent) => {
    e.stopPropagation();
    if (isProcessing()) return;

    try {
      const selected = await open({
        multiple: true,
        directory: false,
        filters: [
          {
            name: "Images",
            extensions: ACCEPTED_EXTENSIONS,
          },
        ],
      });

      if (selected) {
        const paths = Array.isArray(selected) ? selected : [selected];
        await processDroppedPaths(paths);
      }
    } catch (error) {
      console.error("Failed to open file picker:", error);
      appActions.setError(
        `${t("errors.addFilesFailed")}: ${sanitizeError(String(error))}`,
      );
    }
  };

  // Handle click to open folder picker
  const handleSelectFolder = async (e: MouseEvent) => {
    e.stopPropagation();
    if (isProcessing()) return;

    try {
      const selected = await open({
        multiple: false,
        directory: true,
      });

      if (selected) {
        const paths = Array.isArray(selected) ? selected : [selected];
        await processDroppedPaths(paths);
      }
    } catch (error) {
      console.error("Failed to open folder picker:", error);
      appActions.setError(
        `${t("errors.addFilesFailed")}: ${sanitizeError(String(error))}`,
      );
    }
  };

  return (
    <div
      class={`
        relative overflow-hidden
        border-2 border-dashed rounded-2xl
        transition-all duration-300 ease-out
        ${
          isDragActive()
            ? "border-indigo-400 bg-indigo-50/80 scale-[1.02] shadow-lg shadow-indigo-200/50"
            : "border-slate-200 bg-white/60 dark:border-slate-700 dark:bg-white/5"
        }
        ${isProcessing() ? "opacity-50 pointer-events-none" : ""}
        ${isDropped() ? "animate-dropBounce" : ""}
      `}
    >
      {/* Background gradient overlay */}
      <div class="absolute inset-0 bg-gradient-to-br from-indigo-500/5 via-transparent to-violet-500/5 pointer-events-none" />

      <Show
        when={!hasFiles()}
        fallback={
          /* Compact bar: keeps "add more" cheap once the list exists */
          <div class="relative flex items-center gap-3 p-3 text-left">
            <div
              class={`
                w-9 h-9 rounded-xl bg-indigo-50
                flex items-center justify-center flex-shrink-0
                transition-all duration-300
                ${isDragActive() ? "scale-110" : ""}
              `}
            >
              <UploadCloudIcon class="w-5 h-5 text-indigo-500" />
            </div>
            <div class="flex-1 min-w-0">
              <p class="text-sm font-semibold text-slate-700">
                {t("dropzone.addMore")}
              </p>
              <p class="text-xs text-slate-400 truncate">
                {t("dropzone.supported")}
              </p>
            </div>
            <div class="flex gap-2 flex-shrink-0">
              <button
                type="button"
                onClick={handleSelectFiles}
                disabled={isProcessing()}
                class="
                  inline-flex items-center gap-1.5 px-3 py-1.5
                  bg-[var(--surface-card)] border border-slate-200 rounded-lg dark:border-slate-700
                  text-xs font-medium text-slate-700
                  hover:bg-slate-50 hover:border-slate-300 hover:shadow-sm
                  transition-all duration-200
                  disabled:opacity-50 disabled:cursor-not-allowed
                "
              >
                <FileIcon class="w-3.5 h-3.5 text-indigo-500" />
                {t("dropzone.selectFiles")}
              </button>
              <button
                type="button"
                onClick={handleSelectFolder}
                disabled={isProcessing()}
                class="
                  inline-flex items-center gap-1.5 px-3 py-1.5
                  bg-[var(--surface-card)] border border-slate-200 rounded-lg dark:border-slate-700
                  text-xs font-medium text-slate-700
                  hover:bg-slate-50 hover:border-slate-300 hover:shadow-sm
                  transition-all duration-200
                  disabled:opacity-50 disabled:cursor-not-allowed
                "
              >
                <FolderIcon class="w-3.5 h-3.5 text-violet-500" />
                {t("dropzone.selectFolder")}
              </button>
            </div>
          </div>
        }
      >
        {/* Empty state (slimmed-down version of the original hero) */}
        <div class="relative p-6 text-center space-y-3">
          <div
            class={`
              mx-auto w-14 h-14 rounded-2xl bg-indigo-50
              flex items-center justify-center
              transition-all duration-300
              ${isDragActive() ? "scale-110 shadow-lg shadow-indigo-200/50" : ""}
            `}
          >
            <UploadCloudIcon
              class={`
                w-7 h-7 text-indigo-500
                transition-transform duration-300
                ${isDragActive() ? "-translate-y-1" : ""}
              `}
            />
          </div>

          <div class="space-y-1">
            <p class="text-base font-semibold text-slate-700">
              {t("dropzone.title")}
            </p>
            <p class="text-xs text-slate-400 font-medium">
              {t("dropzone.supported")}
            </p>
            <p class="text-xs text-slate-400">{t("dropzone.pasteHint")}</p>
          </div>

          <div class="flex justify-center gap-2.5 pt-1">
            <button
              type="button"
              onClick={handleSelectFiles}
              disabled={isProcessing()}
              class="
                inline-flex items-center gap-2 px-3.5 py-1.5
                bg-[var(--surface-card)] border border-slate-200 rounded-lg dark:border-slate-700
                text-sm font-medium text-slate-700
                hover:bg-slate-50 hover:border-slate-300 hover:shadow-sm
                transition-all duration-200
                disabled:opacity-50 disabled:cursor-not-allowed
              "
            >
              <FileIcon class="w-4 h-4 text-indigo-500" />
              {t("dropzone.selectFiles")}
            </button>
            <button
              type="button"
              onClick={handleSelectFolder}
              disabled={isProcessing()}
              class="
                inline-flex items-center gap-2 px-3.5 py-1.5
                bg-[var(--surface-card)] border border-slate-200 rounded-lg dark:border-slate-700
                text-sm font-medium text-slate-700
                hover:bg-slate-50 hover:border-slate-300 hover:shadow-sm
                transition-all duration-200
                disabled:opacity-50 disabled:cursor-not-allowed
              "
            >
              <FolderIcon class="w-4 h-4 text-violet-500" />
              {t("dropzone.selectFolder")}
            </button>
          </div>
        </div>
      </Show>
    </div>
  );
}
