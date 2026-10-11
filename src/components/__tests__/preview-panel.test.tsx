// PreviewPanel race-condition tests.
//
// The panel serializes every options change into a preview request and only
// lets the NEWEST request write results (a monotonic `seq` guard). That guard
// is the most intricate untestable-until-now logic in the frontend: a slow
// older encode that lands after a newer one must never overwrite (or clear)
// the fresher preview, and its failure must not flag the panel as failed.
// These tests drive the real component with controllable invoke promises.
import { render, waitFor } from "@solidjs/testing-library";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { appActions, store } from "../../store/useAppStore";
import { PreviewPanel } from "../PreviewPanel";

const invokeMock = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
}));

interface Deferred {
  resolve: (value: unknown) => void;
  reject: (reason?: unknown) => void;
}

beforeEach(() => {
  invokeMock.mockReset();
  appActions.clearFiles();
  appActions.setSelectedPath(null);
  appActions.setError(null);
  // Reset the option the race tests mutate, so test order can never make a
  // "change" a no-op (equal snapshots don't re-trigger the preview effect).
  appActions.setOptions({ quality: 80 });
});

afterEach(() => {
  vi.restoreAllMocks();
});

/** One pending `preview_image` call per invocation, resolved manually. */
function deferPreviewInvocations() {
  const pending: Deferred[] = [];
  invokeMock.mockImplementation((cmd: string) => {
    if (cmd === "preview_image")
      return new Promise((resolve, reject) => {
        pending.push({ resolve, reject });
      });
    return Promise.reject(new Error(`unexpected command: ${cmd}`));
  });
  return pending;
}

const PREVIEW_PAYLOAD = (tag: string) => ({
  before_base64: `BEFORE_${tag}`,
  after_base64: `AFTER_${tag}`,
  before_size: 100,
  after_size: 50,
  width: 8,
  height: 8,
});

describe("PreviewPanel preview races", () => {
  it("a slow older preview never overwrites a newer one", async () => {
    const pending = deferPreviewInvocations();

    appActions.addFiles([
      { path: "D:/pics/a.png", name: "a.png", size: 100, status: "pending" },
    ]);
    appActions.setSelectedPath("D:/pics/a.png");

    render(() => <PreviewPanel />);

    // Mount fires request #1 (after the 300ms debounce).
    await waitFor(() => expect(pending.length).toBe(1), { timeout: 3000 });
    // An options change fires request #2.
    appActions.setOptions({ quality: 42 });
    await waitFor(() => expect(pending.length).toBe(2), { timeout: 3000 });

    // The NEWER request finishes first.
    pending[1]?.resolve(PREVIEW_PAYLOAD("NEW"));
    const afterImg = await waitFor(
      () => {
        const imgs = Array.from(document.querySelectorAll("img"));
        const img = imgs.find((i) =>
          i.getAttribute("src")?.includes("AFTER_NEW"),
        );
        expect(img).toBeTruthy();
        return img as HTMLImageElement;
      },
      { timeout: 3000 },
    );

    // The OLDER request lands late: the seq guard must drop it entirely.
    pending[0]?.resolve(PREVIEW_PAYLOAD("OLD"));
    await new Promise((r) => setTimeout(r, 50));
    expect(afterImg.getAttribute("src")).toContain("AFTER_NEW");
    expect(document.body.textContent).not.toContain("BEFORE_OLD");
  });

  it("a stale preview failure does not flag the panel as failed", async () => {
    const pending = deferPreviewInvocations();

    appActions.addFiles([
      { path: "D:/pics/a.png", name: "a.png", size: 100, status: "pending" },
    ]);
    appActions.setSelectedPath("D:/pics/a.png");

    const consoleError = vi
      .spyOn(console, "error")
      .mockImplementation(() => {});
    render(() => <PreviewPanel />);

    await waitFor(() => expect(pending.length).toBe(1), { timeout: 3000 });
    appActions.setOptions({ quality: 42 });
    await waitFor(() => expect(pending.length).toBe(2), { timeout: 3000 });

    // Newer succeeds; the older request then fails out of order.
    pending[1]?.resolve(PREVIEW_PAYLOAD("NEW"));
    await waitFor(
      () =>
        Array.from(document.querySelectorAll("img")).some((i) =>
          i.getAttribute("src")?.includes("AFTER_NEW"),
        ),
      { timeout: 3000 },
    );
    pending[0]?.reject(new Error("encoder died"));
    await new Promise((r) => setTimeout(r, 50));

    // The panel must still show the fresh preview — no failure banner.
    expect(document.body.textContent).not.toContain("Preview failed");
    expect(
      Array.from(document.querySelectorAll("img")).some((i) =>
        i.getAttribute("src")?.includes("AFTER_NEW"),
      ),
    ).toBe(true);
    consoleError.mockRestore();
  });

  it("shows the failure banner when the CURRENT request fails", async () => {
    // Inverse direction of the guard: a failure from the newest request must
    // surface (the guard must not swallow legitimate errors).
    const pending = deferPreviewInvocations();

    appActions.addFiles([
      { path: "D:/pics/a.png", name: "a.png", size: 100, status: "pending" },
    ]);
    appActions.setSelectedPath("D:/pics/a.png");

    const consoleError = vi
      .spyOn(console, "error")
      .mockImplementation(() => {});
    render(() => <PreviewPanel />);

    await waitFor(() => expect(pending.length).toBe(1), { timeout: 3000 });
    pending[0]?.reject(new Error("decode failed"));

    await waitFor(
      () => expect(document.body.textContent).toContain("Preview failed"),
      { timeout: 3000 },
    );
    consoleError.mockRestore();
  });

  it("requests the preview through the path command for a selected file", async () => {
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "preview_image") return Promise.resolve(PREVIEW_PAYLOAD("X"));
      return Promise.reject(new Error(`unexpected command: ${cmd}`));
    });
    appActions.addFiles([
      { path: "D:/pics/a.png", name: "a.png", size: 100, status: "pending" },
    ]);
    appActions.setSelectedPath("D:/pics/a.png");

    render(() => <PreviewPanel />);
    await waitFor(() => expect(invokeMock).toHaveBeenCalled(), {
      timeout: 3000,
    });
    expect(invokeMock).toHaveBeenCalledWith("preview_image", {
      path: "D:/pics/a.png",
      options: store.options,
    });
  });
});
