import type { CommentJob } from "@/shared/bindings/CommentJob";
import type { CommentPublishOutcome } from "@/shared/bindings/CommentPublishOutcome";

/** A resolved numeric comment target — the cafe + article to comment on. */
export interface CommentArticleTarget {
  cafeId: number;
  articleId: number;
}

/**
 * Parse a naver cafe article URL into numeric `cafeId`/`articleId`.
 *
 * Handles the SPA form (`…/cafes/{cafeId}/articles/{articleId}`, optionally
 * under `/ca-fe`, `/f-e`, or mobile hosts) and the legacy query form
 * (`ArticleRead.nhn?clubid={cafeId}&articleid={articleId}`). Returns `null`
 * when neither shape (with positive ids) is present.
 */
export function parseCafeArticleUrl(
  url: string | undefined,
): CommentArticleTarget | null {
  if (!url) return null;

  const spa = url.match(/cafes\/(\d+)\/articles\/(\d+)/);
  if (spa) {
    const cafeId = Number(spa[1]);
    const articleId = Number(spa[2]);
    if (cafeId > 0 && articleId > 0) return { cafeId, articleId };
  }

  const club = url.match(/clubid=(\d+)/i);
  const art = url.match(/articleid=(\d+)/i);
  if (club && art) {
    const cafeId = Number(club[1]);
    const articleId = Number(art[1]);
    if (cafeId > 0 && articleId > 0) return { cafeId, articleId };
  }

  return null;
}

/**
 * `both` mode: one comment job per (successfully-posted article × comment).
 *
 * `posted` is the subset of naver posts that succeeded — each carries the
 * account plus the cafe/article the comment should attach to.
 */
export function buildBothCommentJobs(
  posted: { accountId: string; cafeId: number; articleId: number }[],
  comments: string[],
): CommentJob[] {
  return posted.flatMap((p) =>
    comments.map((content) => ({
      accountId: p.accountId,
      cafeId: p.cafeId,
      articleId: p.articleId,
      content,
    })),
  );
}

/**
 * `comment` + `url` mode: one comment job per (account × comment), all aimed at
 * the same parsed article `target`.
 */
export function buildUrlCommentJobs(
  accountIds: string[],
  target: CommentArticleTarget,
  comments: string[],
): CommentJob[] {
  return accountIds.flatMap((accountId) =>
    comments.map((content) => ({
      accountId,
      cafeId: target.cafeId,
      articleId: target.articleId,
      content,
    })),
  );
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
