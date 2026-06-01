import { invoke } from "@tauri-apps/api/core";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { listCafes } from "./cafes";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
const mockInvoke = vi.mocked(invoke);

describe("cafes ipc wrapper", () => {
  beforeEach(() => mockInvoke.mockReset());

  it("invokes list_cafes and returns the result", async () => {
    const data = [{ name: "주식투자연구소 카페", boards: ["종목분석"] }];
    mockInvoke.mockResolvedValue(data);
    await expect(listCafes()).resolves.toBe(data);
    expect(mockInvoke).toHaveBeenCalledWith("list_cafes");
  });
});
