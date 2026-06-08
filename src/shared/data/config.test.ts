import { describe, it, expect } from "vitest";

import {
  ACTIVE_PLATFORMS,
  KIND,
  PLATFORM,
  PLATFORMS,
  STATUS_ACCOUNT,
  STATUS_ACCOUNT_CYCLE,
  STATUS_ACCOUNT_ORDER,
  STATUS_GUIDE,
} from "./config";

describe("platform config", () => {
  it("defines the five platforms", () => {
    expect(PLATFORMS.map((p) => p.id)).toEqual([
      "forum",
      "naver",
      "band",
      "instagram",
      "threads",
    ]);
  });

  it("PLATFORM indexes platforms by id", () => {
    expect(PLATFORM.forum?.name).toBe("종합토론방");
    expect(PLATFORM.naver?.color).toBe("naver");
  });

  it("ACTIVE_PLATFORMS is exactly the non-'soon' platforms", () => {
    expect(ACTIVE_PLATFORMS.every((p) => !p.soon)).toBe(true);
    expect(ACTIVE_PLATFORMS).toEqual(PLATFORMS.filter((p) => !p.soon));
  });
});

describe("label tables", () => {
  it("KIND covers post/comment/both", () => {
    expect(Object.keys(KIND).sort()).toEqual(["both", "comment", "post"]);
  });

  it("STATUS_ACCOUNT order matches its label map keys", () => {
    expect([...STATUS_ACCOUNT_ORDER].sort()).toEqual(
      Object.keys(STATUS_ACCOUNT).sort(),
    );
  });

  it("STATUS_ACCOUNT covers the five login outcomes plus 'new'", () => {
    expect(Object.keys(STATUS_ACCOUNT).sort()).toEqual(
      [
        "active",
        "badCredentials",
        "blocked",
        "challenge",
        "error",
        "new",
      ].sort(),
    );
  });

  it("STATUS_GUIDE has a guide line for every account status", () => {
    expect(Object.keys(STATUS_GUIDE).sort()).toEqual(
      Object.keys(STATUS_ACCOUNT).sort(),
    );
    for (const text of Object.values(STATUS_GUIDE)) {
      expect(text.length).toBeGreaterThan(0);
    }
  });

  it("STATUS_ACCOUNT_CYCLE is a subset of real statuses and excludes system-set ones", () => {
    // 수동 순환은 사용자 의미 상태만 — 자동 설정되는 비번오류/인증필요/에러는 제외.
    for (const s of STATUS_ACCOUNT_CYCLE) {
      expect(Object.keys(STATUS_ACCOUNT)).toContain(s);
    }
    expect(STATUS_ACCOUNT_CYCLE).not.toContain("badCredentials");
    expect(STATUS_ACCOUNT_CYCLE).not.toContain("challenge");
    expect(STATUS_ACCOUNT_CYCLE).not.toContain("error");
  });
});
