import { invoke } from "@tauri-apps/api/core";

import type { Scheduled } from "@/shared/bindings/Scheduled";

export type { Scheduled };

/** Typed wrapper around the Rust `list_scheduled` command over Tauri IPC. */
export async function listScheduled(): Promise<Scheduled[]> {
  return invoke<Scheduled[]>("list_scheduled");
}
