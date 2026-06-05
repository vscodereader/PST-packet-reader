import type { CommentPublishOutcome } from "@/shared/bindings/CommentPublishOutcome";

/** A resolved numeric comment target — the cafe + article to comment on. */
export interface CommentArticleTarget {
  cafeId: number;
  articleId: number;
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
 * Take the top-N entries of a latest/popular article list, preserving the
 * backend's order. Graceful fallback: when the list has fewer than N (or N <= 0)
 * only the available entries are returned — never throws or pads.
 */
export function topNArticles<T>(articles: T[], n: number): T[] {
  if (n <= 0) return [];
  return articles.slice(0, n);
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
