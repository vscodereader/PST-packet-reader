import { describe, it, expect } from "vitest";

import type { CommentPublishOutcome } from "@/shared/bindings/CommentPublishOutcome";

import {
  buildArticleListCommentJobs,
  buildBothCommentJobs,
  buildUrlCommentJobs,
  commentSummary,
  commentsAllOk,
  parseCafeArticleUrl,
  topNArticles,
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

  it("parses the real copied URL with a double-encoded iframe_url_utf8 param", () => {
    // What a user actually gets when copying a cafe post URL: the article ref is
    // URL-encoded (here doubly) inside iframe_url_utf8.
    expect(
      parseCafeArticleUrl(
        "https://cafe.naver.com/bluegrayoc3uc?iframe_url_utf8=%2FArticleRead.nhn%253Fclubid%3D31732304%2526articleid%3D9%2526referrerAllArticles%3Dtrue",
      ),
    ).toEqual({ cafeId: 31732304, articleId: 9 });
  });

  it("ignores substring params (relatedarticleid/subclubid) and picks the real ids", () => {
    // Unanchored regexes would match the trailing 'articleid'/'clubid' of these
    // params first and extract 999/7 instead of the real ids.
    expect(
      parseCafeArticleUrl(
        "https://cafe.naver.com/ArticleRead.nhn?relatedarticleid=999&subclubid=7&clubid=31732304&articleid=12345",
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

describe("topNArticles", () => {
  const article = (articleId: number) => ({ articleId });

  it("takes the first N articles in list order", () => {
    const arts = [article(1), article(2), article(3), article(4), article(5)];
    expect(topNArticles(arts, 3).map((a) => a.articleId)).toEqual([1, 2, 3]);
  });

  it("returns only what's available when the list is shorter than N", () => {
    const arts = [article(1), article(2)];
    expect(topNArticles(arts, 5).map((a) => a.articleId)).toEqual([1, 2]);
  });

  it("returns an empty list for N <= 0 or an empty source", () => {
    expect(topNArticles([article(1)], 0)).toEqual([]);
    expect(topNArticles([], 5)).toEqual([]);
  });
});

describe("buildArticleListCommentJobs", () => {
  const article = (articleId: number) => ({ articleId });

  it("produces one job per (top-N article × comment) for each account", () => {
    const jobs = buildArticleListCommentJobs(
      ["a5", "a10"],
      111,
      [article(1000), article(1001), article(1002)],
      2,
      ["좋네요", "추가매수"],
    );
    // 2 accounts × 2 articles × 2 comments = 8 jobs
    expect(jobs).toHaveLength(8);
    expect(jobs[0]).toEqual({
      accountId: "a5",
      cafeId: 111,
      articleId: 1000,
      content: "좋네요",
    });
    expect(jobs.every((j) => j.cafeId === 111)).toBe(true);
    // every targeted article is one of the top-2
    expect(new Set(jobs.map((j) => j.articleId))).toEqual(
      new Set([1000, 1001]),
    );
  });

  it("falls back to available articles when fewer than N exist", () => {
    const jobs = buildArticleListCommentJobs(["a5"], 222, [article(2000)], 5, [
      "댓글",
    ]);
    // only 1 article available though N=5 → 1 account × 1 article × 1 comment
    expect(jobs).toHaveLength(1);
    expect(jobs[0]).toEqual({
      accountId: "a5",
      cafeId: 222,
      articleId: 2000,
      content: "댓글",
    });
  });

  it("is empty when there are no articles, accounts, or comments", () => {
    expect(buildArticleListCommentJobs([], 1, [article(1)], 3, ["x"])).toEqual(
      [],
    );
    expect(buildArticleListCommentJobs(["a"], 1, [], 3, ["x"])).toEqual([]);
    expect(buildArticleListCommentJobs(["a"], 1, [article(1)], 3, [])).toEqual(
      [],
    );
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

describe("commentsAllOk", () => {
  const out = (accountId: string, success: boolean): CommentPublishOutcome => ({
    accountId,
    cafeId: 1,
    articleId: 2,
    success,
  });

  it("is true only when all of this account's comments succeeded", () => {
    expect(commentsAllOk([out("a5", true), out("a5", true)], "a5")).toBe(true);
  });

  it("is false when any of this account's comments failed", () => {
    // The 글+댓글 regression: post landed but a comment failed — must not be ok.
    expect(commentsAllOk([out("a5", true), out("a5", false)], "a5")).toBe(
      false,
    );
  });

  it("is false when the whole call rejected (null)", () => {
    expect(commentsAllOk(null, "a5")).toBe(false);
  });

  it("is false when the account has no outcomes", () => {
    expect(commentsAllOk([out("a10", true)], "a5")).toBe(false);
  });
});
