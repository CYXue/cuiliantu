//! Parameter-template management (built-in presets + user-defined CRUD).
//!
//! Extracted from `SettingsPanel`: the template block carried its own
//! signals, two-step delete flow and rename editing — a self-contained
//! concern that made the panel a 590-line mix of seven responsibilities.
import { createSignal, For, Show } from "solid-js";
import { useTranslation } from "../../i18n";
import { BUILTIN_TEMPLATES } from "../../store/templates";
import { appActions, store } from "../../store/useAppStore";
import type { OptionsTemplate } from "../../types";
import {
  CheckIcon,
  ChevronDownIcon,
  ChevronUpIcon,
  PencilIcon,
  PlusIcon,
  TrashIcon,
  XIcon,
} from "../Icons";

export function TemplateSection() {
  const { t } = useTranslation();

  const isProcessing = () => store.processingState === "processing";

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

  return (
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
  );
}
