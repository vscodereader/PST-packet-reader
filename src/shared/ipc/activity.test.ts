import { invoke } from "@tauri-apps/api/core";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { listActivity, resetActivityFallbackForTests } from "./activity";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

const mockInvoke = vi.mocked(invoke);

function setTauri(on: boolean) {
  const w = window as unknown as Record<string, unknown>;
  if (on) w.__TAURI_INTERNALS__ = {};
  else delete w.__TAURI_INTERNALS__;
}

describe("activity ipc wrapper", () => {
  beforeEach(() => {
    mockInvoke.mockReset();
    resetActivityFallbackForTests();
  });
  afterEach(() => setTauri(false));

  it("invokes list_activity inside Tauri", async () => {
    setTauri(true);
    mockInvoke.mockResolvedValue([]);
    await listActivity();
    expect(mockInvoke).toHaveBeenCalledWith("list_activity");
  });

  it("returns mock data without invoking outside Tauri", async () => {
    const items = await listActivity();
    expect(mockInvoke).not.toHaveBeenCalled();
    expect(items.length).toBeGreaterThan(0);
    expect(items.some((a) => a.type === "error")).toBe(true);
  });
});
