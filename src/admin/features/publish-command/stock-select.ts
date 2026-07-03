// 게시 명령(종토) — 종목 선택 알고리즘(설계서 07 §4-4·§4-5의 신규 로직).
//
// 규칙(요구 그대로):
// 0. 이름에 "삼성전자"·"하이닉스"가 들어간 종목은 불꽃 유무와 무관하게 **후보에서 제외**.
// 1. 불꽃🔥 종목을 목록 순서대로 먼저 채운다.
// 2. 불꽃으로 N을 못 채우면, 나머지는 불꽃 없는 종목 중 맨 위에서부터 순차로 채운다.
// 3. 불꽃 개수 ≥ N 이면, 불꽃 종목 중 상위 N개만.
// 4. N > 가용 종목수 M 이면, 가용 M개 전부 선택 + 오류 문구.
//
// 목록 순서 = 네이버 정렬 순서 그대로(입력 배열 순서를 보존).

/** 선택 입력에 필요한 최소 필드(ForumStock의 부분집합). */
export interface SelectableStock {
  code: string;
  name: string;
  isHotDiscussion: boolean;
}

/** 이름 제외 토큰 — "삼성전자"·"하이닉스"(사용자 확정 2026-07-03). "삼성전자"는 "삼성전자우"도 포함. */
export const EXCLUDE_NAME_TOKENS = ["삼성전자", "하이닉스"] as const;

/** 종목 이름이 제외 대상(삼성전자/하이닉스 포함)인지. */
export function isExcludedByName(name: string): boolean {
  return EXCLUDE_NAME_TOKENS.some((token) => name.includes(token));
}

export interface PickResult<T> {
  /** 선택된 종목(목록 순서 보존). */
  picked: T[];
  /** N이 실제 가용 종목수를 넘어서면 안내 문구, 아니면 null. */
  error: string | null;
}

/**
 * 불꽃 우선 + 상위 N 선택. `list`는 네이버 정렬 순서 그대로 넘긴다(제외 전 원본).
 * 삼성전자/하이닉스는 여기서 먼저 걸러낸 뒤 규칙을 적용한다.
 */
export function pickStocks<T extends SelectableStock>(
  list: T[],
  n: number,
): PickResult<T> {
  const candidates = list.filter((s) => !isExcludedByName(s.name));
  const m = candidates.length;

  // 4. N > 가용 M → 전부 + 오류 문구.
  if (n > m) {
    return {
      picked: candidates,
      error: `선택 ${n}, 실종목 ${m}개, 선택불가 ${n - m}개`,
    };
  }

  const hot = candidates.filter((s) => s.isHotDiscussion);
  if (hot.length >= n) {
    // 3. 불꽃이 N 이상 → 불꽃 상위 N개만.
    return { picked: hot.slice(0, n), error: null };
  }
  // 1·2. 불꽃 전부 + 부족분은 비불꽃 위에서부터.
  const cold = candidates.filter((s) => !s.isHotDiscussion);
  return { picked: [...hot, ...cold.slice(0, n - hot.length)], error: null };
}
