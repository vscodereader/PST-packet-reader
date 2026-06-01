import { invoke } from "@tauri-apps/api/core";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { listStats } from "./stats";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
const mockInvoke = vi.mocked(invoke);

describe("stats ipc wrapper", () => {
  beforeEach(() => mockInvoke.mockReset());

  it("invokes list_stats and returns the result", async () => {
    const data = [{ key: "rate", label: "성공률", value: "97%" }];
    mockInvoke.mockResolvedValue(data);
    await expect(listStats()).resolves.toBe(data);
    expect(mockInvoke).toHaveBeenCalledWith("list_stats");
  });
});
