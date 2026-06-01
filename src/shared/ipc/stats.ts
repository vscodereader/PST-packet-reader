import { invoke } from "@tauri-apps/api/core";

import type { DashStat } from "@/shared/bindings/DashStat";
import { STATS } from "@/shared/data/mock";

import { isTauri } from "./runtime";

export type { DashStat };

/**
 * Typed wrapper around the Rust `list_stats` command, with a mock fallback for
 * browser dev / Vitest. Read-only — the dashboard renders the stat tiles.
 * Mirrors the other IPC wrappers.
 */

let statsFallback: DashStat[] = STATS.map((s) => ({ ...s }));

/** Reset the in-memory fallback to the pristine mock data. Test-only seam. */
export function resetStatsFallbackForTests(): void {
  statsFallback = STATS.map((s) => ({ ...s }));
}

export async function listStats(): Promise<DashStat[]> {
  if (isTauri()) return invoke<DashStat[]>("list_stats");
  return statsFallback.map((s) => ({ ...s }));
}
