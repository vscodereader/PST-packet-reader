type Level = "debug" | "info" | "warn" | "error";

/**
 * Minimal level-routed logger. Calls go to the browser console today;
 * a follow-up can pipe `warn`/`error` into Rust `tracing` via a Tauri
 * command without changing call sites.
 */
export const logger = {
  debug: (message: string, data?: unknown) => emit("debug", message, data),
  info: (message: string, data?: unknown) => emit("info", message, data),
  warn: (message: string, data?: unknown) => emit("warn", message, data),
  error: (message: string, data?: unknown) => emit("error", message, data),
};

function emit(level: Level, message: string, data?: unknown): void {
  const method = level === "debug" ? "log" : level;
  if (data === undefined) {
    console[method](`[${level}] ${message}`);
  } else {
    console[method](`[${level}] ${message}`, data);
  }
}
