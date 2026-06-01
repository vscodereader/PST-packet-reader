import { invoke } from "@tauri-apps/api/core";

import type { Scheduled } from "@/shared/bindings/Scheduled";
import { SCHEDULED } from "@/shared/data/mock";

import { isTauri } from "./runtime";

export type { Scheduled };

/**
 * Typed wrapper around the Rust `list_scheduled` command, with a mock fallback
 * for browser dev / Vitest. Read-only — the dashboard "게시 대기열" digest reads
 * from it. Distinct from the `queue` domain. Mirrors the other IPC wrappers.
 */

let scheduledFallback: Scheduled[] = SCHEDULED.map((s) => ({ ...s }));

/** Reset the in-memory fallback to the pristine mock data. Test-only seam. */
export function resetScheduledFallbackForTests(): void {
  scheduledFallback = SCHEDULED.map((s) => ({ ...s }));
}

export async function listScheduled(): Promise<Scheduled[]> {
  if (isTauri()) return invoke<Scheduled[]>("list_scheduled");
  return scheduledFallback.map((s) => ({ ...s }));
}
