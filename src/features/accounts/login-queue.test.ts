import { describe, expect, it } from "vitest";

import type { Account } from "@/shared/data/types";

import { buildLoginNowItem, isSelectiveLoginPlatform } from "./login-queue";

function acct(over: Partial<Account>): Account {
  return {
    id: over.id ?? "a1",
    platform: over.platform ?? "naver",
    loginId: over.loginId ?? "user01",
    pw: over.pw ?? "pw",
    status: over.status ?? "new",
    last: "—",
    tags: [],
    ...over,
  };
}

describe("buildLoginNowItem", () => {
  it("로그인 전용 아이템으로 묶고 게시 필드는 비운다", () => {
    const item = buildLoginNowItem(
      [acct({ id: "a1", loginId: "naver01", platform: "naver" })],
      "id-1",
    );
    expect(item.id).toBe("id-1");
    expect(item.state).toBe("waiting");
    expect(item.title).toBe("계정 로그인 1건");
    // 게시 페이로드는 비어 있고 login만 채워진다.
    expect(item.plan?.naver).toEqual([]);
    expect(item.plan?.forum).toEqual([]);
    expect(item.plan?.band).toEqual([]);
    expect(item.plan?.login).toHaveLength(1);
  });

  it("platform에 따라 naver/band로 분기하고 useAdb를 정한다", () => {
    const item = buildLoginNowItem(
      [
        acct({ id: "a1", loginId: "naver01", platform: "naver" }),
        acct({ id: "a2", loginId: "forum01", platform: "forum" }),
        acct({ id: "a3", loginId: "band01", platform: "band" }),
      ],
      "id-2",
    );
    const login = item.plan?.login ?? [];
    expect(login).toHaveLength(3);
    // 네이버/종토방 → naver 로그인. 밴드 → band 로그인. 셋 다 useAdb true(모바일 IP 로테이션,
    // #210 — 밴드도 네이버와 동일하게 ADB IP 회전으로 봇탐지/캡차 완화).
    expect(login[0]).toMatchObject({
      accountId: "naver01",
      platform: "naver",
      useAdb: true,
    });
    expect(login[1]).toMatchObject({
      accountId: "forum01",
      platform: "naver",
      useAdb: true,
    });
    expect(login[2]).toMatchObject({
      accountId: "band01",
      platform: "band",
      useAdb: true,
    });
    // 명시적 선택 로그인은 항상 force=true(죽은 쿠키 덮어쓰기, #132).
    expect(login.every((l) => l.force && !l.headless)).toBe(true);
  });

  it("locs를 계정별로 채워 큐 카드에 표시되게 한다", () => {
    const item = buildLoginNowItem(
      [acct({ loginId: "band01", platform: "band" })],
      "id-3",
    );
    expect(item.locs).toEqual([{ p: "band", name: "band01" }]);
  });

  it("블로그 계정도 종토방처럼 naver 로그인으로 묶는다", () => {
    // 네이버블로그(blog)는 네이버 쿠키 기반이라 종토방과 동일하게 naver 로그인을 쓴다(선택 로그인).
    const item = buildLoginNowItem(
      [acct({ id: "b1", loginId: "blog01", platform: "blog" })],
      "id-blog",
    );
    expect(item.plan?.login?.[0]).toMatchObject({
      accountId: "blog01",
      platform: "naver",
      useAdb: true,
      force: true,
    });
    // 큐 카드에는 원래 플랫폼(blog)으로 표시된다.
    expect(item.locs).toEqual([{ p: "blog", name: "blog01" }]);
  });
});

describe("isSelectiveLoginPlatform", () => {
  it("종토방·블로그·클립·밴드는 선택 로그인을 허용하고 카페만 막는다", () => {
    // 네이버 쿠키 플랫폼과 BAND 이메일 로그인은 명시적 선택 로그인을 쓴다.
    expect(isSelectiveLoginPlatform("forum")).toBe(true);
    expect(isSelectiveLoginPlatform("blog")).toBe(true);
    expect(isSelectiveLoginPlatform("clip")).toBe(true);
    expect(isSelectiveLoginPlatform("band")).toBe(true);
    // 카페(naver)는 게시 직전 백엔드가 로그인을 원자 처리하므로 선택 로그인 제외.
    expect(isSelectiveLoginPlatform("naver")).toBe(false);
  });
});
