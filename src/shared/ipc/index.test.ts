import { invoke } from "@tauri-apps/api/core";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { ipc } from "./index";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
const mockInvoke = vi.mocked(invoke);

describe("ipc facade", () => {
  beforeEach(() => {
    mockInvoke.mockReset();
    mockInvoke.mockResolvedValue([]);
  });

  it("accounts channels map to the right commands + args", async () => {
    const a = { id: "a1" } as never;
    await ipc.accounts.list();
    expect(mockInvoke).toHaveBeenCalledWith("list_accounts", undefined);
    await ipc.accounts.add(a);
    expect(mockInvoke).toHaveBeenCalledWith("add_account", { account: a });
    await ipc.accounts.update(a);
    expect(mockInvoke).toHaveBeenCalledWith("update_account", { account: a });
    await ipc.accounts.remove(["a1", "a2"]);
    expect(mockInvoke).toHaveBeenCalledWith("delete_accounts", {
      ids: ["a1", "a2"],
    });
  });

  it("posts channels map to the right commands + args", async () => {
    const p = { id: "p1" } as never;
    await ipc.posts.list();
    expect(mockInvoke).toHaveBeenCalledWith("list_posts", undefined);
    await ipc.posts.upsert(p);
    expect(mockInvoke).toHaveBeenCalledWith("upsert_post", { post: p });
    await ipc.posts.remove("p1");
    expect(mockInvoke).toHaveBeenCalledWith("delete_post", { id: "p1" });
  });

  it("queue channels map to the right commands + args", async () => {
    await ipc.queue.listNow();
    expect(mockInvoke).toHaveBeenCalledWith("list_queue_now", undefined);
    await ipc.queue.listScheduled();
    expect(mockInvoke).toHaveBeenCalledWith("list_queue_scheduled", undefined);
    await ipc.queue.cancelNow("q1");
    expect(mockInvoke).toHaveBeenCalledWith("cancel_queue_now", { id: "q1" });
    await ipc.queue.cancelScheduled("qs1");
    expect(mockInvoke).toHaveBeenCalledWith("cancel_queue_scheduled", {
      id: "qs1",
    });
    await ipc.queue.promote("qs1");
    expect(mockInvoke).toHaveBeenCalledWith("promote_queue_scheduled", {
      id: "qs1",
    });
  });

  it("read-only channels each invoke their list command", async () => {
    await ipc.stocks.list();
    await ipc.activity.list();
    await ipc.stats.list();
    await ipc.logBatches.list();
    await ipc.cafes.list();
    await ipc.bands.list();
    expect(mockInvoke.mock.calls.map((c) => c[0])).toEqual([
      "list_stocks",
      "list_activity",
      "list_stats",
      "list_log_batches",
      "list_cafes",
      "list_bands",
    ]);
  });
});
