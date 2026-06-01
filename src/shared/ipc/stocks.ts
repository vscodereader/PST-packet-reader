import { invoke } from "@tauri-apps/api/core";

import type { Stock } from "@/shared/bindings/Stock";
import { STOCKS } from "@/shared/data/mock";

import { isTauri } from "./runtime";

export type { Stock };

/**
 * Typed wrapper around the Rust `list_stocks` command, with a mock fallback for
 * browser dev / Vitest. Read-only — the stock list is crawled server-side and
 * the UI only picks targets by `code`. Mirrors the other IPC wrappers.
 */

let stocksFallback: Stock[] = STOCKS.map((s) => ({ ...s }));

/** Reset the in-memory fallback to the pristine mock data. Test-only seam. */
export function resetStocksFallbackForTests(): void {
  stocksFallback = STOCKS.map((s) => ({ ...s }));
}

export async function listStocks(): Promise<Stock[]> {
  if (isTauri()) return invoke<Stock[]>("list_stocks");
  return stocksFallback.map((s) => ({ ...s }));
}
