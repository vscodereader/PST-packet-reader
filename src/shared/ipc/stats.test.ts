import { invoke } from "@tauri-apps/api/core";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { listStats, resetStatsFallbackForTests } from "./stats";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

const mockInvoke = vi.mocked(invoke);

function setTauri(on: boolean) {
  const w = window as unknown as Record<string, unknown>;
  if (on) w.__TAURI_INTERNALS__ = {};
  else delete w.__TAURI_INTERNALS__;
}

describe("stats ipc wrapper", () => {
  beforeEach(() => {
    mockInvoke.mockReset();
    resetStatsFallbackForTests();
  });
  afterEach(() => setTauri(false));

  it("invokes list_stats inside Tauri", async () => {
    setTauri(true);
    mockInvoke.mockResolvedValue([]);
    await listStats();
    expect(mockInvoke).toHaveBeenCalledWith("list_stats");
  });

  it("returns mock data without invoking outside Tauri", async () => {
    const stats = await listStats();
    expect(mockInvoke).not.toHaveBeenCalled();
    expect(stats.length).toBeGreaterThan(0);
    expect(stats.some((s) => s.key === "rate")).toBe(true);
  });
});
