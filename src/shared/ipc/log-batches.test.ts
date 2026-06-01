import { invoke } from "@tauri-apps/api/core";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { listLogBatches } from "./log-batches";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
const mockInvoke = vi.mocked(invoke);

describe("log-batches ipc wrapper", () => {
  beforeEach(() => mockInvoke.mockReset());

  it("invokes list_log_batches and returns the result", async () => {
    const data = [
      { id: "b0", title: "t", kind: "post", time: "방금 전", items: [] },
    ];
    mockInvoke.mockResolvedValue(data);
    await expect(listLogBatches()).resolves.toBe(data);
    expect(mockInvoke).toHaveBeenCalledWith("list_log_batches");
  });
});
