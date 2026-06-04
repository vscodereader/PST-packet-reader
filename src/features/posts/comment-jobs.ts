import type { CommentJob } from "@/shared/bindings/CommentJob";
import type { CommentPublishOutcome } from "@/shared/bindings/CommentPublishOutcome";

/** A resolved numeric comment target — the cafe + article to comment on. */
export interface CommentArticleTarget {
  cafeId: number;
  articleId: number;
}

/** A pseudo-random source: a function returning a float in `[0, 1)`. */
export type Rng = () => number;

/**
 * `mulberry32` — a tiny, fast 32-bit seeded PRNG.
 *
 * Given the same numeric `seed` it always produces the same stream, which makes
 * the comment distribution reproducible in tests. We avoid `Math.random` inside
 * the builders precisely so a fixed seed yields a deterministic assignment;
 * production callers can seed with `Date.now()` (the default below).
 */
export function mulberry32(seed: number): Rng {
  let a = seed >>> 0;
  return () => {
    a |= 0;
    a = (a + 0x6d2b79f5) | 0;
    let t = Math.imul(a ^ (a >>> 15), 1 | a);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

/** Options shared by every comment-job builder. */
export interface DistributeOptions {
  /**
   * The RNG driving the random comment↔account assignment. Defaults to a
   * `mulberry32` seeded from `Date.now()` so production output varies per run;
   * pass a fixed-seed `mulberry32(n)` for deterministic tests.
   */
  rng?: Rng;
}

/** One account paired with the single comment randomly dealt to it. */
export interface CommentAssignment {
  accountId: string;
  content: string;
}

/**
 * Fisher–Yates shuffle of a *copy* of `items`, driven by the injected `rng`.
 * Pure w.r.t. the input array; the result order is fully determined by `rng`.
 */
function shuffle<T>(items: readonly T[], rng: Rng): T[] {
  const out = items.slice();
  for (let i = out.length - 1; i > 0; i--) {
    const j = Math.floor(rng() * (i + 1));
    // both indices are in-bounds (0..i), but noUncheckedIndexedAccess still
    // types these as T | undefined — capture then guard before swapping.
    const a = out[i];
    const b = out[j];
    if (a !== undefined && b !== undefined) {
      out[i] = b;
      out[j] = a;
    }
  }
  return out;
}

/**
 * Randomly assign **one** comment to each account so different accounts post
 * different comments (the UX promise: "계정마다 다른 댓글이 무작위로 게시돼").
 *
 * The comment pool is shuffled with the injected `rng` and dealt round-robin to
 * the accounts in order. This gives an even, varied spread in both boundary
 * cases:
 *   - **comments < accounts**: the shuffled pool is cycled, so every account
 *     still gets a comment and all comments get reused fairly.
 *   - **comments > accounts**: each account receives a distinct comment from
 *     the front of the shuffled pool (the surplus comments simply go unused).
 *
 * Returns `[]` when either list is empty. Exported so issue #97's future
 * latest·popular builder can reuse the exact same distribution.
 */
export function distributeComments(
  accountIds: readonly string[],
  comments: readonly string[],
  rng: Rng,
): CommentAssignment[] {
  if (accountIds.length === 0 || comments.length === 0) return [];
  const pool = shuffle(comments, rng);
  return accountIds.map((accountId, i) => {
    // round-robin over the shuffled pool; modulo keeps us in-bounds, but the
    // indexed read is still T | undefined under noUncheckedIndexedAccess.
    const content = pool[i % pool.length] ?? comments[0] ?? "";
    return { accountId, content };
  });
}

/** Default production RNG: a fresh `mulberry32` seeded from the wall clock. */
function defaultRng(): Rng {
  return mulberry32(Date.now());
}

/**
 * Parse a naver cafe article URL into numeric `cafeId`/`articleId`.
 *
 * The real-world input is what a user gets by copying a cafe post URL: the
 * article ref (`ArticleRead.nhn?clubid={cafeId}&articleid={articleId}`) sits
 * URL-encoded — often *doubly* — inside an `iframe_url_utf8` query param, e.g.
 * `…/bluegrayoc3uc?iframe_url_utf8=%2FArticleRead.nhn%253Fclubid%3D31732304…`.
 * So we progressively `decodeURIComponent` the string and match each level for
 * `clubid`/`articleid`. The bare SPA form (`cafes/{id}/articles/{id}`, seen in
 * packet captures) is kept as a fallback. Returns `null` when no shape with
 * positive ids is found.
 */
export function parseCafeArticleUrl(
  url: string | undefined,
): CommentArticleTarget | null {
  if (!url) return null;

  // Build the raw string plus progressively-decoded versions (capped to guard
  // against pathological input). Decoding stops once it stabilizes or throws.
  const candidates: string[] = [url];
  let cur = url;
  for (let i = 0; i < 3; i++) {
    let decoded: string;
    try {
      decoded = decodeURIComponent(cur);
    } catch {
      break;
    }
    if (decoded === cur) break;
    candidates.push(decoded);
    cur = decoded;
  }

  for (const c of candidates) {
    // Copied-URL form (primary): clubid/articleid, once `=` is decoded. The `\b`
    // anchors stop substring params (relatedarticleid, subclubid, m_clubid) from
    // matching the trailing `clubid`/`articleid` and yielding the wrong ids.
    const club = c.match(/\bclubid=(\d+)/i);
    const art = c.match(/\barticleid=(\d+)/i);
    if (club && art) {
      const cafeId = Number(club[1]);
      const articleId = Number(art[1]);
      if (cafeId > 0 && articleId > 0) return { cafeId, articleId };
    }
    // SPA form (fallback): cafes/{id}/articles/{id}.
    const spa = c.match(/cafes\/(\d+)\/articles\/(\d+)/);
    if (spa) {
      const cafeId = Number(spa[1]);
      const articleId = Number(spa[2]);
      if (cafeId > 0 && articleId > 0) return { cafeId, articleId };
    }
  }

  return null;
}

/**
 * `both` mode: one comment job per successfully-posted article, each getting a
 * single **randomly-distributed** comment (see {@link distributeComments}) so
 * accounts don't all post the same text in the same order.
 *
 * `posted` is the subset of naver posts that succeeded — each carries the
 * account plus the cafe/article the comment should attach to.
 */
export function buildBothCommentJobs(
  posted: { accountId: string; cafeId: number; articleId: number }[],
  comments: string[],
  opts: DistributeOptions = {},
): CommentJob[] {
  if (posted.length === 0 || comments.length === 0) return [];
  const rng = opts.rng ?? defaultRng();
  const assigned = distributeComments(
    posted.map((p) => p.accountId),
    comments,
    rng,
  );
  return posted.map((p, i) => ({
    accountId: p.accountId,
    cafeId: p.cafeId,
    articleId: p.articleId,
    // distributeComments returns one assignment per account, same order/length
    // as `posted`; fall back defensively to keep the type non-undefined.
    content: assigned[i]?.content ?? comments[0] ?? "",
  }));
}

/**
 * `comment` + `url` mode: one comment job per account, each getting a single
 * **randomly-distributed** comment (see {@link distributeComments}), all aimed
 * at the same parsed article `target`.
 */
export function buildUrlCommentJobs(
  accountIds: string[],
  target: CommentArticleTarget,
  comments: string[],
  opts: DistributeOptions = {},
): CommentJob[] {
  if (accountIds.length === 0 || comments.length === 0) return [];
  const rng = opts.rng ?? defaultRng();
  return distributeComments(accountIds, comments, rng).map((a) => ({
    accountId: a.accountId,
    cafeId: target.cafeId,
    articleId: target.articleId,
    content: a.content,
  }));
}

/**
 * Human "댓글 N/M건" summary of one account's comment outcomes, for folding into
 * a result row. `null` outcomes (the whole call rejected) read as "댓글 실패".
 */
export function commentSummary(
  outs: CommentPublishOutcome[] | null,
  accountId: string,
): string {
  if (!outs) return "댓글 실패";
  const mine = outs.filter((o) => o.accountId === accountId);
  if (mine.length === 0) return "댓글 없음";
  const ok = mine.filter((o) => o.success).length;
  return `댓글 ${ok}/${mine.length}건`;
}

/**
 * Whether *every* one of this account's comments landed. A `null` result (the
 * whole call rejected) or an account with no outcomes both read as not-ok — so a
 * row that posted but whose comments all failed never shows a green "성공" badge.
 * Shared by the comment-only and `both` (글+댓글) success judgements.
 */
export function commentsAllOk(
  outs: CommentPublishOutcome[] | null,
  accountId: string,
): boolean {
  if (!outs) return false;
  const mine = outs.filter((o) => o.accountId === accountId);
  return mine.length > 0 && mine.every((o) => o.success);
}
