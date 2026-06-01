import { invoke } from "@tauri-apps/api/core";

import type { Cafe } from "@/shared/bindings/Cafe";

export type { Cafe };

/** Typed wrapper around the Rust `list_cafes` command over Tauri IPC. */
export async function listCafes(): Promise<Cafe[]> {
  return invoke<Cafe[]>("list_cafes");
}
