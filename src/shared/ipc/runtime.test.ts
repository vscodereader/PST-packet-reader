import { afterEach, describe, expect, it } from "vitest";

import { isTauri } from "./runtime";

function clearTauri() {
  delete (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__;
}

describe("isTauri", () => {
  afterEach(clearTauri);

  it("returns false in a plain browser / jsdom environment", () => {
    clearTauri();
    expect(isTauri()).toBe(false);
  });

  it("returns true when Tauri injects its internals global", () => {
    (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__ = {};
    expect(isTauri()).toBe(true);
  });
});
