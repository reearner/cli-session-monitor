import { describe, it, expect } from "vitest";
import { t, setLang } from "./i18n";

describe("i18n t()", () => {
  it("returns the English string for the active language", () => {
    setLang("en");
    expect(t("status.running")).toBe("Running");
    expect(t("status.done")).toBe("Replied");
  });

  it("returns the Chinese string when language is zh", () => {
    setLang("zh");
    expect(t("status.done")).toBe("已回复");
    expect(t("status.waiting")).toBe("等待你确认");
  });

  it("substitutes {var} placeholders", () => {
    setLang("en");
    expect(t("card.window", { title: "main.rs" })).toBe("Window: main.rs");
  });

  it("falls back to the key when missing", () => {
    expect(t("no.such.key")).toBe("no.such.key");
  });
});
