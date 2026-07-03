import { describe, expect, it } from "vitest";

import { maskId, shortTitle } from "./publish-command";

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
});
