import { invoke } from "@tauri-apps/api/core";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { listScheduled, resetScheduledFallbackForTests } from "./scheduled";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

const mockInvoke = vi.mocked(invoke);

function setTauri(on: boolean) {
  const w = window as unknown as Record<string, unknown>;
  if (on) w.__TAURI_INTERNALS__ = {};
  else delete w.__TAURI_INTERNALS__;
}

describe("scheduled ipc wrapper", () => {
  beforeEach(() => {
    mockInvoke.mockReset();
    resetScheduledFallbackForTests();
  });
  afterEach(() => setTauri(false));

  it("invokes list_scheduled inside Tauri", async () => {
    setTauri(true);
    mockInvoke.mockResolvedValue([]);
    await listScheduled();
    expect(mockInvoke).toHaveBeenCalledWith("list_scheduled");
  });

  it("returns mock data without invoking outside Tauri", async () => {
    const items = await listScheduled();
    expect(mockInvoke).not.toHaveBeenCalled();
    expect(items.length).toBeGreaterThan(0);
    expect(items.some((s) => s.accounts.length > 1)).toBe(true);
  });
});
