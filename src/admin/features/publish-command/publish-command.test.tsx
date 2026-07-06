import { describe, expect, it } from "vitest";

import { maskId, postDisplay, shortTitle } from "./publish-command";

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
        postDisplay({ title: "제목 없음", kind: "comment", excerpt: "오늘 흐름 좋네요 👍" }),
      ).toBe("오늘 흐름 좋네요 👍");
    });
    it("댓글인데 내용이 비면 제목으로 폴백", () => {
      expect(postDisplay({ title: "제목 없음", kind: "comment", excerpt: "  " })).toBe(
        "제목 없음",
      );
      expect(postDisplay({ title: "제목 없음", kind: "comment" })).toBe("제목 없음");
    });
    it("글/글+댓글은 제목을 쓴다(내용 무시)", () => {
      expect(
        postDisplay({ title: "급등주 분석", kind: "post", excerpt: "본문 요약" }),
      ).toBe("급등주 분석");
      expect(
        postDisplay({ title: "모멘텀 글+댓글", kind: "both", excerpt: "댓글 내용" }),
      ).toBe("모멘텀 글+댓글");
    });
    it("kind 없으면 post로 보고 제목 사용(옛 하위 하위호환)", () => {
      expect(postDisplay({ title: "제목", excerpt: "내용" })).toBe("제목");
    });
  });
});
