import { invoke } from "@tauri-apps/api/core";

import type { DashStat } from "@/shared/bindings/DashStat";

export type { DashStat };

/** Typed wrapper around the Rust `list_stats` command over Tauri IPC. */
export async function listStats(): Promise<DashStat[]> {
  return invoke<DashStat[]>("list_stats");
}
