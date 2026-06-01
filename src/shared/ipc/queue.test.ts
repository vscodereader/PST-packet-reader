import { invoke } from "@tauri-apps/api/core";
import { beforeEach, describe, expect, it, vi } from "vitest";

import {
  cancelQueueNow,
  cancelQueueScheduled,
  listQueueNow,
  listQueueScheduled,
} from "./queue";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
const mockInvoke = vi.mocked(invoke);

describe("queue ipc wrapper", () => {
  beforeEach(() => mockInvoke.mockReset());

  it("listQueueNow / listQueueScheduled invoke their commands", async () => {
    mockInvoke.mockResolvedValue([]);
    await listQueueNow();
    expect(mockInvoke).toHaveBeenCalledWith("list_queue_now");
    await listQueueScheduled();
    expect(mockInvoke).toHaveBeenCalledWith("list_queue_scheduled");
  });

  it("cancelQueueNow / cancelQueueScheduled invoke with the id", async () => {
    mockInvoke.mockResolvedValue([]);
    await cancelQueueNow("q1");
    expect(mockInvoke).toHaveBeenCalledWith("cancel_queue_now", { id: "q1" });
    await cancelQueueScheduled("qs1");
    expect(mockInvoke).toHaveBeenCalledWith("cancel_queue_scheduled", {
      id: "qs1",
    });
  });
});
