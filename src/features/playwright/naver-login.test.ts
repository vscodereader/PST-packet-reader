import { describe, expect, it } from "vitest";

import {
  buildCookieResult,
  hasNaverSessionCookies,
  validateInput,
} from "./naver-login.ts";

describe("naver-login sidecar helpers", () => {
  it("validates input json", () => {
    expect(() =>
      validateInput({
        accountId: "user1",
        id: "user1",
        password: "secret",
        cookiesPath: "cookies/user1.json",
      }),
    ).not.toThrow();

    expect(() => validateInput({ id: "user1" })).toThrow(/accountId/);
  });

  it("defaults headless to false and accepts headless mode", () => {
    expect(
      validateInput({
        accountId: "user1",
        id: "user1",
        password: "secret",
        cookiesPath: "cookies/user1.json",
      }).headless,
    ).toBe(false);

    expect(
      validateInput({
        accountId: "user1",
        id: "user1",
        password: "secret",
        cookiesPath: "cookies/user1.json",
        headless: true,
      }).headless,
    ).toBe(true);
  });

  it("detects required Naver session cookies", () => {
    expect(
      hasNaverSessionCookies([
        { name: "NID_AUT", domain: ".naver.com" },
        { name: "NID_SES", domain: ".naver.com" },
      ]),
    ).toBe(true);

    expect(
      hasNaverSessionCookies([{ name: "NID_AUT", domain: ".naver.com" }]),
    ).toBe(false);
  });

  it("builds account-scoped cookie output", () => {
    const result = buildCookieResult("user1", [
      { name: "NID_AUT", domain: ".naver.com" },
      { name: "other", domain: "example.com" },
    ]);

    expect(result.accountId).toBe("user1");
    expect(result.cookies).toHaveLength(1);
    expect(result.cookies[0]?.name).toBe("NID_AUT");
  });
});
