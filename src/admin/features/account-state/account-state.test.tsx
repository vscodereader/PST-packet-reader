import { describe, expect, it } from "vitest";

import {
  diffAccountRows,
  isReversibleStatus,
  statusLabel,
  type AccountRow,
} from "./account-state";

// 렌더/네트워크 비의존 순수 헬퍼만 검증(다른 Admin 테스트와 동일 방침).
describe("account-state 헬퍼", () => {
  describe("diffAccountRows (§2 저장: 바뀐 행만)", () => {
    const original: AccountRow[] = [
      { loginId: "a", platform: "forum", status: "waiting" },
      { loginId: "b", platform: "blog", status: "active" },
      { loginId: "c", platform: "clip", status: "onHold" },
    ];

    it("바뀐 게 없으면 빈 배열", () => {
      expect(diffAccountRows(original, original)).toEqual([]);
    });

    it("플랫폼만 바뀐 행은 platform만 담는다", () => {
      const edited: AccountRow[] = [
        { loginId: "a", platform: "blog", status: "waiting" },
        ...original.slice(1),
      ];
      expect(diffAccountRows(original, edited)).toEqual([
        { loginId: "a", platform: "blog" },
      ]);
    });

    it("상태만 바뀐 행은 status만 담는다", () => {
      const edited: AccountRow[] = [
        original[0]!,
        { loginId: "b", platform: "blog", status: "waiting" },
        original[2]!,
      ];
      expect(diffAccountRows(original, edited)).toEqual([
        { loginId: "b", status: "waiting" },
      ]);
    });

    it("둘 다 바뀌면 둘 다 담고, 여러 행을 모은다", () => {
      const edited: AccountRow[] = [
        { loginId: "a", platform: "naver", status: "active" },
        original[1]!,
        { loginId: "c", platform: "clip", status: "active" },
      ];
      expect(diffAccountRows(original, edited)).toEqual([
        { loginId: "a", platform: "naver", status: "active" },
        { loginId: "c", status: "active" },
      ]);
    });

    it("original에 없는 loginId는 무시한다", () => {
      const edited: AccountRow[] = [
        ...original,
        { loginId: "ghost", platform: "band", status: "active" },
      ];
      expect(diffAccountRows(original, edited)).toEqual([]);
    });
  });

  describe("isReversibleStatus (§5: 사람이 되돌릴 수 있는 3종)", () => {
    it("active/waiting/onHold만 true", () => {
      expect(isReversibleStatus("active")).toBe(true);
      expect(isReversibleStatus("waiting")).toBe(true);
      expect(isReversibleStatus("onHold")).toBe(true);
    });
    it("워커 판정값은 false(드롭다운 제외)", () => {
      expect(isReversibleStatus("blocked")).toBe(false);
      expect(isReversibleStatus("badCredentials")).toBe(false);
      expect(isReversibleStatus("timedOut")).toBe(false);
    });
  });

  describe("statusLabel", () => {
    it("알려진 상태는 한글 라벨", () => {
      expect(statusLabel("onHold")).toBe("보류");
      expect(statusLabel("blocked")).toBe("차단");
    });
    it("미상은 원문 그대로", () => {
      expect(statusLabel("mystery")).toBe("mystery");
    });
  });
});
