/**
 * Detects whether the app is running inside the Tauri webview, as opposed to a
 * plain browser dev server or the jsdom test environment.
 *
 * Tauri v2 injects `__TAURI_INTERNALS__` onto `window`. When it's absent the IPC
 * wrappers fall back to the in-memory mock data so `pnpm dev` and Vitest keep
 * working without a Rust backend.
 */
export function isTauri(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}
