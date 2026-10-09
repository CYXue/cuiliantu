import { getName } from "@tauri-apps/api/app";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import {
  isPermissionGranted,
  requestPermission,
  sendNotification,
} from "@tauri-apps/plugin-notification";
import { Show } from "solid-js";
import { useTranslation } from "../i18n";
import { appActions, store } from "../store/useAppStore";
import type { BatchStats, ProcessingResult, ProgressUpdate } from "../types";
import { sanitizeError } from "../utils/sanitize";
import { PlayIcon, SpinnerIcon, XIcon } from "./Icons";

export function ActionButtons() {
  const { t } = useTranslation();

  const isProcessing = () => store.processingState === "processing";
  const canStart = () =>
    store.files.length > 0 && store.outputDir !== "" && !isProcessing();

  // Monotonic per-run id. Each start (and each cancel) bumps it; handlers
  // remember the id they began with and stop touching the store once the id
  // has moved on ("this is no longer my run"). That keeps late events and
  // late return values from a cancelled run — or from the next run — from
  // overwriting current state. A Solid component body runs only once, so a
  // plain `let` works as the ref.
  let runId = 0;
  // Unsubscribe for the current run. Called from both cancel and the
  // `finally` in handleStart, so it must be safe to call twice.
  let unsubscribeRef: (() => void) | null = null;

  // Send desktop notification
  const sendCompletionNotification = async (stats: BatchStats) => {
    try {
      let permissionGranted = await isPermissionGranted();
      if (!permissionGranted) {
        const permission = await requestPermission();
        permissionGranted = permission === "granted";
      }

      if (permissionGranted) {
        const body = t("notification.body", {
          count: stats.successful_files,
          reduction: stats.overall_reduction_percent.toFixed(1),
        });
        // Read the product name from the app config so it always matches
        // `tauri.conf.json` (no hardcoded title to keep in sync on rename).
        const appName = await getName().catch(() => "CuiLianTu");
        sendNotification({
          title: appName,
          body,
        });
      }
    } catch (error) {
      console.error("Failed to send notification:", error);
    }
  };

  const handleStart = async () => {
    if (!canStart()) {
      if (store.files.length === 0) {
        appActions.setError(t("errors.noFiles"));
      } else if (store.outputDir === "") {
        appActions.setError(t("errors.noOutputDir"));
      }
      return;
    }

    runId += 1;
    const myRunId = runId;
    // Whether this run is still "the current run". Becomes false on cancel
    // or when a newer run starts.
    const isCurrentRun = () => runId === myRunId;

    appActions.setProcessingState("processing");
    appActions.setError(null);
    appActions.setBatchStats(null);
    appActions.resetFileStatuses();

    // Listen for progress updates
    const unlistenProgress = await listen<ProgressUpdate>(
      "processing-progress",
      (event) => {
        if (!isCurrentRun()) return;
        appActions.setProgress(event.payload);
        // Update current file status to processing
        appActions.updateFileStatus(event.payload.current_file, "processing");
      },
    );

    // Listen for individual file results
    const unlistenResult = await listen<ProcessingResult>(
      "processing-result",
      (event) => {
        if (!isCurrentRun()) return;
        const result = event.payload;
        if (result.success) {
          appActions.updateFileStatus(result.original_path, "completed", {
            outputPath: result.output_path,
            outputSize: result.output_size,
            reductionPercent: result.reduction_percent,
            skipped: result.skipped,
            withinTarget: result.within_target,
          });
        } else {
          appActions.updateFileStatus(result.original_path, "error", {
            // Strip absolute paths the same way the top-level batch error is
            // sanitized: per-file rows must not leak the directory tree either.
            error: sanitizeError(result.error || t("errors.unknown")),
          });
        }
      },
    );

    // Completion is applied exactly once: whichever arrives first — the
    // `processing-complete` event or the `process_batch` return value — wins.
    let completionHandled = false;
    const handleCompletion = (stats: BatchStats) => {
      // Drop completions from a cancelled run or after a newer run started.
      if (!isCurrentRun()) return;
      if (completionHandled) return;
      completionHandled = true;
      appActions.setBatchStats(stats);
      appActions.setProcessingState("completed");
      // Send desktop notification
      void sendCompletionNotification(stats);
    };

    // Listen for completion
    const unlistenComplete = await listen<BatchStats>(
      "processing-complete",
      (event) => {
        handleCompletion(event.payload);
      },
    );

    // Unsubscribe can be called from both cancel and `finally`; guard so the
    // second call is a no-op.
    let unsubscribed = false;
    const unsubscribe = () => {
      if (unsubscribed) return;
      unsubscribed = true;
      unlistenProgress();
      unlistenResult();
      unlistenComplete();
    };
    unsubscribeRef = unsubscribe;

    try {
      const inputPaths = store.files.map((f) => f.path);
      const stats = await invoke<BatchStats>("process_batch", {
        inputPaths,
        outputDir: store.outputDir,
        options: store.options,
      });
      // Complete via the return value even when the events never arrive.
      handleCompletion(stats);
    } catch (error) {
      console.error("Processing failed:", error);
      // A failure arriving after cancel must not touch the current display.
      if (isCurrentRun()) {
        // Strip absolute paths from the backend error before showing it: the
        // app is private-by-design and must not leak the user's directory
        // structure into the UI.
        appActions.setError(
          `${t("errors.processingFailed")}: ${sanitizeError(String(error))}`,
        );
        appActions.setProcessingState("error");
      }
    } finally {
      unsubscribe();
      if (isCurrentRun()) {
        appActions.setProgress(null);
        unsubscribeRef = null;
      }
    }
  };

  // Cancel interrupts the display AND tells the backend to stop. The Rust
  // batch checks the cancellation epoch per file, so files not yet started
  // are never written (the one file in flight runs to completion).
  const handleCancel = () => {
    runId += 1;
    unsubscribeRef?.();
    unsubscribeRef = null;
    void invoke("cancel_processing").catch((err) => {
      // The visual cancel stands even if the backend is unreachable; log the
      // failure so a real breakage is never fully invisible.
      console.debug("cancel_processing failed:", err);
    });
    appActions.setProcessingState("idle");
    appActions.setProgress(null);
  };

  return (
    <div class="flex gap-4 pt-2">
      <button
        type="button"
        onClick={() => void handleStart()}
        disabled={!canStart()}
        class={`
          flex-1 py-3.5 px-6 rounded-xl font-semibold text-white
          flex items-center justify-center gap-2.5
          transition-all duration-200 ease-out
          ${
            canStart()
              ? "bg-gradient-to-r from-[#5b5bd6] to-[#6d56e0] hover:from-[#4f4fc8] hover:to-[#5d48d2] shadow-md shadow-indigo-500/20 hover:shadow-lg hover:shadow-indigo-500/25 hover:scale-[1.01] active:scale-[0.99]"
              : "bg-slate-200 text-slate-400 cursor-not-allowed shadow-none"
          }
        `}
      >
        <Show
          when={!isProcessing()}
          fallback={
            <>
              <SpinnerIcon class="w-5 h-5 animate-spin" />
              <span>{t("actions.processing")}</span>
            </>
          }
        >
          <PlayIcon class="w-5 h-5" />
          <span>{t("actions.start")}</span>
        </Show>
      </button>

      <Show when={isProcessing()}>
        <button
          type="button"
          onClick={handleCancel}
          class="
            flex items-center gap-2 px-6 py-3.5 rounded-xl font-semibold text-white
            bg-gradient-to-r from-rose-500 to-rose-600
            hover:from-rose-600 hover:to-rose-700
            shadow-md shadow-rose-500/20 hover:shadow-lg hover:shadow-rose-500/25
            transition-all duration-200 ease-out
            hover:scale-[1.01] active:scale-[0.99]
            animate-fadeIn
          "
        >
          <XIcon class="w-5 h-5" />
          <span>{t("actions.cancel")}</span>
        </button>
      </Show>
    </div>
  );
}
