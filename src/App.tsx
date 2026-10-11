import { For, Show } from "solid-js";
import {
  ActionButtons,
  CanvasView,
  DropZone,
  FileList,
  LanguageSwitcher,
  PreviewPanel,
  ResultsPanel,
  SettingsPanel,
  ThemeToggle,
  UpdateNotification,
} from "./components";
import { AlertCircleIcon, XIcon } from "./components/Icons";
import { useTranslation } from "./i18n";
import { canvasActions, store as canvasStore } from "./store/canvasStore";
import { appActions, store } from "./store/useAppStore";
import type { ViewMode } from "./types/canvas";
import "./App.css";

/** Top-level mode tabs: batch pipeline vs canvas compose. */
function ViewModeTabs() {
  const { t } = useTranslation();
  const tabs: { mode: ViewMode; label: string }[] = [
    { mode: "batch", label: t("canvas.tabs.batch") },
    { mode: "canvas", label: t("canvas.tabs.canvas") },
  ];
  return (
    <div class="flex rounded-xl bg-slate-100 p-1 dark:bg-slate-800">
      <For each={tabs}>
        {(tab) => (
          <button
            type="button"
            onClick={() => canvasActions.setViewMode(tab.mode)}
            classList={{
              "rounded-lg px-4 py-1.5 text-sm font-semibold transition-all duration-200": true,
              "bg-white text-indigo-600 shadow-sm dark:bg-slate-700 dark:text-indigo-300":
                canvasStore.viewMode === tab.mode,
              "text-slate-500 hover:text-slate-700 dark:text-slate-400 dark:hover:text-slate-200":
                canvasStore.viewMode !== tab.mode,
            }}
          >
            {tab.label}
          </button>
        )}
      </For>
    </div>
  );
}

function BatchView() {
  return (
    <>
      {/* Main Content */}
      <div class="grid grid-cols-1 lg:grid-cols-2 gap-5">
        {/* Left Column */}
        <div class="space-y-5">
          <DropZone />
          <FileList />
          <PreviewPanel />
          <ResultsPanel />
        </div>

        {/* Right Column */}
        <div class="space-y-5">
          <SettingsPanel />
        </div>
      </div>

      {/* Action Buttons */}
      <ActionButtons />
    </>
  );
}

function App() {
  const { t } = useTranslation();

  return (
    <div class="h-screen flex flex-col bg-[var(--surface-app)] overflow-hidden">
      {/* Header + global banners */}
      <div class="shrink-0 space-y-3 px-5 pt-4">
        <div class="flex justify-between items-center">
          <div class="flex items-center gap-3">
            <img
              src="/favicon.png"
              alt=""
              class="rounded-[10px] shadow-md"
              style={{ width: "40px", height: "40px" }}
            />
            <h1 class="text-2xl font-semibold tracking-tight text-slate-900 dark:text-slate-50">
              {t("app.title")}
            </h1>
          </div>
          <div class="flex items-center gap-3">
            <ViewModeTabs />
            <ThemeToggle />
            <LanguageSwitcher />
          </div>
        </div>

        {/* Update Notification */}
        <UpdateNotification />

        {/* Error Message */}
        <Show when={store.error}>
          <div class="bg-rose-50 border border-rose-200 text-rose-700 px-4 py-3 rounded-xl shadow-sm flex items-center gap-3 animate-fadeIn">
            <AlertCircleIcon class="w-5 h-5 text-rose-500 flex-shrink-0" />
            <span class="flex-1 text-sm">{store.error}</span>
            <button
              type="button"
              onClick={() => appActions.setError(null)}
              class="p-1.5 hover:bg-rose-100 rounded-lg transition-colors"
            >
              <XIcon class="w-4 h-4" />
            </button>
          </div>
        </Show>
      </div>

      {/* Active view: the canvas composes full-bleed and fills the remaining
          viewport; the batch grid keeps its centered readable width and
          scrolls the page. */}
      <Show
        when={canvasStore.viewMode === "canvas"}
        fallback={
          <div class="flex-1 min-h-0 overflow-y-auto px-5 pb-5 animate-fadeIn">
            <div class="max-w-5xl mx-auto">
              <BatchView />
            </div>
          </div>
        }
      >
        <div class="flex-1 min-h-0 px-5 pb-4 pt-1 animate-fadeIn">
          <CanvasView />
        </div>
      </Show>
    </div>
  );
}

export default App;
