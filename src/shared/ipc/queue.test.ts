import { invoke } from "@tauri-apps/api/core";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import {
  cancelQueueNow,
  cancelQueueScheduled,
  listQueueNow,
  listQueueScheduled,
  resetQueueFallbackForTests,
} from "./queue";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

const mockInvoke = vi.mocked(invoke);

function setTauri(on: boolean) {
  const w = window as unknown as Record<string, unknown>;
  if (on) w.__TAURI_INTERNALS__ = {};
  else delete w.__TAURI_INTERNALS__;
}

describe("queue ipc wrapper", () => {
  beforeEach(() => {
    mockInvoke.mockReset();
    resetQueueFallbackForTests();
  });
  afterEach(() => setTauri(false));

  describe("inside Tauri", () => {
    beforeEach(() => setTauri(true));

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

  describe("without Tauri (mock fallback)", () => {
    it("lists return mock data without invoking", async () => {
      const now = await listQueueNow();
      const sched = await listQueueScheduled();
      expect(mockInvoke).not.toHaveBeenCalled();
      expect(now.length).toBeGreaterThan(0);
      expect(sched.length).toBeGreaterThan(0);
    });

    it("cancelQueueNow removes the item and persists", async () => {
      const before = await listQueueNow();
      const first = before[0];
      expect(first).toBeDefined();
      if (!first) return;
      const after = await cancelQueueNow(first.id);
      expect(after.find((q) => q.id === first.id)).toBeUndefined();
      expect((await listQueueNow()).length).toBe(before.length - 1);
    });

    it("cancelQueueScheduled removes the scheduled item", async () => {
      const before = await listQueueScheduled();
      const first = before[0];
      expect(first).toBeDefined();
      if (!first) return;
      const after = await cancelQueueScheduled(first.id);
      expect(after.find((q) => q.id === first.id)).toBeUndefined();
    });
  });
});
