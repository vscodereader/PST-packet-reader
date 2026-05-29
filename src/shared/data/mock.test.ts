import { describe, it, expect } from "vitest";

import {
  accountLog,
  acctPlatforms,
  batchStatus,
  hasToken,
  jobLink,
  resolveTemplate,
} from "./mock";
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

describe("batchStatus", () => {
  const base = (items: LogBatch["items"]): LogBatch => ({
    id: "x",
    title: "t",
    kind: "post",
    time: "now",
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
  it("returns distinct platforms for the given account ids", () => {
    expect(acctPlatforms(["a1", "a2", "a5"])).toEqual(["forum", "naver"]);
  });

  it("ignores unknown ids", () => {
    expect(acctPlatforms(["nope"])).toEqual([]);
  });
});

describe("accountLog", () => {
  const base: Account = {
    id: "x",
    platform: "forum",
    loginId: "u",
    pw: "p",
    status: "new",
    last: "—",
    tags: [],
  };

  it("returns no entries for a brand-new account", () => {
    expect(accountLog(base)).toEqual([]);
  });

  it("returns activity entries for an active account", () => {
    const log = accountLog({ ...base, status: "active" });
    expect(log.length).toBeGreaterThan(0);
    expect(log.every((e) => e.type !== "error")).toBe(true);
  });

  it("includes an error entry for an errored account", () => {
    const log = accountLog({ ...base, status: "error" });
    expect(log.some((e) => e.type === "error")).toBe(true);
  });
});
