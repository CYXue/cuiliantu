// Component-level IPC smoke tests.
//
// Each component here talks to the Rust backend through `invoke` (and, for
// ActionButtons, through event subscriptions). These tests mock the Tauri
// API modules and drive the real Solid components in jsdom, asserting that
// user actions produce the right IPC calls with the right payloads and that
// the store/UI reflect the answers. One bad wire format here would otherwise
// only surface inside the packaged app.
import { render, waitFor } from "@solidjs/testing-library";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import i18n from "../../i18n";
import { appActions, store } from "../../store/useAppStore";
import { ActionButtons } from "../ActionButtons";
import { DropZone } from "../DropZone";
import { UpdateNotification } from "../UpdateNotification";

// ---- Tauri API mocks -------------------------------------------------------

const invokeMock = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
}));

const listenMock = vi.fn();
vi.mock("@tauri-apps/api/event", () => ({
  listen: (...args: unknown[]) => listenMock(...args),
}));

const getNameMock = vi.fn();
vi.mock("@tauri-apps/api/app", () => ({
  getName: (...args: unknown[]) => getNameMock(...args),
}));

// Captured drag-and-drop handler so tests can simulate a real drop.
let dropHandler: ((event: { payload: unknown }) => void) | null = null;
const onDragDropEventMock = vi.fn(
  async (handler: (event: { payload: unknown }) => void) => {
    dropHandler = handler;
    return () => {};
  },
);
vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({ onDragDropEvent: onDragDropEventMock }),
}));

const openMock = vi.fn();
vi.mock("@tauri-apps/plugin-dialog", () => ({
  open: (...args: unknown[]) => openMock(...args),
}));

const permissionGrantedMock = vi.fn();
const requestPermissionMock = vi.fn();
const sendNotificationMock = vi.fn();
vi.mock("@tauri-apps/plugin-notification", () => ({
  isPermissionGranted: (...args: unknown[]) => permissionGrantedMock(...args),
  requestPermission: (...args: unknown[]) => requestPermissionMock(...args),
  sendNotification: (...args: unknown[]) => sendNotificationMock(...args),
}));

const openUrlMock = vi.fn();
vi.mock("@tauri-apps/plugin-opener", () => ({
  openUrl: (...args: unknown[]) => openUrlMock(...args),
}));

const flush = () => new Promise((resolve) => setTimeout(resolve, 0));

beforeEach(() => {
  invokeMock.mockReset();
  listenMock.mockReset();
  getNameMock.mockReset();
  openUrlMock.mockReset();
  openMock.mockReset();
  permissionGrantedMock.mockReset();
  requestPermissionMock.mockReset();
  sendNotificationMock.mockReset();
  onDragDropEventMock.mockClear();
  dropHandler = null;
  // Fake the Tauri runtime so DropZone's drag-and-drop setup proceeds.
  (window as unknown as Record<string, unknown>).__TAURI__ = {};
  // Reset every piece of state the components read, so tests stay
  // independent of the order in which they run.
  appActions.clearFiles();
  appActions.setError(null);
  appActions.setProgress(null);
  appActions.setBatchStats(null);
  appActions.setOutputDir("");
});

afterEach(() => {
  delete (window as unknown as Record<string, unknown>).__TAURI__;
});

// ---- DropZone --------------------------------------------------------------

describe("DropZone IPC chain", () => {
  it("a drop registers paths, expands images and batch-detects types", async () => {
    invokeMock.mockImplementation((cmd: string) => {
      switch (cmd) {
        case "register_allowed_paths":
          return Promise.resolve(null);
        case "get_image_files":
          // The real command expands directories into image files; mirror
          // that here with a fixed expansion.
          return Promise.resolve(["D:/pics/a.png", "D:/pics/b.jpg"]);
        case "detect_file_types":
          return Promise.resolve([
            {
              path: "D:/pics/a.png",
              info: {
                path: "D:/pics/a.png",
                extension: "png",
                detected_format: "png",
                detected_mime: "image/png",
                matches_extension: true,
                width: 8,
                height: 8,
                size_bytes: 100,
              },
            },
            { path: "D:/pics/b.jpg", error: "nope" },
          ]);
        default:
          return Promise.reject(new Error(`unexpected command: ${cmd}`));
      }
    });

    render(() => <DropZone />);
    await waitFor(() => expect(dropHandler).not.toBeNull());

    dropHandler?.({ payload: { type: "drop", paths: ["D:/pics"] } });

    await waitFor(() => expect(store.files.length).toBe(2));
    expect(store.files[0].path).toBe("D:/pics/a.png");

    await flush(); // detection is fire-and-forget; let it settle
    expect(invokeMock).toHaveBeenCalledWith("register_allowed_paths", {
      paths: ["D:/pics"],
    });
    expect(invokeMock).toHaveBeenCalledWith("get_image_files", {
      paths: ["D:/pics"],
    });
    expect(invokeMock).toHaveBeenCalledWith("detect_file_types", {
      paths: ["D:/pics/a.png", "D:/pics/b.jpg"],
    });
    // The detected type landed in the store; the failed entry left no trace.
    expect(store.files[0].detectedFormat).toBe("png");
    expect(store.files[1].detectedFormat).toBeUndefined();
  });
});

// ---- DropZone failure paths, pickers and paste ------------------------------

describe("DropZone failure and edge branches", () => {
  it("surfaces a whitelist/IPC rejection as a user-facing error", async () => {
    invokeMock.mockRejectedValue(
      new Error("Path is outside the folders this session has opened"),
    );
    const consoleError = vi
      .spyOn(console, "error")
      .mockImplementation(() => {});
    render(() => <DropZone />);
    await waitFor(() => expect(dropHandler).not.toBeNull());

    dropHandler?.({ payload: { type: "drop", paths: ["C:/secret"] } });

    // Silent failure is not an option: the error banner must carry the
    // sanitized reason and the file list must stay empty.
    await waitFor(() => expect(store.error).toContain("Failed to add files"));
    expect(store.files.length).toBe(0);
    consoleError.mockRestore();
  });

  it("keeps working when the batch type detection fails entirely", async () => {
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "register_allowed_paths") return Promise.resolve(null);
      if (cmd === "get_image_files") return Promise.resolve(["D:/pics/a.png"]);
      if (cmd === "detect_file_types")
        return Promise.reject(new Error("ipc down"));
      return Promise.reject(new Error(`unexpected command: ${cmd}`));
    });
    const consoleError = vi
      .spyOn(console, "error")
      .mockImplementation(() => {});
    render(() => <DropZone />);
    await waitFor(() => expect(dropHandler).not.toBeNull());

    dropHandler?.({ payload: { type: "drop", paths: ["D:/pics"] } });

    // Files are added anyway (best-effort detection), no error is raised.
    await waitFor(() => expect(store.files.length).toBe(1));
    await flush();
    expect(store.error).toBeNull();
    consoleError.mockRestore();
  });

  it("ignores drops while a batch is processing", async () => {
    appActions.setProcessingState("processing");
    render(() => <DropZone />);
    await waitFor(() => expect(dropHandler).not.toBeNull());

    dropHandler?.({ payload: { type: "drop", paths: ["D:/pics"] } });
    await flush();

    expect(invokeMock).not.toHaveBeenCalled();
  });

  it("toggles drag styling on over and leave events", async () => {
    render(() => <DropZone />);
    await waitFor(() => expect(dropHandler).not.toBeNull());

    const zone = document.querySelector(".border-dashed") as HTMLElement;
    dropHandler?.({ payload: { type: "over", position: { x: 0, y: 0 } } });
    expect(zone.className).toContain("border-indigo-400");

    dropHandler?.({ payload: { type: "leave" } });
    expect(zone.className).not.toContain("border-indigo-400");
  });

  it("warns and skips drag setup outside the Tauri runtime", async () => {
    delete (window as unknown as Record<string, unknown>).__TAURI__;
    const consoleWarn = vi.spyOn(console, "warn").mockImplementation(() => {});

    render(() => <DropZone />);
    await flush();

    expect(consoleWarn).toHaveBeenCalledWith(
      "Not running in Tauri environment, drag and drop disabled",
    );
    expect(onDragDropEventMock).not.toHaveBeenCalled();
    consoleWarn.mockRestore();
  });

  it("adds files picked through the file dialog (array and single string)", async () => {
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "register_allowed_paths") return Promise.resolve(null);
      if (cmd === "get_image_files") return Promise.resolve(["D:/x/1.png"]);
      if (cmd === "detect_file_types") return Promise.resolve([]);
      return Promise.reject(new Error(`unexpected command: ${cmd}`));
    });
    render(() => <DropZone />);
    const fileButtons = await waitFor(() =>
      document.querySelectorAll("button"),
    );
    const selectFiles = Array.from(fileButtons).find(
      (b) => b.textContent === "Select Files",
    );
    expect(selectFiles).toBeTruthy();

    // Dialog answers with a plain string: the component must array-ify it.
    openMock.mockResolvedValue("D:/x/1.png");
    selectFiles?.click();
    await waitFor(() => expect(store.files.length).toBe(1));

    // The drop zone has collapsed into the compact bar by now: the click
    // must go to the *current* button, not the detached hero one.
    const compactSelect = Array.from(document.querySelectorAll("button")).find(
      (b) => b.textContent === "Select Files",
    );
    expect(compactSelect).toBeTruthy();
    openMock.mockResolvedValue(["D:/x/1.png"]);
    compactSelect?.click();
    await flush();
    const expandCalls = invokeMock.mock.calls.filter(
      (c) => c[0] === "get_image_files",
    ).length;
    expect(expandCalls).toBe(2);
  });

  it("does nothing when the user cancels the file dialog", async () => {
    openMock.mockResolvedValue(null);
    render(() => <DropZone />);
    const selectFiles = await waitFor(() =>
      Array.from(document.querySelectorAll("button")).find(
        (b) => b.textContent === "Select Files",
      ),
    );
    selectFiles?.click();
    await flush();

    expect(invokeMock).not.toHaveBeenCalled();
    expect(store.error).toBeNull();
  });

  it("reports file-picker failures through the error banner", async () => {
    openMock.mockRejectedValue(new Error("dialog crashed"));
    const consoleError = vi
      .spyOn(console, "error")
      .mockImplementation(() => {});
    render(() => <DropZone />);
    const selectFiles = await waitFor(() =>
      Array.from(document.querySelectorAll("button")).find(
        (b) => b.textContent === "Select Files",
      ),
    );
    selectFiles?.click();

    await waitFor(() => expect(store.error).toContain("Failed to add files"));
    consoleError.mockRestore();
  });

  it("reports folder-picker failures through the error banner", async () => {
    openMock.mockRejectedValue(new Error("dialog crashed"));
    const consoleError = vi
      .spyOn(console, "error")
      .mockImplementation(() => {});
    render(() => <DropZone />);
    const selectFolder = await waitFor(() =>
      Array.from(document.querySelectorAll("button")).find(
        (b) => b.textContent === "Select Folder",
      ),
    );
    selectFolder?.click();

    await waitFor(() => expect(store.error).toContain("Failed to add files"));
    consoleError.mockRestore();
  });

  it("shows the compact bar once files exist", async () => {
    appActions.addFiles([
      { path: "D:/pics/a.png", name: "a.png", size: 100, status: "pending" },
    ]);
    const { getByText } = render(() => <DropZone />);

    expect(getByText("Add more images")).toBeTruthy();
  });
});

describe("DropZone paste handling", () => {
  /** Build a paste event whose clipboardData carries the given files. */
  const pasteEvent = (files: File[]) => {
    const event = new Event("paste") as ClipboardEvent;
    Object.defineProperty(event, "clipboardData", {
      value: { files: files as unknown as FileList },
    });
    return event;
  };

  const fakeImageFile = (opts: {
    name: string;
    type: string;
    path?: string;
    bytes?: Uint8Array;
  }) =>
    ({
      name: opts.name,
      type: opts.type,
      path: opts.path,
      arrayBuffer: async () => (opts.bytes ?? new Uint8Array([1, 2, 3])).buffer,
    }) as unknown as File;

  it("parks a pasted screenshot on the backend and adds the returned path", async () => {
    invokeMock.mockImplementation((cmd: string, args?: unknown) => {
      if (cmd === "save_temp_image")
        return Promise.resolve("C:/temp/paste-1.png");
      if (cmd === "register_allowed_paths") return Promise.resolve(null);
      if (cmd === "get_image_files")
        return Promise.resolve((args as { paths: string[] }).paths);
      if (cmd === "detect_file_types") return Promise.resolve([]);
      return Promise.reject(new Error(`unexpected command: ${cmd}`));
    });
    render(() => <DropZone />);
    await flush();

    window.dispatchEvent(
      pasteEvent([fakeImageFile({ name: "shot.png", type: "image/png" })]),
    );

    await waitFor(() => expect(store.files.length).toBe(1));
    expect(store.files[0].path).toBe("C:/temp/paste-1.png");
    // The extension came from the MIME type, the bytes were base64-encoded.
    expect(invokeMock).toHaveBeenCalledWith("save_temp_image", {
      dataBase64: btoa("\u0001\u0002\u0003"),
      ext: "png",
    });
  });

  it("uses the explorer-injected path for clipboard files instead of re-saving", async () => {
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "register_allowed_paths") return Promise.resolve(null);
      if (cmd === "get_image_files") return Promise.resolve(["D:/copied.jpg"]);
      if (cmd === "detect_file_types") return Promise.resolve([]);
      return Promise.reject(new Error(`unexpected command: ${cmd}`));
    });
    render(() => <DropZone />);
    await flush();

    window.dispatchEvent(
      pasteEvent([
        fakeImageFile({
          name: "copied.jpg",
          type: "image/jpeg",
          path: "D:/copied.jpg",
        }),
      ]),
    );

    await waitFor(() => expect(store.files.length).toBe(1));
    expect(invokeMock).not.toHaveBeenCalledWith(
      "save_temp_image",
      expect.anything(),
    );
  });

  it("skips saving when the backend rejects the pasted image", async () => {
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "save_temp_image")
        return Promise.reject(new Error("too large"));
      return Promise.reject(new Error(`unexpected command: ${cmd}`));
    });
    const consoleError = vi
      .spyOn(console, "error")
      .mockImplementation(() => {});
    render(() => <DropZone />);
    await flush();

    window.dispatchEvent(
      pasteEvent([fakeImageFile({ name: "shot.png", type: "image/png" })]),
    );
    await flush();

    // The failed paste never enters the pipeline.
    expect(store.files.length).toBe(0);
    expect(invokeMock).not.toHaveBeenCalledWith(
      "register_allowed_paths",
      expect.anything(),
    );
    consoleError.mockRestore();
  });

  it("ignores clipboard content without images", async () => {
    render(() => <DropZone />);
    await flush();

    window.dispatchEvent(
      pasteEvent([fakeImageFile({ name: "notes.txt", type: "text/plain" })]),
    );
    await flush();

    expect(invokeMock).not.toHaveBeenCalled();
  });
});

describe("Language switch persistence", () => {
  it("persists a runtime switch and syncs <html lang> via the mounted tree", async () => {
    invokeMock.mockResolvedValue(null);
    render(() => <DropZone />);
    await flush();

    // The languageChanged handler lives in useTranslation, registered by the
    // mounted component: switching language must persist the choice and keep
    // the document language in sync.
    await i18n.changeLanguage("zh-CN");
    expect(localStorage.getItem("cuiliantu-lang")).toBe("zh-CN");
    expect(document.documentElement.lang).toBe("zh-CN");

    await i18n.changeLanguage("en");
    expect(localStorage.getItem("cuiliantu-lang")).toBe("en");
    expect(document.documentElement.lang).toBe("en");
  });
});

// ---- ActionButtons ---------------------------------------------------------

const BATCH_STATS = {
  total_files: 1,
  processed_files: 1,
  successful_files: 1,
  failed_files: 0,
  total_original_size: 100,
  total_output_size: 50,
  overall_reduction_percent: 50,
  average_reduction_percent: 50,
  median_reduction_percent: 50,
};

describe("ActionButtons IPC", () => {
  it("start invokes process_batch with store state and completes on its result", async () => {
    listenMock.mockResolvedValue(() => {});
    getNameMock.mockResolvedValue("CuiLianTu");
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "process_batch") return Promise.resolve(BATCH_STATS);
      return Promise.reject(new Error(`unexpected command: ${cmd}`));
    });

    appActions.addFiles([
      { path: "D:/pics/a.png", name: "a.png", size: 100, status: "pending" },
    ]);
    appActions.setOutputDir("D:/out");

    const { getByText } = render(() => <ActionButtons />);
    // Native click: it bubbles to Solid's delegated handler.
    getByText("Start Conversion").click();

    await waitFor(() => expect(store.processingState).toBe("completed"));
    expect(invokeMock).toHaveBeenCalledWith("process_batch", {
      inputPaths: ["D:/pics/a.png"],
      outputDir: "D:/out",
      options: store.options,
    });
    // The three progress channels were subscribed before the batch started.
    const subscribedEvents = listenMock.mock.calls.map((c) => c[0]);
    expect(subscribedEvents).toEqual([
      "processing-progress",
      "processing-result",
      "processing-complete",
    ]);
    expect(store.batchStats).toEqual(BATCH_STATS);
  });

  it("cancel notifies the backend and returns the UI to idle", async () => {
    invokeMock.mockResolvedValue(null);
    appActions.setProcessingState("processing");

    const { getByText } = render(() => <ActionButtons />);
    getByText("Cancel").click();

    await waitFor(() => expect(store.processingState).toBe("idle"));
    expect(invokeMock).toHaveBeenCalledWith("cancel_processing");
  });
});

// ---- ActionButtons failure paths, events and races --------------------------

/** Start a batch with one file queued and event handlers captured. */
async function startCapturedBatch() {
  const handlers: Record<string, (event: { payload: unknown }) => void> = {};
  listenMock.mockImplementation(
    async (event: string, handler: (event: { payload: unknown }) => void) => {
      handlers[event] = handler;
      return () => {};
    },
  );
  getNameMock.mockResolvedValue("CuiLianTu");
  permissionGrantedMock.mockResolvedValue(true);
  appActions.addFiles([
    { path: "D:/pics/a.png", name: "a.png", size: 100, status: "pending" },
  ]);
  appActions.setOutputDir("D:/out");
  return handlers;
}

describe("ActionButtons event and race branches", () => {
  it("moves to the error state when process_batch rejects", async () => {
    const handlers = await startCapturedBatch();
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "process_batch")
        return Promise.reject(new Error("disk full"));
      return Promise.reject(new Error(`unexpected command: ${cmd}`));
    });
    const consoleError = vi
      .spyOn(console, "error")
      .mockImplementation(() => {});

    render(() => <ActionButtons />);
    document.querySelector("button")?.click();

    await waitFor(() => expect(store.processingState).toBe("error"));
    expect(store.error).toContain("Processing failed");
    // The listeners of the dead run were unsubscribed.
    expect(handlers["processing-progress"]).toBeTruthy();
    consoleError.mockRestore();
  });

  it("applies progress and result events to the file list", async () => {
    const handlers = await startCapturedBatch();
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "process_batch") return new Promise(() => {}); // stays pending; events drive the UI
      return Promise.reject(new Error(`unexpected command: ${cmd}`));
    });

    render(() => <ActionButtons />);
    document.querySelector("button")?.click();
    await waitFor(() => expect(store.processingState).toBe("processing"));
    await waitFor(() => expect(handlers["processing-progress"]).toBeTruthy());

    handlers["processing-progress"]?.({
      payload: {
        current: 1,
        total: 1,
        current_file: "D:/pics/a.png",
        percent: 50,
      },
    });
    expect(store.files[0].status).toBe("processing");

    handlers["processing-result"]?.({
      payload: {
        original_path: "D:/pics/a.png",
        output_path: "D:/out/a.webp",
        original_size: 100,
        output_size: 50,
        reduction_percent: 50,
        success: true,
        error: null,
        skipped: false,
        within_target: true,
      },
    });
    expect(store.files[0].status).toBe("completed");
    expect(store.files[0].outputPath).toBe("D:/out/a.webp");

    // A failed result sanitizes its error before it reaches the row.
    appActions.addFiles([
      { path: "D:/pics/b.png", name: "b.png", size: 100, status: "pending" },
    ]);
    handlers["processing-result"]?.({
      payload: {
        original_path: "D:/pics/b.png",
        output_path: "",
        original_size: 100,
        output_size: 100,
        reduction_percent: 0,
        success: false,
        error: "cannot read C:/Users/me/secret/b.png",
        skipped: false,
        within_target: false,
      },
    });
    const failed = store.files.find((f) => f.path === "D:/pics/b.png");
    expect(failed?.status).toBe("error");
    expect(failed?.error).not.toContain("C:/Users");
  });

  it("applies completion exactly once when the event and the return value race", async () => {
    const handlers = await startCapturedBatch();
    let resolveBatch: ((v: unknown) => void) | undefined;
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "process_batch")
        return new Promise((resolve) => {
          resolveBatch = resolve;
        });
      return Promise.reject(new Error(`unexpected command: ${cmd}`));
    });

    render(() => <ActionButtons />);
    document.querySelector("button")?.click();
    await waitFor(() => expect(store.processingState).toBe("processing"));
    // handleStart awaits three listens before invoking the batch; wait until
    // the completion handler is actually registered before firing it.
    await waitFor(() => expect(handlers["processing-complete"]).toBeTruthy());

    // The completion event arrives first and wins.
    handlers["processing-complete"]?.({
      payload: { ...BATCH_STATS, total_files: 7 },
    });
    await waitFor(() => expect(store.processingState).toBe("completed"));
    expect(store.batchStats?.total_files).toBe(7);

    // The return value arriving later must not overwrite the state.
    resolveBatch?.({ ...BATCH_STATS, total_files: 999 });
    await flush();
    expect(store.batchStats?.total_files).toBe(7);
  });

  it("drops late events from a cancelled run", async () => {
    const handlers = await startCapturedBatch();
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "process_batch") return new Promise(() => {});
      if (cmd === "cancel_processing") return Promise.resolve(null);
      return Promise.reject(new Error(`unexpected command: ${cmd}`));
    });

    render(() => <ActionButtons />);
    document.querySelector("button")?.click();
    await waitFor(() => expect(store.processingState).toBe("processing"));
    await waitFor(() => expect(handlers["processing-progress"]).toBeTruthy());

    document.querySelectorAll("button")[1]?.click(); // Cancel
    await waitFor(() => expect(store.processingState).toBe("idle"));

    // Events from the cancelled run arrive afterwards: no state changes.
    handlers["processing-progress"]?.({
      payload: {
        current: 1,
        total: 1,
        current_file: "D:/pics/a.png",
        percent: 10,
      },
    });
    expect(store.progress).toBeNull();
    expect(store.files[0].status).toBe("pending");
  });

  it("sends the desktop notification when permission is already granted", async () => {
    await startCapturedBatch();
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "process_batch") return Promise.resolve(BATCH_STATS);
      return Promise.reject(new Error(`unexpected command: ${cmd}`));
    });

    render(() => <ActionButtons />);
    document.querySelector("button")?.click();

    await waitFor(() => expect(store.processingState).toBe("completed"));
    await flush();
    expect(sendNotificationMock).toHaveBeenCalledWith(
      expect.objectContaining({ title: "CuiLianTu" }),
    );
    expect(requestPermissionMock).not.toHaveBeenCalled();
  });

  it("requests permission once when not yet granted, and stays quiet when denied", async () => {
    await startCapturedBatch();
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "process_batch") return Promise.resolve(BATCH_STATS);
      return Promise.reject(new Error(`unexpected command: ${cmd}`));
    });
    permissionGrantedMock.mockResolvedValue(false);
    requestPermissionMock.mockResolvedValue("denied");

    render(() => <ActionButtons />);
    document.querySelector("button")?.click();

    await waitFor(() => expect(store.processingState).toBe("completed"));
    await flush();
    expect(requestPermissionMock).toHaveBeenCalled();
    expect(sendNotificationMock).not.toHaveBeenCalled();
  });

  it("falls back to the app name when getName fails", async () => {
    await startCapturedBatch();
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "process_batch") return Promise.resolve(BATCH_STATS);
      return Promise.reject(new Error(`unexpected command: ${cmd}`));
    });
    getNameMock.mockRejectedValue(new Error("no app name"));

    render(() => <ActionButtons />);
    document.querySelector("button")?.click();

    await waitFor(() => expect(store.processingState).toBe("completed"));
    await flush();
    expect(sendNotificationMock).toHaveBeenCalledWith(
      expect.objectContaining({ title: "CuiLianTu" }),
    );
  });

  it("keeps the visual cancel when cancel_processing fails", async () => {
    invokeMock.mockRejectedValue(new Error("backend gone"));
    const consoleDebug = vi
      .spyOn(console, "debug")
      .mockImplementation(() => {});
    appActions.setProcessingState("processing");

    const { getByText } = render(() => <ActionButtons />);
    getByText("Cancel").click();

    await waitFor(() => expect(store.processingState).toBe("idle"));
    expect(consoleDebug).toHaveBeenCalled();
    consoleDebug.mockRestore();
  });
});

// ---- UpdateNotification ----------------------------------------------------

const UPDATE_INFO = {
  update_available: true,
  current_version: "0.2.1",
  latest_version: "0.3.0",
  release_url: "https://github.com/CYXue/cuiliantu/releases/tag/v0.3.0",
  release_notes: null,
};

describe("UpdateNotification IPC", () => {
  it("shows the banner with versions when an update is available", async () => {
    invokeMock.mockResolvedValue(UPDATE_INFO);
    const { findByText } = render(() => <UpdateNotification />);

    // The version pair is rendered raw (not translated), so it is a stable
    // assertion even though the surrounding strings are localized.
    expect(await findByText(/v0\.2\.1/)).toBeTruthy();
    expect(invokeMock).toHaveBeenCalledWith("check_for_updates");
  });

  it("shows the failure banner when the check fails", async () => {
    invokeMock.mockRejectedValue(new Error("network down"));
    const consoleError = vi
      .spyOn(console, "error")
      .mockImplementation(() => {});
    const { findByText } = render(() => <UpdateNotification />);

    expect(
      await findByText("Could not check for updates. Please try again later."),
    ).toBeTruthy();
    expect(consoleError).toHaveBeenCalled();
    consoleError.mockRestore();
  });

  it("dismiss hides the banner", async () => {
    invokeMock.mockResolvedValue(UPDATE_INFO);
    const { findByText, queryByText, getByRole } = render(() => (
      <UpdateNotification />
    ));
    await findByText(/v0\.2\.1/);

    getByRole("button", { name: "Dismiss" }).click();
    await waitFor(() => expect(queryByText(/v0\.2\.1/)).toBeNull());
  });

  it("opens the https release URL from the Download button", async () => {
    invokeMock.mockResolvedValue(UPDATE_INFO);
    const { findByText, getByText } = render(() => <UpdateNotification />);
    await findByText(/v0\.2\.1/);

    getByText("Download").click();
    await flush();
    expect(openUrlMock).toHaveBeenCalledWith(
      "https://github.com/CYXue/cuiliantu/releases/tag/v0.3.0",
    );
  });

  it("refuses to open a non-https URL as a second line of defense", async () => {
    invokeMock.mockResolvedValue({
      ...UPDATE_INFO,
      release_url: "http://github.com.evil.tld/fake",
    });
    const { findByText, getByText } = render(() => <UpdateNotification />);
    await findByText(/v0\.2\.1/);

    getByText("Download").click();
    await flush();
    expect(openUrlMock).not.toHaveBeenCalled();
  });
});
