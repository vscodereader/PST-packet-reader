import { invoke } from "@tauri-apps/api/core";

import type { ActivityItem } from "@/shared/bindings/ActivityItem";

export type { ActivityItem };

/** Typed wrapper around the Rust `list_activity` command over Tauri IPC. */
export async function listActivity(): Promise<ActivityItem[]> {
  return invoke<ActivityItem[]>("list_activity");
}
