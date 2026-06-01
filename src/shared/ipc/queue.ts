import { invoke } from "@tauri-apps/api/core";

import type { QueueNowItem } from "@/shared/bindings/QueueNowItem";
import type { QueueScheduledItem } from "@/shared/bindings/QueueScheduledItem";

export type { QueueNowItem, QueueScheduledItem };

/**
 * Typed wrapper around the Rust `queue` commands over Tauri IPC. Read + cancel
 * only (priority reordering stays client-side for this PoC). Mirrors the
 * accounts/posts wrappers.
 */

export async function listQueueNow(): Promise<QueueNowItem[]> {
  return invoke<QueueNowItem[]>("list_queue_now");
}

export async function listQueueScheduled(): Promise<QueueScheduledItem[]> {
  return invoke<QueueScheduledItem[]>("list_queue_scheduled");
}

export async function cancelQueueNow(id: string): Promise<QueueNowItem[]> {
  return invoke<QueueNowItem[]>("cancel_queue_now", { id });
}

export async function cancelQueueScheduled(
  id: string,
): Promise<QueueScheduledItem[]> {
  return invoke<QueueScheduledItem[]>("cancel_queue_scheduled", { id });
}
