import { invoke } from "@tauri-apps/api/core";
import { openUrl } from "@tauri-apps/plugin-opener";
import { createSignal, onMount, Show } from "solid-js";
import { useTranslation } from "../i18n";
import { AlertCircleIcon, DownloadIcon, XIcon } from "./Icons";

interface UpdateInfo {
  update_available: boolean;
  current_version: string;
  latest_version: string;
  release_url: string;
}

export function UpdateNotification() {
  const { t } = useTranslation();
  const [updateInfo, setUpdateInfo] = createSignal<UpdateInfo | null>(null);
  const [dismissed, setDismissed] = createSignal(false);
  const [checking, setChecking] = createSignal(true);
  const [failed, setFailed] = createSignal(false);

  onMount(() => {
    const checkUpdate = async () => {
      try {
        const info = await invoke<UpdateInfo>("check_for_updates");
        setUpdateInfo(info);
      } catch (error) {
        // The backend returns Err for both network failures and 403s. Swallow
        // the error here and the user cannot tell "no updates" from "the
        // check is broken", so surface it.
        console.error("Failed to check for updates:", error);
        setFailed(true);
      } finally {
        setChecking(false);
      }
    };

    void checkUpdate();
  });

  const handleDownload = async () => {
    const url = updateInfo()?.release_url;
    // The backend already constrains the URL to https://github.com/, but the
    // frontend opens it with a system handler, so reject anything that is not
    // a https link as a second line of defense.
    if (url?.startsWith("https://")) {
      await openUrl(url);
    }
  };

  const handleDismiss = () => {
    setDismissed(true);
  };

  return (
    <Show when={!checking() && !dismissed()}>
      <Show
        when={!failed()}
        fallback={
          <div class="bg-amber-50 border border-amber-200 text-amber-800 px-4 py-3 rounded-xl shadow-sm flex items-center gap-3 animate-fadeIn">
            <AlertCircleIcon class="w-5 h-5 text-amber-500 flex-shrink-0" />
            <span class="flex-1 text-sm">{t("update.checkFailed")}</span>
            <button
              type="button"
              onClick={handleDismiss}
              class="p-1.5 hover:bg-amber-100 rounded-lg transition-colors"
              aria-label={t("update.dismiss")}
            >
              <XIcon class="w-4 h-4" />
            </button>
          </div>
        }
      >
        <Show when={updateInfo()?.update_available}>
          <div class="bg-gradient-to-r from-indigo-50 to-violet-50 border border-indigo-200 text-indigo-800 px-4 py-3 rounded-xl shadow-sm animate-fadeIn">
            <div class="flex items-start gap-3">
              <div class="p-2 bg-indigo-100 rounded-lg">
                <DownloadIcon class="w-5 h-5 text-indigo-600" />
              </div>
              <div class="flex-1 min-w-0">
                <div class="flex items-center gap-2 flex-wrap">
                  <span class="font-medium text-sm">
                    {t("update.available")}
                  </span>
                  <span class="text-xs bg-indigo-100 text-indigo-700 px-2 py-0.5 rounded-full">
                    v{updateInfo()?.current_version} → v
                    {updateInfo()?.latest_version}
                  </span>
                </div>
                <p class="text-xs text-indigo-600 mt-1">
                  {t("update.description")}
                </p>
              </div>
              <div class="flex items-center gap-2 flex-shrink-0">
                <button
                  type="button"
                  onClick={handleDownload}
                  class="px-3 py-1.5 bg-indigo-600 hover:bg-indigo-700 text-white text-xs font-medium rounded-lg transition-colors dark:bg-indigo-500 dark:hover:bg-indigo-400"
                >
                  {t("update.download")}
                </button>
                <button
                  type="button"
                  onClick={handleDismiss}
                  class="p-1.5 hover:bg-indigo-100 rounded-lg transition-colors"
                  aria-label={t("update.dismiss")}
                >
                  <XIcon class="w-4 h-4 text-indigo-500" />
                </button>
              </div>
            </div>
          </div>
        </Show>
      </Show>
    </Show>
  );
}
