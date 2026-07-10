import { describe, expect, it } from "vitest";

import {
  canForumCommentDistribute,
  commentTargetPayload,
  filterAccountsByTarget,
  maskId,
  postDisplay,
  shortTitle,
} from "./publish-command";

// 렌더/네트워크 비의존 순수 헬퍼만 검증(다른 Admin 테스트와 동일 방침). 선택 알고리즘은
// stock-select.test.ts 참조.
describe("publish-command 헬퍼", () => {
  describe("maskId (§10-4-1: 앞 2글자 + •)", () => {
    it("앞 2글자만 남기고 나머지는 •", () => {
      expect(maskId("stock_id041")).toBe("st•••••••••");
    });
    it("2글자 이하는 그대로(• 없음)", () => {
      expect(maskId("ab")).toBe("ab");
      expect(maskId("a")).toBe("a");
    });
  });

  describe("shortTitle (6자 말줄임)", () => {
    it("6자 초과는 6자 + …", () => {
      expect(shortTitle("오늘의 급등주 분석과 전망")).toBe("오늘의 급등…");
    });
    it("6자 이하는 그대로", () => {
      expect(shortTitle("급등주")).toBe("급등주");
      expect(shortTitle("여섯글자입니")).toBe("여섯글자입니");
    });
  });

  describe("postDisplay (댓글=내용, 글=제목)", () => {
    it("댓글은 제목이 없어도 작성한 댓글 내용(excerpt)을 보여준다", () => {
      expect(
        postDisplay({
          title: "제목 없음",
          kind: "comment",
          excerpt: "오늘 흐름 좋네요 👍",
        }),
      ).toBe("오늘 흐름 좋네요 👍");
    });
    it("댓글인데 내용이 비면 제목으로 폴백", () => {
      expect(
        postDisplay({ title: "제목 없음", kind: "comment", excerpt: "  " }),
      ).toBe("제목 없음");
      expect(postDisplay({ title: "제목 없음", kind: "comment" })).toBe(
        "제목 없음",
      );
    });
    it("글/글+댓글은 제목을 쓴다(내용 무시)", () => {
      expect(
        postDisplay({
          title: "급등주 분석",
          kind: "post",
          excerpt: "본문 요약",
        }),
      ).toBe("급등주 분석");
      expect(
        postDisplay({
          title: "모멘텀 글+댓글",
          kind: "both",
          excerpt: "댓글 내용",
        }),
      ).toBe("모멘텀 글+댓글");
    });
    it("kind 없으면 post로 보고 제목 사용(옛 하위 하위호환)", () => {
      expect(postDisplay({ title: "제목", excerpt: "내용" })).toBe("제목");
    });
  });

  describe("commentTargetPayload (카페·밴드 댓글 대상 → 명령 페이로드)", () => {
    it("댓글 아닌 모드(글/글+댓글)는 대상 필드를 싣지 않는다", () => {
      expect(commentTargetPayload("post", "latest", 5)).toEqual({});
      expect(commentTargetPayload("both", "popular", 5)).toEqual({});
    });
    it("최신/인기는 mode + 개수(N)를 싣는다", () => {
      expect(commentTargetPayload("comment", "latest", 20)).toEqual({
        commentMode: "latest",
        commentCount: 20,
      });
      expect(commentTargetPayload("comment", "popular", 3)).toEqual({
        commentMode: "popular",
        commentCount: 3,
      });
    });
    it("특정글(url)은 mode만 싣고 개수는 싣지 않는다", () => {
      expect(commentTargetPayload("comment", "url", 20)).toEqual({
        commentMode: "url",
      });
    });
    it("개수는 최소 1로 보정(0·음수·소수 방어)", () => {
      expect(commentTargetPayload("comment", "latest", 0).commentCount).toBe(1);
      expect(commentTargetPayload("comment", "latest", -5).commentCount).toBe(
        1,
      );
      expect(commentTargetPayload("comment", "latest", 2.9).commentCount).toBe(
        2,
      );
    });
  });

  describe("filterAccountsByTarget (게시 대상별 platform 필터)", () => {
    const rows = [
      { loginId: "f1", platform: "forum", status: "active" },
      { loginId: "f2", platform: "forum", status: "waiting" },
      { loginId: "c1", platform: "naver", status: "active" },
      { loginId: "c2", platform: "naver", status: "new" },
      { loginId: "b1", platform: "blog", status: "active" },
      { loginId: "k1", platform: "clip", status: "active" },
      { loginId: "d1", platform: "band", status: "active" },
      { loginId: "old", status: "active" }, // platform 없음 → forum으로 본다
    ];
    it("종토는 platform=forum && active만 (카페·블로그 안 섞임)", () => {
      // f1(active)·old(platform없음=forum,active). f2는 waiting이라 제외.
      expect(filterAccountsByTarget("forum", rows)).toEqual(["f1", "old"]);
    });
    it("카페는 platform=naver 전부 (상태 무관 — 게시순간 로그인)", () => {
      expect(filterAccountsByTarget("cafe", rows)).toEqual(["c1", "c2"]);
    });
    it("블로그/클립/밴드는 자기 platform && active만", () => {
      expect(filterAccountsByTarget("blog", rows)).toEqual(["b1"]);
      expect(filterAccountsByTarget("clip", rows)).toEqual(["k1"]);
      expect(filterAccountsByTarget("band", rows)).toEqual(["d1"]);
    });
    it("rows 없거나 비면 null (호출부 더미 폴백)", () => {
      expect(filterAccountsByTarget("forum", undefined)).toBeNull();
      expect(filterAccountsByTarget("forum", [])).toBeNull();
    });
    it("종토로 로그인한 계정을 블로그로 바꾸면 종토에서 빠지고 블로그에 뜬다(#5)", () => {
      const changed = [{ loginId: "x", platform: "blog", status: "active" }];
      expect(filterAccountsByTarget("forum", changed)).toEqual([]);
      expect(filterAccountsByTarget("blog", changed)).toEqual(["x"]);
    });
  });

  describe("canForumCommentDistribute (#403: 댓글 나눠서 1:1 조건)", () => {
    it("댓글 수 == 계정 수이고 둘 다 1 이상이면 참(겹침 없는 1:1)", () => {
      expect(canForumCommentDistribute(3, 3)).toBe(true);
      expect(canForumCommentDistribute(1, 1)).toBe(true);
    });
    it("댓글 수 != 계정 수면 거짓", () => {
      expect(canForumCommentDistribute(2, 3)).toBe(false);
      expect(canForumCommentDistribute(3, 2)).toBe(false);
    });
    it("0이면 거짓(댓글·계정 중 하나라도 비면 분배 불가)", () => {
      expect(canForumCommentDistribute(0, 0)).toBe(false);
      expect(canForumCommentDistribute(0, 3)).toBe(false);
      expect(canForumCommentDistribute(3, 0)).toBe(false);
    });
  });
});
