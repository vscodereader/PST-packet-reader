import { describe, it, expect } from "vitest";

import type { CommentPublishOutcome } from "@/shared/bindings/CommentPublishOutcome";

import {
  buildBothCommentJobs,
  buildUrlCommentJobs,
  commentSummary,
  commentsAllOk,
  distributeComments,
  mulberry32,
  parseCafeArticleUrl,
} from "./comment-jobs";

/**
 * Deterministic RNG stub yielding the given values in order (cycling). Lets a
 * test force an exact Fisher–Yates permutation, proving a builder actually
 * routes through the shuffle rather than dealing comments in input order.
 */
function seqRng(values: number[]): () => number {
  let i = 0;
  return () => values[i++ % values.length] ?? 0;
}

describe("mulberry32", () => {
  it("is deterministic for a fixed seed", () => {
    const a = mulberry32(123);
    const b = mulberry32(123);
    const seqA = [a(), a(), a(), a()];
    const seqB = [b(), b(), b(), b()];
    expect(seqA).toEqual(seqB);
  });

  it("yields values in [0, 1)", () => {
    const rng = mulberry32(42);
    for (let i = 0; i < 100; i++) {
      const v = rng();
      expect(v).toBeGreaterThanOrEqual(0);
      expect(v).toBeLessThan(1);
    }
  });

  it("produces different streams for different seeds", () => {
    expect(mulberry32(1)()).not.toBe(mulberry32(2)());
  });
});

describe("distributeComments", () => {
  it("assigns exactly one comment to every account", () => {
    const got = distributeComments(
      ["a1", "a2", "a3"],
      ["c1", "c2", "c3"],
      mulberry32(7),
    );
    expect(got.map((g) => g.accountId)).toEqual(["a1", "a2", "a3"]);
    expect(got.every((g) => ["c1", "c2", "c3"].includes(g.content))).toBe(true);
  });

  it("is deterministic for a fixed seed (same input → same output)", () => {
    const args = [
      ["a1", "a2", "a3", "a4", "a5"],
      ["c1", "c2", "c3", "c4", "c5"],
    ] as const;
    const first = distributeComments(args[0], args[1], mulberry32(99));
    const second = distributeComments(args[0], args[1], mulberry32(99));
    expect(first).toEqual(second);
  });

  it("actually shuffles (does not return comments in input order) for a seed", () => {
    // With this seed the assignment must differ from the trivial identity map,
    // proving the RNG drives a real permutation rather than a passthrough.
    const got = distributeComments(
      ["a1", "a2", "a3", "a4"],
      ["c1", "c2", "c3", "c4"],
      mulberry32(3),
    );
    const contents = got.map((g) => g.content);
    expect(contents).not.toEqual(["c1", "c2", "c3", "c4"]);
  });

  it("boundary: comments < accounts — reuses the pool, every account still gets one", () => {
    const got = distributeComments(
      ["a1", "a2", "a3", "a4", "a5"],
      ["c1", "c2"],
      mulberry32(11),
    );
    expect(got).toHaveLength(5);
    expect(got.map((g) => g.accountId)).toEqual(["a1", "a2", "a3", "a4", "a5"]);
    expect(got.every((g) => ["c1", "c2"].includes(g.content))).toBe(true);
    // both comments are used across the five accounts (pool is cycled)
    const used = new Set(got.map((g) => g.content));
    expect(used).toEqual(new Set(["c1", "c2"]));
  });

  it("boundary: comments > accounts — each account gets a distinct comment", () => {
    const got = distributeComments(
      ["a1", "a2"],
      ["c1", "c2", "c3", "c4", "c5"],
      mulberry32(5),
    );
    expect(got).toHaveLength(2);
    const used = got.map((g) => g.content);
    expect(new Set(used).size).toBe(2);
  });

  it("is empty when there are no accounts or no comments", () => {
    expect(distributeComments([], ["c1"], mulberry32(1))).toEqual([]);
    expect(distributeComments(["a1"], [], mulberry32(1))).toEqual([]);
  });

  it("single comment: every account gets that one comment", () => {
    const got = distributeComments(["a1", "a2"], ["only"], mulberry32(1));
    expect(got).toEqual([
      { accountId: "a1", content: "only" },
      { accountId: "a2", content: "only" },
    ]);
  });
});

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
  it("produces one randomly-distributed comment per posted article", () => {
    const posted = [
      { accountId: "a5", cafeId: 111, articleId: 1000 },
      { accountId: "a6", cafeId: 222, articleId: 1001 },
    ];
    const jobs = buildBothCommentJobs(posted, ["좋네요", "추가매수"], {
      rng: mulberry32(7),
    });
    // one job per posted article (not the old account × all-comments fan-out)
    expect(jobs).toHaveLength(2);
    expect(jobs[0]?.accountId).toBe("a5");
    expect(jobs[0]?.cafeId).toBe(111);
    expect(jobs[0]?.articleId).toBe(1000);
    expect(jobs[1]?.accountId).toBe("a6");
    expect(jobs[1]?.cafeId).toBe(222);
    expect(jobs[1]?.articleId).toBe(1001);
    expect(jobs.every((j) => ["좋네요", "추가매수"].includes(j.content))).toBe(
      true,
    );
  });

  it("is deterministic for a fixed seed", () => {
    const posted = [
      { accountId: "a5", cafeId: 111, articleId: 1000 },
      { accountId: "a6", cafeId: 222, articleId: 1001 },
      { accountId: "a7", cafeId: 333, articleId: 1002 },
    ];
    const comments = ["c1", "c2", "c3"];
    const first = buildBothCommentJobs(posted, comments, {
      rng: mulberry32(5),
    });
    const second = buildBothCommentJobs(posted, comments, {
      rng: mulberry32(5),
    });
    expect(first).toEqual(second);
  });

  it("routes through the shuffle (rng reverses the pool, not raw input order)", () => {
    // rng=[0] reverses a 2-element Fisher–Yates: pool ["x","y"] → ["y","x"].
    // So a5 must get "y" (shuffled), NOT "x" (raw input order) — this fails if a
    // regression drops distributeComments and deals comments in input order.
    const posted = [
      { accountId: "a5", cafeId: 111, articleId: 1000 },
      { accountId: "a6", cafeId: 222, articleId: 1001 },
    ];
    const jobs = buildBothCommentJobs(posted, ["x", "y"], { rng: seqRng([0]) });
    expect(jobs[0]?.content).toBe("y");
    expect(jobs[1]?.content).toBe("x");
  });

  it("is empty when there are no posts or no comments", () => {
    expect(buildBothCommentJobs([], ["x"])).toEqual([]);
    expect(
      buildBothCommentJobs([{ accountId: "a", cafeId: 1, articleId: 2 }], []),
    ).toEqual([]);
  });
});

describe("buildUrlCommentJobs", () => {
  it("produces one randomly-distributed comment per account at the fixed target", () => {
    const jobs = buildUrlCommentJobs(
      ["a5", "a10"],
      { cafeId: 31732304, articleId: 9 },
      ["댓글1", "댓글2", "댓글3"],
      { rng: mulberry32(7) },
    );
    expect(jobs).toHaveLength(2);
    expect(jobs.every((j) => j.cafeId === 31732304 && j.articleId === 9)).toBe(
      true,
    );
    expect(jobs[0]?.accountId).toBe("a5");
    expect(jobs[1]?.accountId).toBe("a10");
    expect(
      jobs.every((j) => ["댓글1", "댓글2", "댓글3"].includes(j.content)),
    ).toBe(true);
  });

  it("is deterministic for a fixed seed", () => {
    const accounts = ["a5", "a10", "a15"];
    const target = { cafeId: 1, articleId: 2 };
    const comments = ["c1", "c2", "c3"];
    const first = buildUrlCommentJobs(accounts, target, comments, {
      rng: mulberry32(42),
    });
    const second = buildUrlCommentJobs(accounts, target, comments, {
      rng: mulberry32(42),
    });
    expect(first).toEqual(second);
  });

  it("routes through the shuffle (rng reverses the pool, not raw input order)", () => {
    // rng=[0, 0.5] reverses a 3-element Fisher–Yates: ["c1","c2","c3"] → ["c3","c2","c1"].
    // a5 must get "c3" (shuffled), NOT "c1" (raw input order) — guards against a
    // regression that bypasses distributeComments and assigns comments[i] directly.
    const jobs = buildUrlCommentJobs(
      ["a5", "a10"],
      { cafeId: 31732304, articleId: 9 },
      ["c1", "c2", "c3"],
      { rng: seqRng([0, 0.5]) },
    );
    expect(jobs[0]?.content).toBe("c3");
    expect(jobs[1]?.content).toBe("c2");
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
