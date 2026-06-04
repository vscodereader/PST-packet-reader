import { describe, expect, it } from "vitest";

import {
  acctPlatforms,
  batchStatus,
  dayBucket,
  formatRelative,
  hasToken,
  jobLink,
  resolveTemplate,
} from "./helpers";
import type { Account, LogBatch } from "./types";

describe("jobLink", () => {
  it("builds a Naver Finance URL from a stock code", () => {
    expect(jobLink({ code: "005930" })).toBe(
      "https://finance.naver.com/item/main.naver?code=005930",
    );
  });

  it("falls back to a provided url, then empty string", () => {
    expect(jobLink({ url: "https://example.com" })).toBe("https://example.com");
    expect(jobLink(null)).toBe("");
    expect(jobLink({})).toBe("");
  });
});

describe("resolveTemplate", () => {
  it("substitutes 종목명 / 종목코드 / 링크 for the job", () => {
    const out = resolveTemplate(
      "#{종목명}(#{종목코드}) → #{링크}",
      { targetName: "삼성전자", code: "005930" },
      undefined,
    );
    expect(out).toBe(
      "삼성전자(005930) → https://finance.naver.com/item/main.naver?code=005930",
    );
  });

  it("honors a link override", () => {
    const out = resolveTemplate("#{링크}", { code: "005930" }, "https://x.io");
    expect(out).toBe("https://x.io");
  });

  it("returns falsy text unchanged", () => {
    expect(resolveTemplate("", null)).toBe("");
  });

  it("leaves text without tokens untouched", () => {
    expect(resolveTemplate("plain", { code: "005930" })).toBe("plain");
  });
});

describe("hasToken", () => {
  it("detects any template token by default", () => {
    expect(hasToken("a #{링크} b")).toBe(true);
    expect(hasToken("no tokens")).toBe(false);
  });

  it("returns false for empty text", () => {
    expect(hasToken("")).toBe(false);
  });

  it("detects a specific token kind", () => {
    expect(hasToken("#{종목명}", "stock")).toBe(true);
    expect(hasToken("#{종목명}", "link")).toBe(false);
  });
});

describe("formatRelative", () => {
  const now = 1_700_000_000_000;
  it("shows 방금 within a minute", () => {
    expect(formatRelative(now - 30_000, now)).toBe("방금");
  });
  it("shows minutes then hours", () => {
    expect(formatRelative(now - 5 * 60_000, now)).toBe("5분 전");
    expect(formatRelative(now - 3 * 3_600_000, now)).toBe("3시간 전");
  });
});

describe("dayBucket", () => {
  const now = new Date("2026-06-04T10:00:00").getTime();
  it("buckets today / yesterday / older", () => {
    expect(dayBucket(new Date("2026-06-04T08:00:00").getTime(), now)).toBe(
      "오늘",
    );
    expect(dayBucket(new Date("2026-06-03T23:00:00").getTime(), now)).toBe(
      "어제",
    );
    expect(dayBucket(new Date("2026-06-01T09:00:00").getTime(), now)).toBe(
      "이전",
    );
  });
});

describe("batchStatus", () => {
  const base = (items: LogBatch["items"]): LogBatch => ({
    id: "x",
    title: "t",
    kind: "post",
    at: 1_700_000_000_000,
    items,
  });

  it("is running when any item is running/waiting", () => {
    expect(
      batchStatus(
        base([
          {
            platform: "forum",
            target: "t",
            loginId: "x",
            status: "running",
            msg: "",
          },
        ]),
      ),
    ).toBe("running");
  });

  it("is success when all items succeed", () => {
    expect(
      batchStatus(
        base([
          {
            platform: "forum",
            target: "t",
            loginId: "x",
            status: "success",
            msg: "",
          },
        ]),
      ),
    ).toBe("success");
  });

  it("is fail when all items fail, partial when mixed", () => {
    expect(
      batchStatus(
        base([
          {
            platform: "forum",
            target: "t",
            loginId: "x",
            status: "fail",
            msg: "",
          },
        ]),
      ),
    ).toBe("fail");
    expect(
      batchStatus(
        base([
          {
            platform: "forum",
            target: "t",
            loginId: "x",
            status: "fail",
            msg: "",
          },
          {
            platform: "forum",
            target: "u",
            loginId: "y",
            status: "success",
            msg: "",
          },
        ]),
      ),
    ).toBe("partial");
  });
});

describe("acctPlatforms", () => {
  const mk = (id: string, platform: Account["platform"]): Account => ({
    id,
    platform,
    loginId: id,
    pw: "p",
    status: "active",
    last: "—",
    tags: [],
  });
  const accounts: Account[] = [
    mk("a1", "forum"),
    mk("a2", "forum"),
    mk("a5", "naver"),
  ];

  it("returns distinct platforms for the given account ids", () => {
    expect(acctPlatforms(["a1", "a2", "a5"], accounts)).toEqual([
      "forum",
      "naver",
    ]);
  });

  it("ignores unknown ids", () => {
    expect(acctPlatforms(["nope"], accounts)).toEqual([]);
  });
});
