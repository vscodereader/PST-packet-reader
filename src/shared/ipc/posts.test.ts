import { invoke } from "@tauri-apps/api/core";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { LibraryPost } from "@/shared/bindings/LibraryPost";

import {
  deletePost,
  listPosts,
  resetPostsFallbackForTests,
  upsertPost,
} from "./posts";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

const mockInvoke = vi.mocked(invoke);

function setTauri(on: boolean) {
  const w = window as unknown as Record<string, unknown>;
  if (on) w.__TAURI_INTERNALS__ = {};
  else delete w.__TAURI_INTERNALS__;
}

const sample: LibraryPost = {
  id: "p1",
  title: "new post",
  kind: "post",
  updated: "방금 전",
  words: 10,
  status: "draft",
  excerpt: "x",
};

describe("posts ipc wrapper", () => {
  beforeEach(() => {
    mockInvoke.mockReset();
    resetPostsFallbackForTests();
  });
  afterEach(() => setTauri(false));

  describe("inside Tauri", () => {
    beforeEach(() => setTauri(true));

    it("listPosts invokes list_posts", async () => {
      mockInvoke.mockResolvedValue([sample]);
      const result = await listPosts();
      expect(mockInvoke).toHaveBeenCalledWith("list_posts");
      expect(result).toEqual([sample]);
    });

    it("upsertPost invokes upsert_post with the post payload", async () => {
      mockInvoke.mockResolvedValue([sample]);
      await upsertPost(sample);
      expect(mockInvoke).toHaveBeenCalledWith("upsert_post", { post: sample });
    });

    it("deletePost invokes delete_post with the id payload", async () => {
      mockInvoke.mockResolvedValue([]);
      await deletePost("p1");
      expect(mockInvoke).toHaveBeenCalledWith("delete_post", { id: "p1" });
    });
  });

  describe("without Tauri (mock fallback)", () => {
    it("listPosts returns the mock data without invoking", async () => {
      const result = await listPosts();
      expect(mockInvoke).not.toHaveBeenCalled();
      expect(result.length).toBeGreaterThan(0);
    });

    it("upsertPost prepends a new post and persists", async () => {
      const before = (await listPosts()).length;
      const afterUpsert = await upsertPost(sample);
      expect(afterUpsert.length).toBe(before + 1);
      expect(afterUpsert[0]?.id).toBe("p1");
      expect((await listPosts()).length).toBe(before + 1);
    });

    it("upsertPost replaces an existing post in place", async () => {
      const list = await listPosts();
      const first = list[0];
      expect(first).toBeDefined();
      if (!first) return;
      const edited: LibraryPost = { ...first, title: "edited title" };
      const afterUpsert = await upsertPost(edited);
      expect(afterUpsert.length).toBe(list.length);
      expect(afterUpsert.find((p) => p.id === first.id)?.title).toBe(
        "edited title",
      );
    });

    it("deletePost removes the post", async () => {
      const list = await listPosts();
      const first = list[0];
      expect(first).toBeDefined();
      if (!first) return;
      const afterDelete = await deletePost(first.id);
      expect(afterDelete.find((p) => p.id === first.id)).toBeUndefined();
    });
  });
});
