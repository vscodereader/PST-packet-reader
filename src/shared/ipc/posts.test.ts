import { invoke } from "@tauri-apps/api/core";
import { beforeEach, describe, expect, it, vi } from "vitest";

import type { LibraryPost } from "@/shared/bindings/LibraryPost";

import { deletePost, listPosts, upsertPost } from "./posts";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
const mockInvoke = vi.mocked(invoke);

const post: LibraryPost = {
  id: "p1",
  title: "t",
  kind: "post",
  updated: "방금 전",
  words: 1,
  status: "draft",
  excerpt: "e",
};

describe("posts ipc wrapper", () => {
  beforeEach(() => mockInvoke.mockReset());

  it("listPosts invokes list_posts and returns the result", async () => {
    mockInvoke.mockResolvedValue([post]);
    await expect(listPosts()).resolves.toEqual([post]);
    expect(mockInvoke).toHaveBeenCalledWith("list_posts");
  });

  it("upsertPost invokes upsert_post with the post", async () => {
    mockInvoke.mockResolvedValue([post]);
    await upsertPost(post);
    expect(mockInvoke).toHaveBeenCalledWith("upsert_post", { post });
  });

  it("deletePost invokes delete_post with the id", async () => {
    mockInvoke.mockResolvedValue([]);
    await deletePost("p1");
    expect(mockInvoke).toHaveBeenCalledWith("delete_post", { id: "p1" });
  });
});
