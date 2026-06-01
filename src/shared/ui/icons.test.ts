import { describe, it, expect } from "vitest";

import { Icon } from "./icons";

describe("Icon map", () => {
  it("exposes the semantic icon names used across the UI", () => {
    for (const name of ["dashboard", "pencil", "bell", "check", "x"] as const) {
      expect(Icon[name]).toBeDefined();
    }
  });

  it("maps every entry to a renderable component", () => {
    const values = Object.values(Icon);
    expect(values.length).toBeGreaterThan(0);
    expect(
      values.every((c) => typeof c === "function" || typeof c === "object"),
    ).toBe(true);
  });
});
