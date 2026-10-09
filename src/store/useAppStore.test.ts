import { beforeEach, describe, expect, it } from "vitest";
import type { FileItem, ProcessingOptions } from "../types";
import { mergeDefaults } from "./defaults";
import { appActions, store } from "./useAppStore";

function makeFile(path: string, overrides: Partial<FileItem> = {}): FileItem {
  return {
    path,
    name: path.split("/").pop() ?? path,
    size: 1000,
    status: "pending",
    ...overrides,
  };
}

// The Solid store is a module-level singleton, so reset it before each test.
beforeEach(() => {
  appActions.reset();
  appActions.setOutputDir("");
});

describe("useAppStore (Solid)", () => {
  describe("addFiles", () => {
    it("adds files", () => {
      appActions.addFiles([makeFile("/a.png"), makeFile("/b.png")]);
      expect(store.files.map((f) => f.path)).toEqual(["/a.png", "/b.png"]);
    });

    it("dedups by path", () => {
      appActions.addFiles([makeFile("/a.png")]);
      appActions.addFiles([makeFile("/a.png"), makeFile("/b.png")]);
      expect(store.files.map((f) => f.path)).toEqual(["/a.png", "/b.png"]);
    });

    it("auto-selects the first added file so the preview switches to it", () => {
      appActions.addFiles([makeFile("/a.png"), makeFile("/b.png")]);
      expect(store.selectedPath).toBe("/a.png");
    });

    it("switches to the first newly added file of a later batch", () => {
      appActions.addFiles([makeFile("/a.png")]);
      appActions.setSelectedPath(null);
      appActions.addFiles([makeFile("/b.png"), makeFile("/c.png")]);
      expect(store.selectedPath).toBe("/b.png");
    });

    it("keeps selection when nothing new was added (dedup only)", () => {
      appActions.addFiles([makeFile("/a.png")]);
      appActions.setSelectedPath(null);
      appActions.addFiles([makeFile("/a.png")]);
      expect(store.selectedPath).toBeNull();
    });
  });

  describe("removeFile", () => {
    it("removes only the matching path", () => {
      appActions.addFiles([makeFile("/a.png"), makeFile("/b.png")]);
      appActions.removeFile("/a.png");
      expect(store.files.map((f) => f.path)).toEqual(["/b.png"]);
    });

    it("is a no-op for unknown path", () => {
      appActions.addFiles([makeFile("/a.png")]);
      appActions.removeFile("/zzz.png");
      expect(store.files).toHaveLength(1);
    });
  });

  describe("updateFileStatus", () => {
    it("updates status and result of the target file only", () => {
      appActions.addFiles([makeFile("/a.png"), makeFile("/b.png")]);
      appActions.updateFileStatus("/a.png", "completed", {
        outputPath: "/a.webp",
        outputSize: 400,
        reductionPercent: 60,
      });

      const [a, b] = store.files;
      expect(a.status).toBe("completed");
      expect(a.outputPath).toBe("/a.webp");
      expect(a.outputSize).toBe(400);
      expect(a.reductionPercent).toBe(60);
      // Other files must be unaffected
      expect(b.status).toBe("pending");
    });
  });

  describe("resetFileStatuses", () => {
    it("resets statuses but keeps files", () => {
      appActions.addFiles([makeFile("/a.png")]);
      appActions.updateFileStatus("/a.png", "completed", {
        outputPath: "/a.webp",
        outputSize: 400,
        reductionPercent: 60,
        error: "err",
      });
      appActions.resetFileStatuses();

      const [a] = store.files;
      expect(a.path).toBe("/a.png");
      expect(a.status).toBe("pending");
      expect(a.outputPath).toBeUndefined();
      expect(a.outputSize).toBeUndefined();
      expect(a.reductionPercent).toBeUndefined();
      expect(a.error).toBeUndefined();
    });
  });

  describe("setOptions", () => {
    it("overwrites only provided keys", () => {
      const before = store.options;
      appActions.setOptions({ quality: 50 });

      expect(store.options.quality).toBe(50);
      expect(store.options.format).toBe(before.format);
      expect(store.options.compression).toBe(before.compression);
    });
  });

  describe("applyTemplate", () => {
    it("mergeDefaults fills options added after an older template was saved", () => {
      // A template persisted before the edit options existed carries only the
      // old keys. mergeDefaults (used by applyTemplate) must fill every
      // missing key from the factory defaults so no option is `undefined`.
      const merged = mergeDefaults({
        format: "webp",
        quality: 55,
      } as Partial<ProcessingOptions>);

      expect(merged.quality).toBe(55);
      expect(merged.brightness).toBe(0);
      expect(merged.crop_ratio).toBeNull();
      expect(merged.rotate_degrees).toBe(0);
      expect(merged.grayscale).toBe(false);
      expect(merged.auto_contrast).toBe(false);
      // Every key of the full option set is defined.
      for (const [key, value] of Object.entries(merged)) {
        expect(value, key).not.toBeUndefined();
      }
    });
  });

  describe("clearFiles", () => {
    it("clears files and stats, returns to idle", () => {
      appActions.addFiles([makeFile("/a.png")]);
      appActions.setProcessingState("completed");
      appActions.clearFiles();

      expect(store.files).toEqual([]);
      expect(store.batchStats).toBeNull();
      expect(store.processingState).toBe("idle");
    });
  });

  describe("reset", () => {
    it("resets processing state; keeps outputDir", () => {
      appActions.addFiles([makeFile("/a.png")]);
      appActions.setOutputDir("/out");
      appActions.setError("failed");
      appActions.setProcessingState("error");
      appActions.reset();

      expect(store.files).toEqual([]);
      expect(store.error).toBeNull();
      expect(store.processingState).toBe("idle");
      // The output dir is a user-selected setting, so `reset` keeps it.
      expect(store.outputDir).toBe("/out");
    });
  });

  describe("setFileType", () => {
    it("stores detected format and flags an extension mismatch", () => {
      appActions.addFiles([makeFile("/x.png")]);
      appActions.setFileType("/x.png", {
        path: "/x.png",
        extension: "png",
        detected_format: "jpeg",
        detected_mime: "image/jpeg",
        matches_extension: false,
        width: 10,
        height: 20,
        size_bytes: 1000,
      });

      const f = store.files[0];
      expect(f.detectedFormat).toBe("jpeg");
      expect(f.matchesExtension).toBe(false);
      expect(f.detectedMime).toBe("image/jpeg");
      expect(f.detectedWidth).toBe(10);
      expect(f.detectedHeight).toBe(20);
    });

    it("keeps files without a detected type untouched", () => {
      appActions.addFiles([makeFile("/y.webp")]);
      expect(store.files[0].detectedFormat).toBeUndefined();
      expect(store.files[0].matchesExtension).toBeUndefined();
    });
  });

  describe("target size default", () => {
    it("defaults target size to off", () => {
      expect(store.options.target_size_bytes).toBeNull();
    });
  });

  describe("integrated options", () => {
    it("has sensible defaults for the new pipeline options", () => {
      const o = store.options;
      expect(o.resize_mode).toBe("pixels");
      expect(o.resize_percent).toBe(100);
      expect(o.fit_mode).toBe("stretch");
      expect(o.rotate).toBe(0);
      expect(o.flip_horizontal).toBe(false);
      expect(o.watermark).toBeNull();
      expect(o.quantize_colors).toBeNull();
      expect(o.filename_pattern).toBe("{name}");
      expect(o.conflict_policy).toBe("overwrite");
      expect(o.skip_if_larger).toBe(false);
      expect(o.preserve_animation).toBe(true);
      expect(o.target_size_bytes).toBeNull();
    });

    it("updates selection and clears it with the file", () => {
      appActions.addFiles([makeFile("/sel.png")]);
      appActions.setSelectedPath("/sel.png");
      expect(store.selectedPath).toBe("/sel.png");
      appActions.setSelectedPath(null);
      expect(store.selectedPath).toBeNull();

      // Removing the selected file clears the selection too.
      appActions.setSelectedPath("/sel.png");
      appActions.removeFile("/sel.png");
      expect(store.selectedPath).toBeNull();
    });
  });

  describe("templates", () => {
    // Custom templates live in a module-level store shared across tests,
    // so clean them up before each case (built-ins are constants).
    beforeEach(() => {
      [...store.templates].forEach((tpl) => {
        appActions.deleteTemplate(tpl.id);
      });
    });

    it("saves the current options as a named template", () => {
      appActions.setOptions({ format: "png", quality: 60 });
      appActions.saveTemplate("my set");

      expect(store.templates).toHaveLength(1);
      const tpl = store.templates[0];
      expect(tpl.name).toBe("my set");
      expect(tpl.options.format).toBe("png");
      expect(tpl.options.quality).toBe(60);
    });

    it("ignores blank names", () => {
      appActions.saveTemplate("   ");
      expect(store.templates).toHaveLength(0);
    });

    it("applying a template replaces every option and marks it active", () => {
      appActions.setOptions({ format: "gif", quality: 10, rotate: 90 });
      appActions.saveTemplate("snap");
      appActions.setOptions({ format: "bmp", quality: 99, rotate: 180 });

      appActions.applyTemplate(store.templates[0].id);
      expect(store.options.format).toBe("gif");
      expect(store.options.quality).toBe(10);
      expect(store.options.rotate).toBe(90);
      expect(store.activeTemplateId).toBe(store.templates[0].id);
    });

    it("applies built-in templates by id", () => {
      appActions.applyTemplate("builtin:id-photo");
      expect(store.options.format).toBe("jpeg");
      expect(store.options.width).toBe(196);
      expect(store.options.height).toBe(250);
      expect(store.options.target_size_bytes).toBe(50 * 1024);
      expect(store.activeTemplateId).toBe("builtin:id-photo");
    });

    it("applies standard inch-photo size templates", () => {
      appActions.applyTemplate("builtin:id-1inch-small");
      expect(store.options.width).toBe(260);
      expect(store.options.height).toBe(378);

      appActions.applyTemplate("builtin:id-1inch");
      expect(store.options.width).toBe(295);
      expect(store.options.height).toBe(413);

      appActions.applyTemplate("builtin:id-1inch-large");
      expect(store.options.width).toBe(390);
      expect(store.options.height).toBe(567);

      appActions.applyTemplate("builtin:id-2inch");
      expect(store.options.width).toBe(413);
      expect(store.options.height).toBe(579);

      appActions.applyTemplate("builtin:id-2inch-small");
      expect(store.options.width).toBe(413);
      expect(store.options.height).toBe(531);

      appActions.applyTemplate("builtin:id-sscard");
      expect(store.options.width).toBe(358);
      expect(store.options.height).toBe(441);
      // All inch-photo presets share these traits.
      expect(store.options.format).toBe("jpeg");
      expect(store.options.quality).toBe(90);
      expect(store.options.fit_mode).toBe("cover");
      expect(store.options.target_size_bytes).toBeNull();
    });

    it("manual option edits deactivate the active template", () => {
      appActions.applyTemplate("builtin:web");
      expect(store.activeTemplateId).toBe("builtin:web");

      appActions.setOptions({ quality: 42 });
      expect(store.activeTemplateId).toBeNull();
      // The manual edit itself is kept.
      expect(store.options.quality).toBe(42);
    });

    it("renames and reorders custom templates", () => {
      appActions.saveTemplate("a");
      appActions.saveTemplate("b");
      appActions.saveTemplate("c");

      appActions.renameTemplate(store.templates[0].id, "a2");
      expect(store.templates.map((tpl) => tpl.name)).toEqual(["a2", "b", "c"]);

      appActions.moveTemplate(store.templates[1].id, -1);
      expect(store.templates.map((tpl) => tpl.name)).toEqual(["b", "a2", "c"]);

      // Moving the first item up is a no-op.
      appActions.moveTemplate(store.templates[0].id, -1);
      expect(store.templates.map((tpl) => tpl.name)).toEqual(["b", "a2", "c"]);
    });

    it("deletes a custom template and clears an active pointer to it", () => {
      appActions.saveTemplate("gone");
      const id = store.templates[0].id;
      appActions.applyTemplate(id);

      appActions.deleteTemplate(id);
      expect(store.templates).toHaveLength(0);
      expect(store.activeTemplateId).toBeNull();
    });
  });
});
