import { Show } from "solid-js";
import {
  ActionButtons,
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
import { appActions, store } from "./store/useAppStore";
import "./App.css";

function App() {
  const { t } = useTranslation();

  return (
    <div class="min-h-screen bg-[var(--surface-app)] p-6">
      <div class="max-w-5xl mx-auto space-y-5 animate-fadeIn">
        {/* Header */}
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
      </div>
    </div>
  );
}

export default App;
