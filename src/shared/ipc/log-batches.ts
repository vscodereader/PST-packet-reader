import { invoke } from "@tauri-apps/api/core";

import type { LogBatch } from "@/shared/bindings/LogBatch";
import { LOG_BATCHES } from "@/shared/data/mock";

import { isTauri } from "./runtime";

export type { LogBatch };

/**
 * Typed wrapper around the Rust `list_log_batches` command, with a mock fallback
 * for browser dev / Vitest. Read-only — the notifications screen renders the
 * batches; `batchStatus`/grouping stay client-side. Mirrors the other IPC
 * wrappers.
 */

let logBatchesFallback: LogBatch[] = LOG_BATCHES.map((b) => ({ ...b }));

/** Reset the in-memory fallback to the pristine mock data. Test-only seam. */
export function resetLogBatchesFallbackForTests(): void {
  logBatchesFallback = LOG_BATCHES.map((b) => ({ ...b }));
}

export async function listLogBatches(): Promise<LogBatch[]> {
  if (isTauri()) return invoke<LogBatch[]>("list_log_batches");
  return logBatchesFallback.map((b) => ({ ...b }));
}
