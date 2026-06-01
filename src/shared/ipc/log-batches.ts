import { invoke } from "@tauri-apps/api/core";

import type { LogBatch } from "@/shared/bindings/LogBatch";

export type { LogBatch };

/** Typed wrapper around the Rust `list_log_batches` command over Tauri IPC. */
export async function listLogBatches(): Promise<LogBatch[]> {
  return invoke<LogBatch[]>("list_log_batches");
}
