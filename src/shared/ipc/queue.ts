import { invoke } from "@tauri-apps/api/core";

import type { QueueNowItem } from "@/shared/bindings/QueueNowItem";
import type { QueueScheduledItem } from "@/shared/bindings/QueueScheduledItem";
import { QUEUE_NOW, QUEUE_SCHEDULED } from "@/shared/data/mock";

import { isTauri } from "./runtime";

export type { QueueNowItem, QueueScheduledItem };

/**
 * Typed wrapper around the Rust `queue` commands, with a mock fallback for
 * browser dev / Vitest. Read + cancel only (priority reordering stays
 * client-side for this PoC). Mirrors the accounts/posts wrappers.
 */

let nowFallback: QueueNowItem[] = QUEUE_NOW.map((q) => ({ ...q }));
let schedFallback: QueueScheduledItem[] = QUEUE_SCHEDULED.map((q) => ({
  ...q,
}));

/** Reset the in-memory fallbacks to the pristine mock data. Test-only seam. */
export function resetQueueFallbackForTests(): void {
  nowFallback = QUEUE_NOW.map((q) => ({ ...q }));
  schedFallback = QUEUE_SCHEDULED.map((q) => ({ ...q }));
}

export async function listQueueNow(): Promise<QueueNowItem[]> {
  if (isTauri()) return invoke<QueueNowItem[]>("list_queue_now");
  return nowFallback.map((q) => ({ ...q }));
}

export async function listQueueScheduled(): Promise<QueueScheduledItem[]> {
  if (isTauri()) return invoke<QueueScheduledItem[]>("list_queue_scheduled");
  return schedFallback.map((q) => ({ ...q }));
}

export async function cancelQueueNow(id: string): Promise<QueueNowItem[]> {
  if (isTauri()) return invoke<QueueNowItem[]>("cancel_queue_now", { id });
  nowFallback = nowFallback.filter((q) => q.id !== id);
  return nowFallback.map((q) => ({ ...q }));
}

export async function cancelQueueScheduled(
  id: string,
): Promise<QueueScheduledItem[]> {
  if (isTauri())
    return invoke<QueueScheduledItem[]>("cancel_queue_scheduled", { id });
  schedFallback = schedFallback.filter((q) => q.id !== id);
  return schedFallback.map((q) => ({ ...q }));
}
