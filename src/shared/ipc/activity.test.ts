import { invoke } from "@tauri-apps/api/core";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { listActivity } from "./activity";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
const mockInvoke = vi.mocked(invoke);

describe("activity ipc wrapper", () => {
  beforeEach(() => mockInvoke.mockReset());

  it("invokes list_activity and returns the result", async () => {
    const data = [{ id: "ac1", type: "info", text: "t", time: "방금 전" }];
    mockInvoke.mockResolvedValue(data);
    await expect(listActivity()).resolves.toBe(data);
    expect(mockInvoke).toHaveBeenCalledWith("list_activity");
  });
});
