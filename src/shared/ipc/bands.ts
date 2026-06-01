import { invoke } from "@tauri-apps/api/core";

import type { Band } from "@/shared/bindings/Band";

export type { Band };

/** Typed wrapper around the Rust `list_bands` command over Tauri IPC. */
export async function listBands(): Promise<Band[]> {
  return invoke<Band[]>("list_bands");
}
