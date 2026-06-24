import { describe, it, expect } from "vitest";

import {
  ACTIVE_PLATFORMS,
  isPostable,
  isProblemStatus,
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

  it("STATUS_ACCOUNT covers the login outcomes plus 'new' and 'waiting'", () => {
    expect(Object.keys(STATUS_ACCOUNT).sort()).toEqual(
      [
        "active",
        // 글 게시 성공 후 대기 상태(#267-3).
        "waiting",
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

  it("isProblemStatus flags the same set as backend stats (error/badCredentials/blocked)", () => {
    expect(isProblemStatus("error")).toBe(true);
    expect(isProblemStatus("badCredentials")).toBe(true);
    expect(isProblemStatus("blocked")).toBe(true);
    // challenge는 진행 중 단계, active/new는 정상 — 오류로 세지 않는다.
    expect(isProblemStatus("challenge")).toBe(false);
    expect(isProblemStatus("active")).toBe(false);
    expect(isProblemStatus("new")).toBe(false);
  });

  it("isPostable allows only active/new and blocks every login-failure status", () => {
    // 게시 모달의 모든 게이트(disabled/preselect/toggle/select-all/job 생성)가 이 헬퍼로
    // 통일돼 있다 — 실패 계열이 게시 위치·잡에 새지 않도록 하는 단일 진실.
    expect(isPostable("active")).toBe(true);
    expect(isPostable("new")).toBe(true);
    expect(isPostable("error")).toBe(false);
    expect(isPostable("badCredentials")).toBe(false);
    expect(isPostable("challenge")).toBe(false);
    expect(isPostable("blocked")).toBe(false);
  });
});
