import { createMemo, For, Show } from "solid-js";
import { useTranslation } from "../i18n";
import { appActions, store } from "../store/useAppStore";
import type { FileStatus } from "../types";
import { STAGGER_DELAY_MS } from "../utils/constants";
import { basename } from "../utils/path";
import {
  AlertCircleIcon,
  CheckCircleIcon,
  ImageIcon,
  SpinnerIcon,
  TrashIcon,
  XIcon,
} from "./Icons";

function StatusIcon(props: { status: FileStatus }) {
  return (
    <Show
      when={props.status === "completed"}
      fallback={
        <Show
          when={props.status === "error"}
          fallback={
            <Show when={props.status === "processing"}>
              <SpinnerIcon class="w-4 h-4 text-indigo-500 animate-spin" />
            </Show>
          }
        >
          <AlertCircleIcon class="w-4 h-4 text-rose-500" />
        </Show>
      }
    >
      <CheckCircleIcon class="w-4 h-4 text-emerald-500" />
    </Show>
  );
}

export function FileList() {
  const { t } = useTranslation();

  const isProcessing = () => store.processingState === "processing";
  // One pass over the list, memoized: during a batch run every per-file
  // status update re-runs this once, instead of two full filter() passes per
  // JSX read (these counts are read from four places in the markup).
  const counts = createMemo(() => {
    let done = 0;
    let err = 0;
    for (const f of store.files) {
      if (f.status === "completed") done++;
      else if (f.status === "error") err++;
    }
    return { done, err };
  });

  return (
    <Show when={store.files.length > 0}>
      <div class="card p-4 animate-slideUp">
        {/* Header */}
        <div class="flex justify-between items-center mb-2.5">
          <div class="flex items-center gap-2">
            <h3 class="font-semibold text-slate-800">{t("files.title")}</h3>
            <span class="px-2.5 py-0.5 bg-indigo-100 text-indigo-700 text-xs font-semibold rounded-full">
              {store.files.length}
            </span>
            <Show when={counts().done > 0}>
              <span class="px-2 py-0.5 bg-emerald-100 text-emerald-700 text-xs font-semibold rounded-full flex items-center gap-1">
                <CheckCircleIcon class="w-3 h-3" />
                {counts().done}
              </span>
            </Show>
            <Show when={counts().err > 0}>
              <span class="px-2 py-0.5 bg-rose-100 text-rose-700 text-xs font-semibold rounded-full flex items-center gap-1">
                <AlertCircleIcon class="w-3 h-3" />
                {counts().err}
              </span>
            </Show>
          </div>
          <button
            type="button"
            onClick={() => appActions.clearFiles()}
            disabled={isProcessing()}
            class={`
              flex items-center gap-1.5 text-sm text-rose-500 hover:text-rose-600
              px-2.5 py-1 rounded-lg hover:bg-rose-50
              transition-colors duration-200
              ${isProcessing() ? "opacity-50 cursor-not-allowed" : ""}
            `}
          >
            <TrashIcon class="w-3.5 h-3.5" />
            {t("files.clear")}
          </button>
        </div>

        {/* File List */}
        <div class="max-h-44 overflow-y-auto space-y-1 custom-scrollbar">
          <For each={store.files}>
            {(file, index) => (
              <div
                class="flex items-center gap-2 py-1.5 px-2.5 rounded-xl text-sm group transition-all duration-200"
                classList={{
                  "bg-emerald-50/80 hover:bg-emerald-100/80":
                    file.status === "completed",
                  "bg-rose-50/80 hover:bg-rose-100/80": file.status === "error",
                  "bg-slate-50/80 hover:bg-slate-100":
                    file.status === "pending" || file.status === "processing",
                  "ring-2 ring-indigo-300": store.selectedPath === file.path,
                }}
                style={{ "animation-delay": `${index() * STAGGER_DELAY_MS}ms` }}
              >
                {/* Status/File icon */}
                <div
                  class="w-7 h-7 rounded-md flex items-center justify-center flex-shrink-0"
                  classList={{
                    "bg-emerald-50": file.status === "completed",
                    "bg-rose-50": file.status === "error",
                    "bg-indigo-50":
                      file.status === "pending" || file.status === "processing",
                  }}
                >
                  <Show
                    when={file.status === "pending"}
                    fallback={<StatusIcon status={file.status} />}
                  >
                    <ImageIcon class="w-4 h-4 text-indigo-500" />
                  </Show>
                </div>

                {/* File info: the name area is the selection control */}
                <button
                  type="button"
                  class="flex-1 min-w-0 text-left cursor-pointer"
                  onClick={() =>
                    appActions.setSelectedPath(
                      store.selectedPath === file.path ? null : file.path,
                    )
                  }
                >
                  <span
                    class={`
                    truncate block font-medium
                    ${file.status === "completed" ? "text-emerald-700" : ""}
                    ${file.status === "error" ? "text-rose-700" : ""}
                    ${
                      file.status === "pending" || file.status === "processing"
                        ? "text-slate-700"
                        : ""
                    }
                  `}
                    title={file.path}
                  >
                    {file.name}
                  </span>
                  <Show
                    when={
                      file.status === "completed" &&
                      !file.skipped &&
                      file.reductionPercent !== undefined
                    }
                  >
                    <span class="text-xs text-emerald-600">
                      → {basename(file.outputPath ?? "")} (
                      {file.reductionPercent?.toFixed(1)}% {t("files.reduced")})
                    </span>
                  </Show>
                  <Show when={file.status === "completed" && file.skipped}>
                    <span class="text-xs text-amber-600">
                      {t("files.skipped")}
                    </span>
                  </Show>
                  <Show when={file.status === "error" && file.error}>
                    <span
                      class="text-xs text-rose-600 truncate block"
                      title={file.error}
                    >
                      {file.error}
                    </span>
                  </Show>
                  {/* Detected real type / extension mismatch */}
                  <Show
                    when={
                      file.matchesExtension === false && file.detectedFormat
                    }
                  >
                    <span
                      class="text-xs text-amber-600 truncate block"
                      title={file.path}
                    >
                      {t("detect.mismatch", { format: file.detectedFormat })}
                    </span>
                  </Show>
                  {/* Output exceeded the requested target size */}
                  <Show when={file.withinTarget === false}>
                    <span
                      class="text-xs text-amber-600 truncate block"
                      title={file.path}
                    >
                      {t("files.overTarget")}
                    </span>
                  </Show>
                </button>

                {/* Remove button - only show for pending files */}
                <Show when={file.status === "pending"}>
                  <button
                    type="button"
                    onClick={() => appActions.removeFile(file.path)}
                    disabled={isProcessing()}
                    class={`
                      p-1 rounded-lg
                      opacity-0 group-hover:opacity-100
                      text-slate-400 hover:text-rose-500 hover:bg-rose-50
                      transition-all duration-200
                      ${isProcessing() ? "opacity-50 cursor-not-allowed" : ""}
                    `}
                    title={t("files.remove")}
                  >
                    <XIcon class="w-4 h-4" />
                  </button>
                </Show>
              </div>
            )}
          </For>
        </div>
      </div>
    </Show>
  );
}
