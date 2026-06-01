import { invoke } from "@tauri-apps/api/core";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { listLogBatches, resetLogBatchesFallbackForTests } from "./log-batches";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

const mockInvoke = vi.mocked(invoke);

function setTauri(on: boolean) {
  const w = window as unknown as Record<string, unknown>;
  if (on) w.__TAURI_INTERNALS__ = {};
  else delete w.__TAURI_INTERNALS__;
}

describe("log-batches ipc wrapper", () => {
  beforeEach(() => {
    mockInvoke.mockReset();
    resetLogBatchesFallbackForTests();
  });
  afterEach(() => setTauri(false));

  it("invokes list_log_batches inside Tauri", async () => {
    setTauri(true);
    mockInvoke.mockResolvedValue([]);
    await listLogBatches();
    expect(mockInvoke).toHaveBeenCalledWith("list_log_batches");
  });

  it("returns mock data without invoking outside Tauri", async () => {
    const batches = await listLogBatches();
    expect(mockInvoke).not.toHaveBeenCalled();
    expect(batches.length).toBeGreaterThan(0);
    expect(batches.some((b) => b.items.some((i) => i.status === "fail"))).toBe(
      true,
    );
  });
});
