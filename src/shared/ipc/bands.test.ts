import { invoke } from "@tauri-apps/api/core";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { listBands } from "./bands";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
const mockInvoke = vi.mocked(invoke);

describe("bands ipc wrapper", () => {
  beforeEach(() => mockInvoke.mockReset());

  it("invokes list_bands and returns the result", async () => {
    const data = [{ name: "가치투자모임 BAND" }];
    mockInvoke.mockResolvedValue(data);
    await expect(listBands()).resolves.toBe(data);
    expect(mockInvoke).toHaveBeenCalledWith("list_bands");
  });
});
