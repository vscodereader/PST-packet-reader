import { describe, expect, it } from "vitest";

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
  });
});
