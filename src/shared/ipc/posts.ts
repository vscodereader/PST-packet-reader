import { invoke } from "@tauri-apps/api/core";

import type { LibraryPost } from "@/shared/bindings/LibraryPost";
import { LIBRARY } from "@/shared/data/mock";

import { isTauri } from "./runtime";

export type { LibraryPost };

/**
 * Typed wrapper around the Rust `posts` commands, with a mock fallback for
 * browser dev / Vitest. Every mutation returns the full updated list. Mirrors
 * the accounts wrapper.
 */

let fallback: LibraryPost[] = LIBRARY.map((p) => ({ ...p }));

/** Reset the in-memory fallback to the pristine mock data. Test-only seam. */
export function resetPostsFallbackForTests(): void {
  fallback = LIBRARY.map((p) => ({ ...p }));
}

function snapshot(): LibraryPost[] {
  return fallback.map((p) => ({ ...p }));
}

export async function listPosts(): Promise<LibraryPost[]> {
  if (isTauri()) return invoke<LibraryPost[]>("list_posts");
  return snapshot();
}

export async function upsertPost(post: LibraryPost): Promise<LibraryPost[]> {
  if (isTauri()) return invoke<LibraryPost[]>("upsert_post", { post });
  const i = fallback.findIndex((p) => p.id === post.id);
  fallback =
    i < 0
      ? [post, ...fallback]
      : fallback.map((p) => (p.id === post.id ? post : p));
  return snapshot();
}

export async function deletePost(id: string): Promise<LibraryPost[]> {
  if (isTauri()) return invoke<LibraryPost[]>("delete_post", { id });
  fallback = fallback.filter((p) => p.id !== id);
  return snapshot();
}
