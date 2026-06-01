import { invoke } from "@tauri-apps/api/core";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { listScheduled } from "./scheduled";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
const mockInvoke = vi.mocked(invoke);

describe("scheduled ipc wrapper", () => {
  beforeEach(() => mockInvoke.mockReset());

  it("invokes list_scheduled and returns the result", async () => {
    const data = [{ id: "s1", title: "t", accounts: ["a1"], kind: "post" }];
    mockInvoke.mockResolvedValue(data);
    await expect(listScheduled()).resolves.toBe(data);
    expect(mockInvoke).toHaveBeenCalledWith("list_scheduled");
  });
});
