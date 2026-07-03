import { describe, it, expect } from "vitest";

import {
  ACTIVE_PLATFORMS,
  isHiddenFromPublish,
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
  it("defines the seven platforms", () => {
    expect(PLATFORMS.map((p) => p.id)).toEqual([
      "forum",
      "naver",
      "blog",
      "clip",
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
        // 로그인 캡차 보류(#267 후속).
        "onHold",
        // 게시 대기초과(#286 후속).
        "timedOut",
        "badCredentials",
        // 세션 만료 → 재로그인 필요(2026-07-03). 재로그인하면 회복.
        "relogin",
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
    // 보류(캡차 미해결)도 로그인된 상태가 아니므로 게시 대상에서 제외(#267 후속).
    expect(isPostable("onHold")).toBe(false);
    // 대기초과(게시 실패, #286 후속)도 게시 대상에서 제외 — 체크박스 목록에 안 보여야 한다.
    expect(isPostable("timedOut")).toBe(false);
  });

  it("isHiddenFromPublish hides waiting/blocked/timedOut but not login-problem states", () => {
    // 사용자 지시: 대기뿐 아니라 차단·대기초과도 게시 계정 목록에서 아예 숨긴다(비활성 아님).
    expect(isHiddenFromPublish("waiting")).toBe(true);
    expect(isHiddenFromPublish("blocked")).toBe(true);
    expect(isHiddenFromPublish("timedOut")).toBe(true);
    // 로그인 문제/정상 계열은 숨기지 않는다 — 사용자가 보고 조치하거나(실패) 선택해야(정상) 한다.
    expect(isHiddenFromPublish("active")).toBe(false);
    expect(isHiddenFromPublish("new")).toBe(false);
    expect(isHiddenFromPublish("badCredentials")).toBe(false);
    expect(isHiddenFromPublish("challenge")).toBe(false);
    expect(isHiddenFromPublish("onHold")).toBe(false);
    expect(isHiddenFromPublish("error")).toBe(false);
  });
});
