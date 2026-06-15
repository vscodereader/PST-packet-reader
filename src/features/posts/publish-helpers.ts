/** Pure helpers for the publish modal — extracted so the job/validation logic is
 * unit-testable without rendering the whole component. */

import type { Stock } from "@/shared/data/types";

/** 붙여넣은 URL 처리. 종목 시세 링크(6자리 코드)는 시세 줄로 바꾸고, 그 외 링크/내용은
 * 붙여넣은 원문 그대로 둔다 — URL이 본문에 남아야 게시 글에서 링크가 보인다. 예전엔
 * 일반 링크를 "[host에서 가져온 내용]" 가짜 문구로 바꿔 URL이 통째로 유실됐다. */
export function crawlToText(u: string, stocks: Stock[]): string {
  const m = u.match(/code=(\d{6})/) ?? u.match(/(\d{6})/);
  const s = m?.[1] ? stocks.find((x) => x.code === m[1]) : undefined;
  if (s) {
    const arrow = s.chg > 0 ? "▲" : s.chg < 0 ? "▼" : "·";
    return `${s.name}(${s.code}) · ${s.market} 현재가 ${s.price} (${arrow}${Math.abs(s.chg)}%)`;
  }
  return u;
}

/** Turn an `<img>` tag into a readable placeholder so it survives the plain-text
 * flattening. Prefers `alt`, falls back to `src`, else a bare marker. */
function imgPlaceholder(tag: string): string {
  const alt =
    tag.match(/\balt\s*=\s*"([^"]*)"/i)?.[1] ??
    tag.match(/\balt\s*=\s*'([^']*)'/i)?.[1];
  const src =
    tag.match(/\bsrc\s*=\s*"([^"]*)"/i)?.[1] ??
    tag.match(/\bsrc\s*=\s*'([^']*)'/i)?.[1];
  const label = (alt ?? "").trim() || (src ?? "").trim();
  return label ? `[이미지: ${label}]` : "[이미지]";
}

/** Turn an `<a href="URL">텍스트</a>` into plain text that keeps the URL, so the
 * link survives flattening. `텍스트 (URL)`, or just the URL when the visible text
 * already equals it. Without this the tag-strip below drops the href entirely and
 * the posted 평문 글 loses the link. */
function anchorToText(_tag: string, href: string, inner: string): string {
  const url = href.trim();
  const text = inner.replace(/<[^>]*>/g, "").trim();
  if (!url) return text;
  return text && text !== url ? `${text} (${url})` : url;
}

/**
 * Flatten the document's HTML body into plain text for the article body.
 *
 * `<img>` is converted to a `[이미지: …]` placeholder (alt, else src) rather than
 * stripped — otherwise a chart/image-only 글 posts to 네이버 as empty text while
 * the forum path keeps the original HTML, silently losing the images.
 * `<a href>` is likewise kept as `텍스트 (URL)` so links aren't lost.
 */
export function htmlToText(html: string): string {
  return html
    .replace(/<br\s*\/?>/gi, "\n")
    .replace(/<\/(p|div|li|h[1-6])>/gi, "\n")
    .replace(/<img\b[^>]*>/gi, imgPlaceholder)
    .replace(
      /<a\b[^>]*\bhref\s*=\s*["']([^"']*)["'][^>]*>([\s\S]*?)<\/a>/gi,
      anchorToText,
    )
    .replace(/<[^>]*>/g, "")
    .replace(/\n{3,}/g, "\n\n")
    .trim();
}

/**
 * The selected 네이버 accounts whose cafe board isn't resolved yet, in
 * post/both mode (comment mode targets a URL/latest/popular, not a board).
 *
 * Job building skips these accounts, but the publish button only checks
 * `jobs.length > 0` — so without this gate, selecting 3 accounts where one is
 * still loading its board would publish 2 and silently drop the third, with no
 * row in the results. Callers use this to block publishing and warn instead.
 */
export function unreadyNaverAccountIds(
  selected: string[],
  accounts: { id: string; platform: string }[],
  picks: Record<string, { boardName: string } | undefined>,
  mode: string,
): string[] {
  if (mode === "comment") return [];
  return selected.filter((id) => {
    const a = accounts.find((x) => x.id === id);
    if (!a || a.platform !== "naver") return false;
    const pick = picks[id];
    return !pick || !pick.boardName;
  });
}
