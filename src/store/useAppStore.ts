import { createStore } from "solid-js/store";
import type {
  BatchStats,
  FileItem,
  FileStatus,
  FileTypeInfo,
  OptionsTemplate,
  ProcessingOptions,
  ProcessingState,
  ProgressUpdate,
} from "../types";
import { defaultOptions, mergeDefaults } from "./defaults";
import {
  BUILTIN_TEMPLATES,
  loadCustomTemplates,
  newTemplateId,
  persistCustomTemplates,
} from "./templates";

interface AppState {
  // Files
  files: FileItem[];
  // Currently selected file for the preview panel
  selectedPath: string | null;
  // Processing options
  options: ProcessingOptions;
  // Output directory
  outputDir: string;
  // Processing state
  processingState: ProcessingState;
  // Progress
  progress: ProgressUpdate | null;
  // Results
  batchStats: BatchStats | null;
  // Error
  error: string | null;
  // User-defined option templates (ordered); built-ins live in ./templates
  templates: OptionsTemplate[];
  // Id of the currently applied template (UI highlight); manual edits clear it
  activeTemplateId: string | null;
}

const [store, setStore] = createStore<AppState>({
  files: [],
  selectedPath: null,
  options: defaultOptions,
  outputDir: "",
  processingState: "idle",
  progress: null,
  batchStats: null,
  error: null,
  templates: loadCustomTemplates(),
  activeTemplateId: null,
});

export { store };

export const appActions = {
  // Files
  addFiles(newFiles: FileItem[]) {
    // Snapshot the known paths first so we can detect what is truly new
    // (single add, batch add, or re-adding an existing path: no-op).
    const known = new Set(store.files.map((f) => f.path));
    const added = newFiles.filter((n) => !known.has(n.path));
    if (added.length === 0) return;
    setStore("files", (prev) => [...prev, ...added]);
    // Auto-switch the preview to the first newly added image, covering
    // single add, batch add, and the very first image entering the app.
    setStore("selectedPath", added[0].path);
  },
  removeFile(path: string) {
    setStore("files", (prev) => prev.filter((f) => f.path !== path));
    if (store.selectedPath === path) {
      setStore("selectedPath", null);
    }
  },
  clearFiles() {
    setStore({
      files: [],
      batchStats: null,
      processingState: "idle",
      selectedPath: null,
    });
  },
  setSelectedPath(path: string | null) {
    setStore("selectedPath", path);
  },
  updateFileStatus(
    path: string,
    status: FileStatus,
    result?: Partial<FileItem>,
  ) {
    setStore("files", (prev) =>
      prev.map((f) => (f.path === path ? { ...f, status, ...result } : f)),
    );
  },
  setFileType(path: string, info: FileTypeInfo) {
    setStore("files", (prev) =>
      prev.map((f) =>
        f.path === path
          ? {
              ...f,
              detectedFormat: info.detected_format,
              extension: info.extension,
              matchesExtension: info.matches_extension,
              detectedMime: info.detected_mime,
              detectedWidth: info.width,
              detectedHeight: info.height,
            }
          : f,
      ),
    );
  },
  // Batch variant of `setFileType`: one store pass for a whole drop instead
  // of one map-over-all-files per detected item.
  setFileTypes(infos: FileTypeInfo[]) {
    if (infos.length === 0) return;
    const byPath = new Map(infos.map((i) => [i.path, i]));
    setStore("files", (prev) =>
      prev.map((f) => {
        const info = byPath.get(f.path);
        if (!info) return f;
        return {
          ...f,
          detectedFormat: info.detected_format,
          extension: info.extension,
          matchesExtension: info.matches_extension,
          detectedMime: info.detected_mime,
          detectedWidth: info.width,
          detectedHeight: info.height,
        };
      }),
    );
  },
  resetFileStatuses() {
    setStore("files", (prev) =>
      prev.map((f) => ({
        ...f,
        status: "pending" as const,
        outputPath: undefined,
        outputSize: undefined,
        reductionPercent: undefined,
        error: undefined,
      })),
    );
  },

  // Processing options
  setOptions(newOptions: Partial<ProcessingOptions>) {
    setStore("options", (prev) => ({ ...prev, ...newOptions }));
    // Any manual edit takes the settings off the applied template.
    setStore("activeTemplateId", null);
  },

  // Option templates -------------------------------------------------------
  saveTemplate(name: string) {
    const trimmed = name.trim();
    if (!trimmed) return;
    const tpl: OptionsTemplate = {
      id: newTemplateId(),
      name: trimmed,
      // Snapshot of the current parameter combination. A shallow copy is
      // enough: every setter replaces nested objects, never mutates them.
      options: { ...store.options },
    };
    setStore("templates", (prev) => [...prev, tpl]);
    persistCustomTemplates(store.templates);
  },
  renameTemplate(id: string, name: string) {
    const trimmed = name.trim();
    if (!trimmed) return;
    setStore("templates", (prev) =>
      prev.map((t) => (t.id === id ? { ...t, name: trimmed } : t)),
    );
    persistCustomTemplates(store.templates);
  },
  deleteTemplate(id: string) {
    setStore("templates", (prev) => prev.filter((t) => t.id !== id));
    persistCustomTemplates(store.templates);
    if (store.activeTemplateId === id) {
      setStore("activeTemplateId", null);
    }
  },
  moveTemplate(id: string, direction: -1 | 1) {
    setStore("templates", (prev) => {
      const i = prev.findIndex((t) => t.id === id);
      const j = i + direction;
      if (i < 0 || j < 0 || j >= prev.length) return prev;
      const next = [...prev];
      [next[i], next[j]] = [next[j], next[i]];
      return next;
    });
    persistCustomTemplates(store.templates);
  },
  applyTemplate(id: string) {
    const tpl =
      BUILTIN_TEMPLATES.find((t) => t.id === id) ??
      store.templates.find((t) => t.id === id);
    if (!tpl) return;
    // Full snapshot replace: every parameter syncs to the template at once,
    // and batch processing picks the new options up via store.options.
    // mergeDefaults keeps options that were introduced after an older
    // template was saved well-defined instead of `undefined`.
    setStore("options", mergeDefaults(tpl.options));
    setStore("activeTemplateId", id);
  },

  // Output directory
  setOutputDir(dir: string) {
    setStore("outputDir", dir);
  },

  // Processing state
  setProcessingState(state: ProcessingState) {
    setStore("processingState", state);
  },

  // Progress
  setProgress(progress: ProgressUpdate | null) {
    setStore("progress", progress);
  },

  // Results
  setBatchStats(stats: BatchStats | null) {
    setStore("batchStats", stats);
  },

  // Error
  setError(error: string | null) {
    setStore("error", error);
  },

  // Reset
  reset() {
    setStore({
      files: [],
      selectedPath: null,
      processingState: "idle",
      progress: null,
      batchStats: null,
      error: null,
    });
  },
};
