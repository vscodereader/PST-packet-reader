import { describe, it, expect } from "vitest";

import type { CommentPublishOutcome } from "@/shared/bindings/CommentPublishOutcome";

import {
  buildBothCommentJobs,
  buildUrlCommentJobs,
  commentSummary,
  parseCafeArticleUrl,
} from "./comment-jobs";

describe("parseCafeArticleUrl", () => {
  it("parses the SPA cafes/{id}/articles/{aid} form", () => {
    expect(
      parseCafeArticleUrl(
        "https://cafe.naver.com/ca-fe/cafes/31732304/articles/9",
      ),
    ).toEqual({ cafeId: 31732304, articleId: 9 });
  });

  it("parses the legacy ArticleRead.nhn query form", () => {
    expect(
      parseCafeArticleUrl(
        "https://cafe.naver.com/ArticleRead.nhn?clubid=31732304&articleid=12345&page=1",
      ),
    ).toEqual({ cafeId: 31732304, articleId: 12345 });
  });

  it("returns null for non-article or empty input", () => {
    expect(
      parseCafeArticleUrl("https://cafe.naver.com/bluegrayoc3uc"),
    ).toBeNull();
    expect(parseCafeArticleUrl("")).toBeNull();
    expect(parseCafeArticleUrl(undefined)).toBeNull();
  });
});

describe("buildBothCommentJobs", () => {
  it("produces one job per (posted article × comment)", () => {
    const jobs = buildBothCommentJobs(
      [
        { accountId: "a5", cafeId: 111, articleId: 1000 },
        { accountId: "a6", cafeId: 222, articleId: 1001 },
      ],
      ["좋네요", "추가매수"],
    );
    expect(jobs).toHaveLength(4);
    expect(jobs[0]).toEqual({
      accountId: "a5",
      cafeId: 111,
      articleId: 1000,
      content: "좋네요",
    });
    expect(jobs[3]).toEqual({
      accountId: "a6",
      cafeId: 222,
      articleId: 1001,
      content: "추가매수",
    });
  });

  it("is empty when there are no posts or no comments", () => {
    expect(buildBothCommentJobs([], ["x"])).toEqual([]);
    expect(
      buildBothCommentJobs([{ accountId: "a", cafeId: 1, articleId: 2 }], []),
    ).toEqual([]);
  });
});

describe("buildUrlCommentJobs", () => {
  it("produces one job per (account × comment) at the fixed target", () => {
    const jobs = buildUrlCommentJobs(
      ["a5", "a10"],
      { cafeId: 31732304, articleId: 9 },
      ["댓글1", "댓글2", "댓글3"],
    );
    expect(jobs).toHaveLength(6);
    expect(jobs.every((j) => j.cafeId === 31732304 && j.articleId === 9)).toBe(
      true,
    );
    expect(jobs[0]?.accountId).toBe("a5");
    expect(jobs[5]?.accountId).toBe("a10");
  });
});

describe("commentSummary", () => {
  const out = (accountId: string, success: boolean): CommentPublishOutcome => ({
    accountId,
    cafeId: 1,
    articleId: 2,
    success,
  });

  it("counts this account's successes out of its total", () => {
    const outs = [out("a5", true), out("a5", false), out("a10", true)];
    expect(commentSummary(outs, "a5")).toBe("댓글 1/2건");
    expect(commentSummary(outs, "a10")).toBe("댓글 1/1건");
  });

  it("reads as 실패 when the whole call rejected", () => {
    expect(commentSummary(null, "a5")).toBe("댓글 실패");
  });

  it("reads as 없음 when the account has no outcomes", () => {
    expect(commentSummary([out("a10", true)], "a5")).toBe("댓글 없음");
  });
});
