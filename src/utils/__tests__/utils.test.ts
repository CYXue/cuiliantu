import { describe, expect, it } from "vitest";
import { basename } from "../path";
import { sanitizeError } from "../sanitize";

describe("basename", () => {
  it("handles forward slashes", () => {
    expect(basename("/home/me/pics/a.png")).toBe("a.png");
  });

  it("handles backslashes", () => {
    expect(basename("C:\\Users\\me\\pics\\a.png")).toBe("a.png");
  });

  it("returns the input when there is no separator", () => {
    expect(basename("a.png")).toBe("a.png");
  });

  it("falls back to the whole path when it ends with a separator", () => {
    // The final segment is empty: the original path is the least-surprising
    // answer rather than an empty string.
    expect(basename("D:/dir/")).toBe("D:/dir/");
  });
});

describe("sanitizeError", () => {
  it("strips Windows absolute paths", () => {
    expect(sanitizeError("Failed to read: C:\\Users\\me\\pics\\a.png")).toBe(
      "Failed to read: a.png",
    );
  });

  it("strips Unix absolute paths", () => {
    expect(sanitizeError("Failed to read: /home/me/pics/a.png")).toBe(
      "Failed to read: a.png",
    );
  });

  it("strips UNC paths", () => {
    expect(sanitizeError("Failed to read: \\\\NAS\\share\\pics\\a.png")).toBe(
      "Failed to read: a.png",
    );
  });

  it("strips Unix paths containing spaces under known roots", () => {
    expect(sanitizeError("Failed to read: /home/John Doe/pics/a.png")).toBe(
      "Failed to read: a.png",
    );
    expect(
      sanitizeError("Failed to read: /Users/John Doe/Pictures/a.png"),
    ).toBe("Failed to read: a.png");
  });

  it("does not mangle slash-separated words that are not paths", () => {
    expect(sanitizeError("options /verbose /quiet")).toBe(
      "options /verbose /quiet",
    );
  });

  it("leaves messages without paths intact", () => {
    expect(sanitizeError("Something went wrong")).toBe("Something went wrong");
  });

  it("returns empty messages untouched", () => {
    expect(sanitizeError("")).toBe("");
  });
});
