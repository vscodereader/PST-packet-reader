import { invoke } from "@tauri-apps/api/core";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { listStocks, resetStocksFallbackForTests } from "./stocks";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

const mockInvoke = vi.mocked(invoke);

function setTauri(on: boolean) {
  const w = window as unknown as Record<string, unknown>;
  if (on) w.__TAURI_INTERNALS__ = {};
  else delete w.__TAURI_INTERNALS__;
}

describe("stocks ipc wrapper", () => {
  beforeEach(() => {
    mockInvoke.mockReset();
    resetStocksFallbackForTests();
  });
  afterEach(() => setTauri(false));

  it("invokes list_stocks inside Tauri", async () => {
    setTauri(true);
    mockInvoke.mockResolvedValue([]);
    await listStocks();
    expect(mockInvoke).toHaveBeenCalledWith("list_stocks");
  });

  it("returns mock data without invoking outside Tauri", async () => {
    const stocks = await listStocks();
    expect(mockInvoke).not.toHaveBeenCalled();
    expect(stocks.length).toBeGreaterThan(0);
    expect(stocks.some((s) => s.code === "005930")).toBe(true);
  });
});
