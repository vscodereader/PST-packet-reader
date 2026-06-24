/** Pure helpers for the publish modal — extracted so the job/validation logic is
 * unit-testable without rendering the whole component. */

import type { Stock } from "@/shared/data/types";

/** 댓글 대상 글 개수의 허용 범위. 최신글은 페이지당 15개라 페이징해도 약 60개가
 * 한계이고, 연속 조회로 의심받지 않게 50으로 제한한다. */
export const MIN_COMMENT_COUNT = 1;
export const MAX_COMMENT_COUNT = 50;

/** 임의 입력값을 댓글 대상 글 개수(1~50 정수)로 정규화한다. 범위 밖·비정수·누락은
 * 1로 떨어진다. 프리셋(1/3/5/10)과 직접 입력 양쪽의 단일 검증 지점이다. */
export function clampCommentCount(value: number | null | undefined): number {
  if (typeof value !== "number" || !Number.isFinite(value)) {
    return MIN_COMMENT_COUNT;
  }
  const n = Math.floor(value);
  if (n < MIN_COMMENT_COUNT) return MIN_COMMENT_COUNT;
  if (n > MAX_COMMENT_COUNT) return MAX_COMMENT_COUNT;
  return n;
}

/**
 * 선택한 종목 코드를 계정 수만큼 **균등 분배**한다(#267-5: "나눠서 게시"). 앞 버킷부터 1개씩
 * 더 받도록 나눠, 나머지가 뒤로 몰리지 않게 한다(차이 최대 1). 예: 18개·4계정 → [5,5,4,4]
 * (5,5,5,3이 아니라). 계정 수가 0이면 빈 배열, 종목이 계정보다 적으면 뒤 버킷은 빈 배열.
 */
export function distributeStocksEvenly(
  codes: string[],
  buckets: number,
): string[][] {
  if (buckets <= 0) return [];
  const out: string[][] = Array.from({ length: buckets }, () => []);
  const base = Math.floor(codes.length / buckets);
  const remainder = codes.length % buckets;
  let cursor = 0;
  for (let b = 0; b < buckets; b++) {
    // 앞에서 remainder개 버킷만 base+1개를 받아, 동등하게(차이 ≤ 1) 분배한다.
    const take = base + (b < remainder ? 1 : 0);
    out[b] = codes.slice(cursor, cursor + take);
    cursor += take;
  }
  return out;
}

/** A cafe publish target parsed from a board link — the cafe plus the board
 * (menu) to post into. `boardType` is resolved later (게시 시점, 쿠키 필요). */
export interface CafeBoardTarget {
  cafeId: number;
  menuId: number;
}

/**
 * Parse a naver cafe **board** URL into numeric `cafeId`/`menuId`.
 *
 * 카페는 밴드와 달리 게시판(menuId)이 필요하고, 게시판 목록 API는 쿠키 필수라
 * 시드 로그인 없이는 못 받는다(401). 그래서 사용자가 올릴 게시판의 URL을 붙여넣으면
 * 거기서 `cafeId`+`menuId`를 **쿠키 없이** 뽑는다. 구조는 [`parseCafeArticleUrl`]과
 * 동일: `clubid`/`menuid`가 `iframe_url_utf8`에 (이중)인코딩될 수 있어 점진적으로
 * `decodeURIComponent`하며 각 단계를 매칭하고, SPA 형(`cafes/{id}/menus/{id}`)을
 * 폴백으로 둔다. menuId 없는 링크(카페 홈 등)는 `null` — 호출부가 추가를 거부한다.
 */
export function parseCafeBoardLink(
  url: string | undefined,
): CafeBoardTarget | null {
  if (!url) return null;

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
    // 쿼리 형: clubid|cafeid + menuid. `\b`로 subclubid 등 부분일치 차단.
    const club = c.match(/\b(?:clubid|cafeid)=(\d+)/i);
    const menu = c.match(/\bmenuid=(\d+)/i);
    if (club && menu) {
      const cafeId = Number(club[1]);
      const menuId = Number(menu[1]);
      if (cafeId > 0 && menuId > 0) return { cafeId, menuId };
    }
    // SPA 형(폴백): cafes/{id}/menus/{id}.
    const spa = c.match(/cafes\/(\d+)\/menus\/(\d+)/);
    if (spa) {
      const cafeId = Number(spa[1]);
      const menuId = Number(spa[2]);
      if (cafeId > 0 && menuId > 0) return { cafeId, menuId };
    }
  }

  return null;
}

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
  return (
    html
      .replace(/<br\s*\/?>/gi, "\n")
      .replace(/<\/(p|div|li|h[1-6])>/gi, "\n")
      .replace(/<img\b[^>]*>/gi, imgPlaceholder)
      .replace(
        /<a\b[^>]*\bhref\s*=\s*["']([^"']*)["'][^>]*>([\s\S]*?)<\/a>/gi,
        anchorToText,
      )
      .replace(/<[^>]*>/g, "")
      // HTML 엔티티를 디코드한다. contentEditable 본문기는 공백(특히 연속/줄끝 공백)을 직렬화할
      // 때 innerHTML에 `&nbsp;`로 넣으므로, 태그만 벗기면 `&nbsp;`라는 글자가 그대로 본문에
      // 박혀 카페에 게시된다. `&amp;`는 이중 디코드를 막기 위해 반드시 마지막에 푼다.
      .replace(/&nbsp;/gi, " ")
      .replace(/&lt;/gi, "<")
      .replace(/&gt;/gi, ">")
      .replace(/&quot;/gi, '"')
      .replace(/&#0*39;|&apos;/gi, "'")
      .replace(/&amp;/gi, "&")
      .replace(/\n{3,}/g, "\n\n")
      .trim()
  );
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
