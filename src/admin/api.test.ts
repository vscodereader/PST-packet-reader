import { afterEach, describe, expect, it, vi } from "vitest";

import { ApiError, OfflineError, api, isOffline } from "./api";

// 순수 헬퍼/형태만 검증한다(네트워크·localStorage 비의존 — 불필요한 통합 테스트는 하지 않음).
describe("admin api client", () => {
  it("isOffline은 OfflineError에만 true", () => {
    expect(isOffline(new OfflineError("x"))).toBe(true);
    expect(isOffline(new ApiError("x"))).toBe(false);
    expect(isOffline(new Error("x"))).toBe(false);
    expect(isOffline(null)).toBe(false);
  });

  it("baseUrl은 문자열이고 끝에 슬래시가 없다", () => {
    expect(typeof api.baseUrl).toBe("string");
    expect(api.baseUrl.endsWith("/")).toBe(false);
  });

  it("주요 엔드포인트 함수가 노출된다", () => {
    expect(typeof api.auth.login).toBe("function");
    expect(typeof api.operators.resetPassword).toBe("function");
    expect(typeof api.devices.issueCode).toBe("function");
    expect(typeof api.accounts.distribute).toBe("function");
    expect(typeof api.audit.list).toBe("function");
    // 07-게시명령 배선(1 게시명령 · 2 종목프록시 · 3 인벤토리 · 4 예약).
    expect(typeof api.forumStocks.list).toBe("function");
    expect(typeof api.publish.send).toBe("function");
    expect(typeof api.devices.inventory).toBe("function");
    expect(typeof api.scheduled.create).toBe("function");
    expect(typeof api.scheduled.list).toBe("function");
    expect(typeof api.scheduled.remove).toBe("function");
  });
});

// 종목 프록시(2단계) — 실제 fetch를 stub해 요청 URL/헤더 계약을 검증한다.
describe("forumStocks.list 요청 계약", () => {
  afterEach(() => {
    vi.unstubAllGlobals();
    localStorage.clear();
  });

  // fetch 시그니처를 명시해 mock.calls가 [url, init] 튜플로 타입되게 한다(noUncheckedIndexedAccess).
  const stubOkFetch = () => {
    const fetchMock = vi.fn(
      (_url: RequestInfo | URL, _init?: RequestInit): Promise<Response> =>
        Promise.resolve(
          new Response(
            JSON.stringify({
              stocks: [],
              totalCount: 0,
              page: 1,
              hasNext: false,
            }),
            { status: 200, headers: { "Content-Type": "application/json" } },
          ),
        ),
    );
    vi.stubGlobal("fetch", fetchMock);
    return fetchMock;
  };

  it("쿼리스트링을 정확히 만들고 Bearer 토큰을 싣는다", async () => {
    localStorage.setItem("pstmacro.admin.token", "tok-123");
    const fetchMock = stubOkFetch();

    const res = await api.forumStocks.list({
      category: "discussion",
      exchange: "krx",
      market: "kospi",
      page: 2,
    });
    expect(res.totalCount).toBe(0);

    const call = fetchMock.mock.calls[0]!;
    expect(String(call[0])).toContain(
      "/admin/forum-stocks?category=discussion&exchange=krx&market=kospi&page=2",
    );
    const headers = call[1]!.headers as Record<string, string>;
    expect(headers["Authorization"]).toBe("Bearer tok-123");
  });

  it("선택 파라미터는 생략하면 쿼리에도 빠진다(category만 필수)", async () => {
    const fetchMock = stubOkFetch();

    await api.forumStocks.list({ category: "tradingValue" });
    const url = String(fetchMock.mock.calls[0]![0]);
    expect(url).toContain("/admin/forum-stocks?category=tradingValue");
    expect(url).not.toContain("exchange=");
    expect(url).not.toContain("market=");
    expect(url).not.toContain("page=");
  });

  it("서버 미기동(fetch reject) → OfflineError로 던진다(미리보기 더미 폴백 신호)", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn(async () => {
        throw new TypeError("Failed to fetch");
      }),
    );
    await expect(
      api.forumStocks.list({ category: "discussion" }),
    ).rejects.toBeInstanceOf(OfflineError);
  });
});
