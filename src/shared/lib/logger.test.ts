import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";

import { logger } from "./logger";

describe("logger", () => {
  beforeEach(() => {
    vi.spyOn(console, "log").mockImplementation(() => {});
    vi.spyOn(console, "info").mockImplementation(() => {});
    vi.spyOn(console, "warn").mockImplementation(() => {});
    vi.spyOn(console, "error").mockImplementation(() => {});
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("routes debug to console.log with a prefixed level tag", () => {
    logger.debug("hello");
    expect(console.log).toHaveBeenCalledWith("[debug] hello");
  });

  it("routes info to console.info and forwards data", () => {
    logger.info("loaded", { count: 3 });
    expect(console.info).toHaveBeenCalledWith("[info] loaded", { count: 3 });
  });

  it("routes warn to console.warn", () => {
    logger.warn("careful");
    expect(console.warn).toHaveBeenCalledWith("[warn] careful");
  });

  it("routes error to console.error with the error object", () => {
    const err = new Error("boom");
    logger.error("threw", err);
    expect(console.error).toHaveBeenCalledWith("[error] threw", err);
  });
});
