import { invoke } from "@tauri-apps/api/core";
import { createEffect, createSignal, For, Show } from "solid-js";
import { useTranslation } from "../i18n";
import { BUILTIN_TEMPLATES } from "../store/templates";
import { appActions, store } from "../store/useAppStore";
import type { CompressionType, OptionsTemplate, OutputFormat } from "../types";
import { pickOutputDir } from "../utils/dialog";
import { sanitizeError } from "../utils/sanitize";
import { sliderFillStyle } from "../utils/slider";
import {
  CheckIcon,
  ChevronDownIcon,
  ChevronUpIcon,
  FolderIcon,
  PencilIcon,
  PlusIcon,
  SettingsIcon,
  TrashIcon,
  XIcon,
} from "./Icons";
import { AdvancedSection } from "./settings/AdvancedSection";
import { EditSection } from "./settings/EditSection";
import { ResizeSection } from "./settings/ResizeSection";
import { TransformSection } from "./settings/TransformSection";
import { WatermarkSection } from "./settings/WatermarkSection";

const OUTPUT_FORMATS: OutputFormat[] = [
  "webp",
  "jpeg",
  "png",
  "gif",
  "bmp",
  "tiff",
];

export function SettingsPanel() {
  const { t } = useTranslation();

  const isProcessing = () => store.processingState === "processing";

  // Target-size mode: when enabled, JPEG output auto-bisects quality to fit a
  // byte budget (the old separate "upload prep" mode, now folded in).
  const [kb, setKb] = createSignal(50);
  const targetEnabled = () => store.options.target_size_bytes !== null;

  // Keep the KB fallback in sync no matter what changed the target size
  // (manual input, template application) so re-enabling the toggle
  // restores the last used budget.
  createEffect(() => {
    const ts = store.options.target_size_bytes;
    if (ts !== null) setKb(Math.round(ts / 1024));
  });

  // --- Templates -----------------------------------------------------------
  const [saving, setSaving] = createSignal(false);
  const [newName, setNewName] = createSignal("");
  const [renamingId, setRenamingId] = createSignal<string | null>(null);
  const [renameValue, setRenameValue] = createSignal("");
  // Two-step delete: first click arms the row, second click really deletes.
  const [confirmDeleteId, setConfirmDeleteId] = createSignal<string | null>(
    null,
  );

  // Built-in names are i18n keys; custom names are plain text.
  const templateLabel = (tpl: OptionsTemplate) =>
    tpl.id.startsWith("builtin:") ? t(tpl.name) : tpl.name;

  const startRename = (tpl: OptionsTemplate) => {
    setRenamingId(tpl.id);
    setRenameValue(tpl.name);
    setConfirmDeleteId(null);
  };
  const commitRename = () => {
    const id = renamingId();
    if (id) appActions.renameTemplate(id, renameValue());
    setRenamingId(null);
  };
  const handleDelete = (id: string) => {
    if (confirmDeleteId() === id) {
      appActions.deleteTemplate(id);
      setConfirmDeleteId(null);
    } else {
      setConfirmDeleteId(id);
    }
  };
  const startSaving = () => {
    setSaving(true);
    setNewName("");
    setConfirmDeleteId(null);
  };
  const confirmSave = () => {
    if (!newName().trim()) return;
    appActions.saveTemplate(newName());
    setSaving(false);
  };

  const handleSelectOutputDir = async () => {
    try {
      const dir = await pickOutputDir(t("settings.selectDir"));
      if (dir) {
        // Whitelist the output root so process commands accept writes there.
        await invoke("register_allowed_paths", { paths: [dir] });
        appActions.setOutputDir(dir);
      }
    } catch (error) {
      console.error("Failed to select directory:", error);
      // The output directory is mandatory for a run; a silent failure here
      // would leave the user stuck with an empty field and no hint.
      appActions.setError(
        `${t("errors.outputDirFailed")}: ${sanitizeError(String(error))}`,
      );
    }
  };

  return (
    <div class="card divide-y divide-slate-100 animate-slideUp">
      {/* Header */}
      <div class="p-4 pb-3">
        <div class="flex items-center gap-2">
          <div class="w-8 h-8 rounded-lg bg-indigo-50 flex items-center justify-center">
            <SettingsIcon class="w-4 h-4 text-indigo-500" />
          </div>
          <h3 class="font-semibold text-slate-800">{t("settings.title")}</h3>
        </div>
      </div>

      <div class="p-4 space-y-5">
        {/* Parameter templates: one-click built-ins + user-defined, persisted */}
        <div class="space-y-2">
          <div class="flex items-center justify-between">
            <span class="text-sm font-medium text-slate-600">
              {t("settings.templates.title")}
            </span>
            <Show
              when={!saving()}
              fallback={
                <span class="text-xs text-slate-400">
                  {t("settings.templates.namePlaceholder")}
                </span>
              }
            >
              <button
                type="button"
                onClick={startSaving}
                disabled={isProcessing()}
                class="inline-flex items-center gap-1 text-xs font-medium text-indigo-600 hover:text-indigo-500 disabled:opacity-50"
              >
                <PlusIcon class="w-3.5 h-3.5" />
                {t("settings.templates.save")}
              </button>
            </Show>
          </div>
          <p class="text-xs text-slate-400">{t("settings.templates.hint")}</p>

          {/* Built-in presets */}
          <div class="flex flex-wrap gap-1.5">
            <For each={BUILTIN_TEMPLATES}>
              {(tpl) => (
                <button
                  type="button"
                  onClick={() => appActions.applyTemplate(tpl.id)}
                  disabled={isProcessing()}
                  aria-pressed={store.activeTemplateId === tpl.id}
                  class={`text-xs font-medium px-2.5 py-1.5 rounded-lg border transition-colors disabled:opacity-50 ${
                    store.activeTemplateId === tpl.id
                      ? "bg-indigo-500 border-indigo-500 text-white"
                      : "bg-slate-100 border-transparent text-slate-600 hover:bg-indigo-50 hover:text-indigo-600"
                  }`}
                >
                  {templateLabel(tpl)}
                </button>
              )}
            </For>
          </div>

          {/* User-defined templates */}
          <Show when={store.templates.length > 0}>
            <div class="space-y-1">
              <For each={store.templates}>
                {(tpl, i) => (
                  <div
                    class={`flex items-center gap-1.5 rounded-xl border px-2.5 py-1.5 ${
                      store.activeTemplateId === tpl.id
                        ? "border-indigo-300 bg-indigo-50/60"
                        : "border-slate-200"
                    }`}
                  >
                    <Show
                      when={renamingId() === tpl.id}
                      fallback={
                        <button
                          type="button"
                          class="flex-1 min-w-0 text-left text-sm text-slate-700 truncate hover:text-indigo-600"
                          onClick={() => appActions.applyTemplate(tpl.id)}
                          title={tpl.name}
                        >
                          {tpl.name}
                        </button>
                      }
                    >
                      <input
                        type="text"
                        class="flex-1 min-w-0 custom-input bg-white border border-slate-200 rounded-lg px-2 py-1 text-sm"
                        value={renameValue()}
                        onInput={(e) => setRenameValue(e.currentTarget.value)}
                        onKeyDown={(e) => {
                          if (e.key === "Enter") commitRename();
                          else if (e.key === "Escape") setRenamingId(null);
                        }}
                        placeholder={t("settings.templates.namePlaceholder")}
                      />
                      <button
                        type="button"
                        onClick={commitRename}
                        class="p-1 rounded-lg text-emerald-600 hover:bg-emerald-50"
                        aria-label={t("settings.templates.confirm")}
                      >
                        <CheckIcon class="w-3.5 h-3.5" />
                      </button>
                      <button
                        type="button"
                        onClick={() => setRenamingId(null)}
                        class="p-1 rounded-lg text-slate-400 hover:bg-slate-100"
                        aria-label={t("settings.templates.cancel")}
                      >
                        <XIcon class="w-3.5 h-3.5" />
                      </button>
                    </Show>
                    <Show when={renamingId() !== tpl.id}>
                      <button
                        type="button"
                        onClick={() => appActions.moveTemplate(tpl.id, -1)}
                        disabled={i() === 0}
                        class="p-1 rounded-lg text-slate-400 hover:bg-slate-100 disabled:opacity-30"
                        aria-label={t("settings.templates.moveUp")}
                      >
                        <ChevronUpIcon class="w-3.5 h-3.5" />
                      </button>
                      <button
                        type="button"
                        onClick={() => appActions.moveTemplate(tpl.id, 1)}
                        disabled={i() === store.templates.length - 1}
                        class="p-1 rounded-lg text-slate-400 hover:bg-slate-100 disabled:opacity-30"
                        aria-label={t("settings.templates.moveDown")}
                      >
                        <ChevronDownIcon class="w-3.5 h-3.5" />
                      </button>
                      <button
                        type="button"
                        onClick={() => startRename(tpl)}
                        class="p-1 rounded-lg text-slate-400 hover:bg-slate-100"
                        aria-label={t("settings.templates.rename")}
                      >
                        <PencilIcon class="w-3.5 h-3.5" />
                      </button>
                      <button
                        type="button"
                        onClick={() => handleDelete(tpl.id)}
                        class={`p-1 rounded-lg transition-colors ${
                          confirmDeleteId() === tpl.id
                            ? "text-rose-600 bg-rose-50"
                            : "text-slate-400 hover:bg-slate-100"
                        }`}
                        aria-label={t("settings.templates.delete")}
                        title={
                          confirmDeleteId() === tpl.id
                            ? t("settings.templates.deleteConfirm")
                            : t("settings.templates.delete")
                        }
                      >
                        <TrashIcon class="w-3.5 h-3.5" />
                      </button>
                    </Show>
                  </div>
                )}
              </For>
            </div>
            <Show when={confirmDeleteId()}>
              <p class="text-xs text-rose-500">
                {t("settings.templates.deleteConfirm")}
              </p>
            </Show>
          </Show>

          {/* Save current options as a named template */}
          <Show when={saving()}>
            <div class="flex items-center gap-2">
              <input
                type="text"
                class="flex-1 min-w-0 custom-input bg-slate-50 border border-slate-200 rounded-xl px-3 py-1.5 text-sm"
                placeholder={t("settings.templates.namePlaceholder")}
                value={newName()}
                onInput={(e) => setNewName(e.currentTarget.value)}
                onKeyDown={(e) => {
                  if (e.key === "Enter") confirmSave();
                  else if (e.key === "Escape") setSaving(false);
                }}
              />
              <button
                type="button"
                onClick={confirmSave}
                disabled={!newName().trim()}
                class="p-1.5 rounded-lg text-emerald-600 hover:bg-emerald-50 disabled:opacity-40"
                aria-label={t("settings.templates.confirm")}
              >
                <CheckIcon class="w-4 h-4" />
              </button>
              <button
                type="button"
                onClick={() => setSaving(false)}
                class="p-1.5 rounded-lg text-slate-400 hover:bg-slate-100"
                aria-label={t("settings.templates.cancel")}
              >
                <XIcon class="w-4 h-4" />
              </button>
            </div>
          </Show>
        </div>

        {/* Output Format */}
        <div class="space-y-2">
          <label
            class="text-sm font-medium text-slate-600"
            for="settings-format"
          >
            {t("settings.format")}
          </label>
          <select
            id="settings-format"
            value={store.options.format}
            onChange={(e) =>
              appActions.setOptions({
                format: e.currentTarget.value as OutputFormat,
              })
            }
            disabled={isProcessing()}
            class={`
              w-full custom-select
              bg-slate-50 border border-slate-200 rounded-xl
              px-4 py-2.5 text-sm text-slate-700 font-medium
              transition-all duration-200
              disabled:opacity-50 disabled:cursor-not-allowed
            `}
          >
            <For each={OUTPUT_FORMATS}>
              {(format) => (
                <option value={format}>{format.toUpperCase()}</option>
              )}
            </For>
          </select>
        </div>

        {/* Quality */}
        <div class="space-y-3">
          <div class="flex justify-between items-center">
            <label
              class="text-sm font-medium text-slate-600"
              for="settings-quality"
            >
              {t("settings.quality")}
            </label>
            <span class="text-sm font-bold text-indigo-600 bg-indigo-50 px-2.5 py-1 rounded-lg">
              {store.options.quality}%
            </span>
          </div>
          <input
            id="settings-quality"
            type="range"
            min="1"
            max="100"
            value={store.options.quality}
            onInput={(e) =>
              appActions.setOptions({
                quality: parseInt(e.currentTarget.value, 10),
              })
            }
            disabled={isProcessing()}
            class="w-full disabled:opacity-50"
            style={{
              background: sliderFillStyle(store.options.quality, 1, 100),
            }}
          />
        </div>

        {/* Target size: auto-fit JPEG to a byte budget (upload-prep folded in) */}
        <div class="space-y-3 pt-4 border-t border-slate-100">
          <div class="flex justify-between items-start gap-4">
            <div>
              <span class="text-sm font-medium text-slate-600">
                {t("settings.targetSize")}
              </span>
              <p class="text-xs text-slate-400 mt-0.5 max-w-[16rem]">
                {t("settings.targetSizeDesc")}
              </p>
            </div>
            <label class="relative inline-flex items-center cursor-pointer flex-shrink-0">
              <input
                type="checkbox"
                class="sr-only peer"
                checked={targetEnabled()}
                onChange={(e) =>
                  appActions.setOptions({
                    target_size_bytes: e.currentTarget.checked
                      ? (kb() || 50) * 1024
                      : null,
                  })
                }
                disabled={isProcessing()}
              />
              <div class="w-10 h-6 bg-slate-200 peer-checked:bg-indigo-500 rounded-full transition-colors"></div>
            </label>
          </div>

          <Show when={targetEnabled()}>
            <div class="space-y-3">
              <div class="flex items-center gap-3">
                <input
                  type="number"
                  min="1"
                  value={Math.round(
                    (store.options.target_size_bytes ?? 0) / 1024,
                  )}
                  onInput={(e) => {
                    const v = parseInt(e.currentTarget.value, 10);
                    if (!Number.isNaN(v) && v > 0) {
                      setKb(v);
                      appActions.setOptions({ target_size_bytes: v * 1024 });
                    }
                  }}
                  disabled={isProcessing()}
                  class="w-28 custom-input bg-slate-50 border border-slate-200 rounded-xl px-3 py-2 text-sm disabled:opacity-50"
                />
                <span class="text-sm text-slate-500">KB</span>
              </div>

              <Show when={store.options.format !== "jpeg"}>
                <p class="text-xs text-amber-600">
                  {t("settings.targetSizeJpegOnly")}
                </p>
              </Show>
            </div>
          </Show>
        </div>

        {/* Resize (pixels / percent + fit modes) */}
        <div class="pt-4 border-t border-slate-100">
          <ResizeSection />
        </div>

        {/* Rotate / Flip */}
        <div class="pt-4 border-t border-slate-100">
          <TransformSection />
        </div>

        {/* Image editing (crop / fine rotate / tonal / filters) */}
        <div class="pt-4 border-t border-slate-100">
          <EditSection />
        </div>

        {/* Picture watermark */}
        <div class="pt-4 border-t border-slate-100">
          <WatermarkSection />
        </div>

        {/* Advanced (quantize / animation / naming / conflicts) */}
        <div class="pt-4 border-t border-slate-100">
          <AdvancedSection />
        </div>

        {/* Metadata */}
        <div class="pt-4 border-t border-slate-100">
          <fieldset class="space-y-2">
            <legend class="text-sm font-medium text-slate-600">
              {t("settings.metadata")}
            </legend>
            <div class="flex gap-4">
              <label class="flex items-center gap-2 cursor-pointer">
                <input
                  type="radio"
                  name="metadata"
                  checked={store.options.keep_metadata}
                  onChange={() =>
                    appActions.setOptions({ keep_metadata: true })
                  }
                  disabled={isProcessing()}
                  class="custom-radio"
                />
                <span class="text-sm text-slate-700">
                  {t("settings.keepMetadata")}
                </span>
              </label>
              <label class="flex items-center gap-2 cursor-pointer">
                <input
                  type="radio"
                  name="metadata"
                  checked={!store.options.keep_metadata}
                  onChange={() =>
                    appActions.setOptions({ keep_metadata: false })
                  }
                  disabled={isProcessing()}
                  class="custom-radio"
                />
                <span class="text-sm text-slate-700">
                  {t("settings.removeMetadata")}
                </span>
              </label>
            </div>
          </fieldset>
        </div>

        {/* Compression */}
        <fieldset class="space-y-2">
          <legend class="text-sm font-medium text-slate-600">
            {t("settings.compression")}
          </legend>
          <div class="flex gap-4">
            <label class="flex items-center gap-2 cursor-pointer">
              <input
                type="radio"
                name="compression"
                checked={store.options.compression === "lossy"}
                onChange={() =>
                  appActions.setOptions({
                    compression: "lossy" as CompressionType,
                  })
                }
                disabled={isProcessing()}
                class="custom-radio"
              />
              <span class="text-sm text-slate-700">{t("settings.lossy")}</span>
            </label>
            <label class="flex items-center gap-2 cursor-pointer">
              <input
                type="radio"
                name="compression"
                checked={store.options.compression === "lossless"}
                onChange={() =>
                  appActions.setOptions({
                    compression: "lossless" as CompressionType,
                  })
                }
                disabled={isProcessing()}
                class="custom-radio"
              />
              <span class="text-sm text-slate-700">
                {t("settings.lossless")}
              </span>
            </label>
          </div>
        </fieldset>

        {/* Output Directory */}
        <div class="space-y-2">
          <label
            class="text-sm font-medium text-slate-600"
            for="settings-output-dir"
          >
            {t("settings.outputDir")}
          </label>
          <div class="flex gap-2">
            <input
              id="settings-output-dir"
              type="text"
              value={store.outputDir}
              disabled={isProcessing()}
              class="flex-1 custom-input bg-slate-50 border border-slate-200 rounded-xl px-4 py-2.5 text-sm text-slate-600 disabled:opacity-50"
              readOnly
              placeholder={t("settings.selectDir")}
            />
            <button
              type="button"
              onClick={handleSelectOutputDir}
              disabled={isProcessing()}
              class={`
                flex items-center gap-2 px-4 py-2.5
                bg-slate-100 hover:bg-slate-200 border border-slate-200
                rounded-xl text-sm font-medium text-slate-700
                transition-all duration-200
                ${isProcessing() ? "opacity-50 cursor-not-allowed" : ""}
              `}
            >
              <FolderIcon class="w-4 h-4" />
              {t("settings.selectDir")}
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}
