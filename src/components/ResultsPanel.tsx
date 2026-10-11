import { Show } from "solid-js";
import { useTranslation } from "../i18n";
import { store } from "../store/useAppStore";
import { formatBytes } from "../utils/format";
import { basename } from "../utils/path";
import {
  AlertCircleIcon,
  ArrowRightIcon,
  BarChartIcon,
  ScaleIcon,
  SpinnerIcon,
  TrendingDownIcon,
} from "./Icons";

export function ResultsPanel() {
  const { t } = useTranslation();

  const showProgress = () =>
    store.processingState === "processing" && store.progress !== null;
  const showResults = () =>
    store.processingState === "completed" && store.batchStats !== null;

  return (
    <Show when={showProgress() || showResults()}>
      <div class="card p-4 space-y-4 animate-slideUp">
        <div class="flex items-center gap-2">
          <div class="w-8 h-8 rounded-lg bg-emerald-50 flex items-center justify-center">
            <BarChartIcon class="w-4 h-4 text-emerald-500" />
          </div>
          <h3 class="font-semibold text-slate-800">{t("results.title")}</h3>
        </div>

        {/* Progress Bar */}
        <Show when={showProgress() && store.progress}>
          {(p) => (
            <div class="space-y-3">
              <div class="flex justify-between items-center text-sm">
                <span class="text-slate-600">
                  <span class="font-semibold text-slate-800">
                    {p().current}
                  </span>{" "}
                  {t("results.of")}{" "}
                  <span class="font-semibold text-slate-800">{p().total}</span>{" "}
                  {t("results.files")}
                </span>
                <span class="font-bold text-indigo-600 bg-indigo-50 px-2 py-0.5 rounded-md">
                  {p().percent.toFixed(0)}%
                </span>
              </div>

              {/* Animated progress bar */}
              <div class="progress-bar-track h-3">
                <div
                  class="progress-bar-fill h-3"
                  style={{ width: `${p().percent}%` }}
                />
              </div>

              <div class="flex items-center gap-2 text-xs text-slate-500">
                <SpinnerIcon class="w-3 h-3 animate-spin text-indigo-500" />
                <span class="truncate" title={p().current_file}>
                  {basename(p().current_file)}
                </span>
              </div>
            </div>
          )}
        </Show>

        {/* Results Statistics */}
        <Show when={showResults() && store.batchStats}>
          {(b) => (
            <div class="space-y-4">
              {/* File count */}
              <div class="flex justify-between items-center text-sm bg-slate-50 rounded-xl px-4 py-2.5">
                <span class="text-slate-600">{t("results.processed")}</span>
                <span class="font-semibold text-slate-800">
                  {b().successful_files} / {b().total_files}{" "}
                  {t("results.files")}
                </span>
              </div>

              {/* Statistics cards */}
              <div class="grid grid-cols-3 gap-3">
                {/* Overall */}
                <div class="bg-emerald-50/70 rounded-xl p-3 text-center border border-emerald-100/50 shadow-sm">
                  <TrendingDownIcon class="w-5 h-5 text-emerald-500 mx-auto mb-1.5" />
                  <div class="text-xl font-bold text-emerald-600">
                    {b().overall_reduction_percent.toFixed(1)}%
                  </div>
                  <div class="text-xs text-emerald-600/70 font-medium">
                    {t("results.overall")}
                  </div>
                </div>

                {/* Average */}
                <div class="bg-blue-50/70 rounded-xl p-3 text-center border border-blue-100/50 shadow-sm">
                  <BarChartIcon class="w-5 h-5 text-blue-500 mx-auto mb-1.5" />
                  <div class="text-xl font-bold text-blue-600">
                    {b().average_reduction_percent.toFixed(1)}%
                  </div>
                  <div class="text-xs text-blue-600/70 font-medium">
                    {t("results.average")}
                  </div>
                </div>

                {/* Median */}
                <div class="bg-violet-50/70 rounded-xl p-3 text-center border border-violet-100/50 shadow-sm">
                  <ScaleIcon class="w-5 h-5 text-violet-500 mx-auto mb-1.5" />
                  <div class="text-xl font-bold text-violet-600">
                    {b().median_reduction_percent.toFixed(1)}%
                  </div>
                  <div class="text-xs text-violet-600/70 font-medium">
                    {t("results.median")}
                  </div>
                </div>
              </div>

              {/* Size comparison */}
              <div class="flex items-center justify-center gap-3 text-sm text-slate-500 bg-slate-50/50 rounded-xl py-2">
                <span class="font-medium">
                  {formatBytes(b().total_original_size)}
                </span>
                <ArrowRightIcon class="w-4 h-4 text-slate-400" />
                <span class="font-semibold text-emerald-600">
                  {formatBytes(b().total_output_size)}
                </span>
              </div>

              {/* Error count */}
              <Show when={b().failed_files > 0}>
                <div class="flex items-center gap-2 text-sm text-rose-600 bg-rose-50 rounded-xl px-4 py-2.5">
                  <AlertCircleIcon class="w-4 h-4" />
                  <span>{t("results.failed", { n: b().failed_files })}</span>
                </div>
              </Show>
            </div>
          )}
        </Show>
      </div>
    </Show>
  );
}
