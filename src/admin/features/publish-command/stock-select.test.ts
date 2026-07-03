import { describe, expect, it } from "vitest";

import {
  isExcludedByName,
  pickStocks,
  type SelectableStock,
} from "./stock-select";

// 헬퍼: code=이름 간이 생성. hot=불꽃 여부.
function s(name: string, isHotDiscussion: boolean): SelectableStock {
  return { code: name, name, isHotDiscussion };
}

describe("isExcludedByName", () => {
  it("삼성전자·하이닉스 이름은 제외(삼성전자우 포함)", () => {
    expect(isExcludedByName("삼성전자")).toBe(true);
    expect(isExcludedByName("삼성전자우")).toBe(true);
    expect(isExcludedByName("SK하이닉스")).toBe(true);
    expect(isExcludedByName("하이닉스")).toBe(true);
  });
  it("그 외 종목은 제외 안 함", () => {
    expect(isExcludedByName("현대차")).toBe(false);
    expect(isExcludedByName("카카오")).toBe(false);
  });
});

describe("pickStocks", () => {
  it("정상: 불꽃으로 부족하면 비불꽃으로 위에서부터 채운다 (N=20, 불꽃 15)", () => {
    const list = [
      ...Array.from({ length: 15 }, (_, i) => s(`hot${i}`, true)),
      ...Array.from({ length: 20 }, (_, i) => s(`cold${i}`, false)),
    ];
    const r = pickStocks(list, 20);
    expect(r.error).toBeNull();
    expect(r.picked).toHaveLength(20);
    // 불꽃 15개 전부 + 비불꽃 위 5개.
    expect(r.picked.slice(0, 15).every((x) => x.isHotDiscussion)).toBe(true);
    expect(r.picked.slice(15).map((x) => x.code)).toEqual([
      "cold0",
      "cold1",
      "cold2",
      "cold3",
      "cold4",
    ]);
  });

  it("불꽃 초과: 불꽃 상위 N개만 (N=20, 불꽃 30)", () => {
    const list = Array.from({ length: 30 }, (_, i) => s(`hot${i}`, true));
    const r = pickStocks(list, 20);
    expect(r.error).toBeNull();
    expect(r.picked).toHaveLength(20);
    expect(r.picked[0]!.code).toBe("hot0");
    expect(r.picked[19]!.code).toBe("hot19");
  });

  it("N 초과: 가용 전부 선택 + 오류 문구 (N=100, 실종목 3)", () => {
    const list = [s("a", true), s("b", false), s("c", true)];
    const r = pickStocks(list, 100);
    expect(r.picked).toHaveLength(3);
    expect(r.error).toBe("선택 100, 실종목 3개, 선택불가 97개");
  });

  it("제외가 선택 이전에 적용된다 (삼성전자/하이닉스는 후보에서 빠짐)", () => {
    const list = [
      s("삼성전자", true),
      s("SK하이닉스", true),
      s("현대차", true),
      s("카카오", false),
    ];
    const r = pickStocks(list, 2);
    expect(r.error).toBeNull();
    expect(r.picked.map((x) => x.code)).toEqual(["현대차", "카카오"]);
  });
});
