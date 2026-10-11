//! Top-level canvas compose view: toolbar + stage + layer panel/inspector,
//! plus the window-level drop & paste wiring and keyboard shortcuts.

import { onCleanup, onMount } from "solid-js";
import { useTranslation } from "../../i18n";
import { canvasActions, store } from "../../store/canvasStore";
import { usePasteHandler, useTauriDragDrop } from "../dropInteractions";
import { CanvasStage } from "./CanvasStage";
import { CanvasToolbar } from "./CanvasToolbar";
import { addImagesFromPaths } from "./canvasAdd";
import { LayerInspector } from "./LayerInspector";
import { LayerPanel } from "./LayerPanel";

export function CanvasView() {
  const { t } = useTranslation();

  // Window-level drag & drop: this view mounts only when the canvas tab is
  // active, so the batch view's identical window listener is already torn
  // down — the two never double-fire.
  useTauriDragDrop({
    onDrop: (paths) => {
      void addImagesFromPaths(paths).catch((error) => {
        console.error("Failed to add dropped images:", error);
      });
    },
    onActiveChange: () => {},
  });

  // Ctrl+V pastes clipboard images (temp-parked by the shared hook).
  usePasteHandler({
    enabled: () => store.exporting,
    onPastedPaths: (paths) => {
      void addImagesFromPaths(paths).catch((error) => {
        console.error("Failed to add pasted images:", error);
      });
    },
  });

  // Keyboard shortcuts: delete, nudge, escape. Bound on the window while
  // this view is mounted (the batch view owns no conflicting shortcuts).
  const onKeyDown = (e: KeyboardEvent) => {
    const target = e.target as HTMLElement | null;
    // Don't hijack typing in inputs/selects.
    if (target && ["INPUT", "TEXTAREA", "SELECT"].includes(target.tagName))
      return;
    const selected = store.layers.find((l) => l.id === store.selectedId);
    if ((e.key === "Delete" || e.key === "Backspace") && selected) {
      e.preventDefault();
      canvasActions.removeLayer(selected.id);
      return;
    }
    if (e.key === "Escape") {
      if (store.croppingId) {
        canvasActions.cancelCrop();
      } else if (store.selectionMode) {
        canvasActions.setSelectionMode(false);
      } else if (store.selection) {
        canvasActions.setSelection(null);
      } else {
        canvasActions.selectLayer(null);
      }
      return;
    }
    // Enter commits an in-progress crop.
    if (e.key === "Enter" && store.croppingId) {
      e.preventDefault();
      canvasActions.applyCrop(store.croppingId);
      return;
    }
    // "R" resets the crop frame to the full image (redraw from scratch).
    if ((e.key === "r" || e.key === "R") && store.croppingId) {
      e.preventDefault();
      canvasActions.resetCrop(store.croppingId);
      return;
    }
    if (!selected) return;
    const step = e.shiftKey ? 10 : 1;
    const nudge: Record<string, [number, number]> = {
      ArrowLeft: [-step, 0],
      ArrowRight: [step, 0],
      ArrowUp: [0, -step],
      ArrowDown: [0, step],
    };
    const delta = nudge[e.key];
    if (delta) {
      e.preventDefault();
      canvasActions.updateLayer(selected.id, {
        x: selected.x + delta[0],
        y: selected.y + delta[1],
      });
    }
  };

  onMount(() => {
    window.addEventListener("keydown", onKeyDown);
    onCleanup(() => window.removeEventListener("keydown", onKeyDown));
  });

  // Dev-only inspection hook (CDP remote debugging): drive the add-image
  // pipeline and dump layer/DOM state without touching the native file
  // dialog. Stripped from production builds by the env guard.
  if (import.meta.env.DEV) {
    Object.assign(window as unknown as Record<string, unknown>, {
      __cltCanvas: {
        add: addImagesFromPaths,
        store,
        actions: canvasActions,
        dumpImages: () =>
          [...document.querySelectorAll("img")].map((img) => {
            const cs = getComputedStyle(img);
            const rect = img.getBoundingClientRect();
            return {
              src: img.src.slice(0, 48),
              complete: img.complete,
              natural: `${img.naturalWidth}x${img.naturalHeight}`,
              box: `${Math.round(rect.width)}x${Math.round(rect.height)}@${Math.round(rect.left)},${Math.round(rect.top)}`,
              display: cs.display,
              visibility: cs.visibility,
              opacity: cs.opacity,
            };
          }),
      },
    });
  }

  return (
    <div class="flex h-full min-h-0 flex-col gap-3">
      <CanvasToolbar />

      <div class="flex min-h-0 flex-1 gap-4">
        <CanvasStage />

        {/* Right rail: layers + properties. */}
        <aside class="flex w-64 shrink-0 flex-col gap-4 overflow-y-auto rounded-2xl border border-slate-200 bg-white p-3 dark:border-slate-700 dark:bg-slate-900">
          <LayerPanel />
          <div class="h-px bg-slate-200 dark:bg-slate-700" />
          <LayerInspector />
        </aside>
      </div>

      <p class="shrink-0 text-center text-xs text-slate-400">
        {t("canvas.hints.shortcuts")}
      </p>
    </div>
  );
}
