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

/**
 * "나눠서 게시"(댓글 분배, 설계서 §3) 활성 조건. 종토 "특정 게시글" 댓글을 계정들에 균등
 * 분배하려면 **댓글 수가 계정 수 이상이고 계정이 1개 이상**이어야 한다. 댓글이 계정보다 적으면
 * 균등 분배 시 빈 계정이 생기므로 비활성이다(기존 `==` 조건을 `>=`로 완화).
 */
export function canDistributeComments(
  commentCount: number,
  accountCount: number,
): boolean {
  return accountCount > 0 && commentCount >= accountCount;
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

/** 네이버 블로그 글 링크에서 파싱한 댓글 대상 — 블로그 식별자(문자열)와 글 번호(문자열). */
export interface BlogPostTarget {
  blogId: string;
  logNo: string;
}

/**
 * 네이버 블로그 **글** URL에서 `blogId`(문자열)와 `logNo`(숫자 문자열)를 뽑는다(#271).
 *
 * 블로그는 댓글 전용이라 카페 게시판처럼 게시판 목록이 아니라 **그 글 하나**가 곧 대상이다.
 * `blogId`는 숫자가 아니라 문자열(예: "press02", "cho41004")이다 — 블루프린트의 숫자 가정 버그를
 * 바로잡는다. 다음 형태를 모두 지원한다:
 *   - `https://blog.naver.com/{blogId}/{logNo}`
 *   - `https://blog.naver.com/PostView.naver?blogId={blogId}&logNo={logNo}`
 *   - 위가 `iframe_url`/encoded로 한 번 더 감싸진 형태(점진적 decodeURIComponent로 풀어 매칭).
 * 인식 못 하면 `null` — 호출부가 추가를 거부한다.
 */
export function parseBlogPostLink(
  url: string | undefined,
): BlogPostTarget | null {
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

  const valid = (blogId: string, logNo: string): BlogPostTarget | null =>
    blogId && /^\d+$/.test(logNo) ? { blogId, logNo } : null;

  for (const c of candidates) {
    // 쿼리 형: blogId=... & logNo=... (blogId는 문자열, logNo는 숫자).
    const qBlog = c.match(/[?&]blogId=([^&#/]+)/i);
    const qLog = c.match(/[?&]logNo=(\d+)/i);
    if (qBlog && qLog) {
      const t = valid(qBlog[1]!, qLog[1]!);
      if (t) return t;
    }
    // 경로 형: blog.naver.com/{blogId}/{logNo}. blogId는 영숫자/._- 허용, logNo는 숫자.
    const path = c.match(
      /blog\.naver\.com\/([A-Za-z0-9][A-Za-z0-9._-]*)\/(\d+)/i,
    );
    if (path) {
      const t = valid(path[1]!, path[2]!);
      if (t) return t;
    }
  }

  return null;
}

/** 네이버 블로그 **홈/임의** 링크에서 파싱한 "최신 N개" 댓글 대상(#279) — 블로그 식별자와
 *  (있으면) 카테고리 번호. 특정 글(logNo)이 아니라 블로그 자체가 대상이다. */
export interface BlogLinkTarget {
  blogId: string;
  /** 글 목록을 좁힐 카테고리 번호(있으면). 없으면 전체(백엔드에서 0으로 본다). */
  categoryNo?: number;
}

/**
 * 네이버 블로그 **홈/임의** URL에서 `blogId`(문자열)와 (있으면) `categoryNo`를 뽑는다(#279).
 *
 * "최신 N개 글에 댓글" 모드는 특정 글(logNo)이 아니라 블로그 자체가 대상이라, logNo 없이
 * blogId만 있으면 충분하다([`parseBlogPostLink`]는 logNo가 필수라 홈 링크엔 못 쓴다). 다음을
 * 모두 지원한다:
 *   - `https://blog.naver.com/{blogId}`            (블로그 홈)
 *   - `https://blog.naver.com/{blogId}/{logNo}`    (글 — blogId만 취한다)
 *   - `https://blog.naver.com/{blogId}?categoryNo=7`
 *   - `https://blog.naver.com/PostList.naver?blogId={blogId}&categoryNo=7`
 *   - 위가 encoded로 한 번 더 감싸진 형태(점진적 decodeURIComponent로 풀어 매칭).
 * 인식 못 하면 `null` — 호출부가 추가를 거부한다.
 */
export function parseBlogLink(url: string | undefined): BlogLinkTarget | null {
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

  const categoryOf = (s: string): number | undefined => {
    const m = s.match(/[?&]categoryNo=(\d+)/i);
    if (!m) return undefined;
    const n = Number(m[1]);
    return Number.isFinite(n) && n > 0 ? n : undefined;
  };
  // blogId만 따로 모아 categoryNo가 있는 후보를 우선 골라 카테고리를 보존한다.
  const make = (blogId: string, s: string): BlogLinkTarget => {
    const categoryNo = categoryOf(s);
    return categoryNo !== undefined ? { blogId, categoryNo } : { blogId };
  };

  // 쿼리 형(blogId=...)을 경로 형보다 우선한다 — 링크가 한 번 더 감싸졌을 때(예: `/x?u=…`)
  // 바깥 경로(`/x`)를 blogId로 오인하지 않도록 모든 후보에서 먼저 쿼리 형을 찾는다.
  for (const c of candidates) {
    const qBlog = c.match(/[?&]blogId=([^&#/]+)/i);
    if (qBlog?.[1]) return make(qBlog[1], c);
  }
  for (const c of candidates) {
    // 경로 형: blog.naver.com/{blogId}(/...)?. blogId는 영숫자/._- 허용. 예약 경로(PostList 등)는
    // 위 쿼리 형에서 처리되므로 여기선 일반 blogId만 잡는다(`.naver` 접미는 제외).
    const path = c.match(/blog\.naver\.com\/([A-Za-z0-9][A-Za-z0-9._-]*)/i);
    if (path?.[1] && !/\.naver$/i.test(path[1])) return make(path[1], c);
  }

  return null;
}

/** 네이버 클립 창작자 링크에서 파싱한 댓글 대상(#클립) — 창작자 핸들과 미디어 탭. */
export interface ClipLinkTarget {
  /** 창작자 핸들(@ 제외). 예: "dongzzi_chef". */
  handle: string;
  /** "video"면 영상만(?tab=video), 그 외/없으면 전체(?tab=all). */
  mediaType?: "all" | "video";
}

/**
 * 네이버 클립 **창작자** URL에서 핸들(@ 제외)과 탭(전체/영상)을 뽑는다(#클립).
 *
 * 클립도 블로그처럼 댓글 전용이며, 사용자가 창작자 링크를 넣으면 그 창작자의 최신 미디어 N개에
 * 댓글을 단다. 다음을 모두 지원한다:
 *   - `https://clip.naver.com/@dongzzi_chef`            (전체)
 *   - `https://clip.naver.com/@dongzzi_chef?tab=video`  (영상만)
 *   - `https://clip.naver.com/@dongzzi_chef?tab=all`    (전체)
 *   - 위가 encoded로 한 번 더 감싸진 형태(점진적 decodeURIComponent로 풀어 매칭).
 * 인식 못 하면 `null` — 호출부가 추가를 거부한다.
 */
export function parseClipLink(url: string | undefined): ClipLinkTarget | null {
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

  const tabOf = (s: string): "all" | "video" | undefined => {
    const m = s.match(/[?&]tab=(video|all)/i);
    if (!m) return undefined;
    return m[1]!.toLowerCase() === "video" ? "video" : "all";
  };

  for (const c of candidates) {
    // clip.naver.com/@<handle> — handle은 영숫자/._- 허용. @는 인코딩(%40)일 수 있어 위 디코드가 푼다.
    const m = c.match(/clip\.naver\.com\/@([A-Za-z0-9][A-Za-z0-9._-]*)/i);
    if (m?.[1]) {
      const mediaType = tabOf(c);
      return mediaType !== undefined
        ? { handle: m[1], mediaType }
        : { handle: m[1] };
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
      .replace(/<img\b[^>]*>/gi, imgPlaceholder)
      // 블록 요소 경계를 줄바꿈으로 바꾼다(#267-1 재수정). contentEditable에서 엔터를 치면 첫
      // 줄은 평문, 다음 줄부터 <div>로 감싸여 "하...<div>미치겠네</div>" 형태가 나온다. 닫는 태그만
      // \n으로 바꾸면 줄 **사이**가 아니라 끝에만 \n이 붙어 "하...미치겠네"로 달라붙는다. 그래서
      // **여는** 블록 태그를 \n으로 바꾸고 닫는 태그는 제거해, 줄과 줄 사이에 정확히 한 번
      // 줄바꿈이 들어가게 한다(<div>하...</div><div>미치겠네</div> 형태도 동일하게 처리).
      .replace(/<\/(?:p|div|li|h[1-6])>/gi, "")
      .replace(/<(?:p|div|li|h[1-6])\b[^>]*>/gi, "\n")
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
