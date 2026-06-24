import { describe, it, expect } from "vitest";
import { keyId, sourceLabel } from "./types";

describe("keyId", () => {
  it("joins source/host/session_id into a stable DOM key", () => {
    expect(keyId({ source: "codex", host: "h1", session_id: "s1" })).toBe("codex::h1::s1");
  });
});

describe("sourceLabel", () => {
  it("maps each source to its display label", () => {
    expect(sourceLabel("claude-code")).toBe("Claude Code");
    expect(sourceLabel("codex")).toBe("Codex");
  });
});
