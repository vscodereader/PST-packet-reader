import { invoke } from "@tauri-apps/api/core";

import type { ActivityItem } from "@/shared/bindings/ActivityItem";
import { ACTIVITY } from "@/shared/data/mock";

import { isTauri } from "./runtime";

export type { ActivityItem };

/**
 * Typed wrapper around the Rust `list_activity` command, with a mock fallback
 * for browser dev / Vitest. Read-only — the dashboard timeline and the
 * notifications "system" rows both read from it. Mirrors the other IPC wrappers.
 */

let activityFallback: ActivityItem[] = ACTIVITY.map((a) => ({ ...a }));

/** Reset the in-memory fallback to the pristine mock data. Test-only seam. */
export function resetActivityFallbackForTests(): void {
  activityFallback = ACTIVITY.map((a) => ({ ...a }));
}

export async function listActivity(): Promise<ActivityItem[]> {
  if (isTauri()) return invoke<ActivityItem[]>("list_activity");
  return activityFallback.map((a) => ({ ...a }));
}
