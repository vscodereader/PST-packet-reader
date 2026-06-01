import { invoke } from "@tauri-apps/api/core";

import type { Stock } from "@/shared/bindings/Stock";

export type { Stock };

/** Typed wrapper around the Rust `list_stocks` command over Tauri IPC. */
export async function listStocks(): Promise<Stock[]> {
  return invoke<Stock[]>("list_stocks");
}
