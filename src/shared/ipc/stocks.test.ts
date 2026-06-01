import { invoke } from "@tauri-apps/api/core";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { listStocks } from "./stocks";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
const mockInvoke = vi.mocked(invoke);

describe("stocks ipc wrapper", () => {
  beforeEach(() => mockInvoke.mockReset());

  it("invokes list_stocks and returns the result", async () => {
    const data = [{ code: "005930" }];
    mockInvoke.mockResolvedValue(data);
    await expect(listStocks()).resolves.toBe(data);
    expect(mockInvoke).toHaveBeenCalledWith("list_stocks");
  });
});
