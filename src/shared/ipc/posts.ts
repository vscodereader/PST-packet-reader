import { invoke } from "@tauri-apps/api/core";

import type { LibraryPost } from "@/shared/bindings/LibraryPost";

export type { LibraryPost };

/**
 * Typed wrapper around the Rust `posts` commands over Tauri IPC. Every mutation
 * returns the full updated list. Mirrors the accounts wrapper.
 */

export async function listPosts(): Promise<LibraryPost[]> {
  return invoke<LibraryPost[]>("list_posts");
}

export async function upsertPost(post: LibraryPost): Promise<LibraryPost[]> {
  return invoke<LibraryPost[]>("upsert_post", { post });
}

export async function deletePost(id: string): Promise<LibraryPost[]> {
  return invoke<LibraryPost[]>("delete_post", { id });
}
