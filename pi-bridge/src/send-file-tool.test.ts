import { describe, it } from "node:test";
import assert from "node:assert";
import { isPathInsideWorkspace, detectMimeType } from "./send-file-tool.js";

describe("send-file-tool", () => {
  describe("isPathInsideWorkspace", () => {
    it("allows a file inside the workspace", () => {
      assert.strictEqual(
        isPathInsideWorkspace("/workspace/report.txt", "/workspace"),
        true
      );
    });

    it("allows a nested file inside the workspace", () => {
      assert.strictEqual(
        isPathInsideWorkspace("/workspace/docs/readme.md", "/workspace"),
        true
      );
    });

    it("rejects a file outside the workspace", () => {
      assert.strictEqual(
        isPathInsideWorkspace("/etc/passwd", "/workspace"),
        false
      );
    });

    it("rejects a path that is a sibling prefix of the workspace", () => {
      assert.strictEqual(
        isPathInsideWorkspace("/workspace-evil/file.txt", "/workspace"),
        false
      );
    });

    it("rejects traversal outside the workspace", () => {
      assert.strictEqual(
        isPathInsideWorkspace("/workspace/../outside.txt", "/workspace"),
        false
      );
    });
  });

  describe("detectMimeType", () => {
    it("detects common mime types by extension", () => {
      assert.strictEqual(detectMimeType("file.txt"), "text/plain");
      assert.strictEqual(detectMimeType("file.png"), "image/png");
      assert.strictEqual(detectMimeType("file.pdf"), "application/pdf");
    });

    it("falls back to octet-stream for unknown extensions", () => {
      assert.strictEqual(detectMimeType("file.unknown"), "application/octet-stream");
    });
  });
});
