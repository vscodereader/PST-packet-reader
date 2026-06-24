import type { Account, LogBatch, PlatformId } from "./types";

// ---------------------------------------------------------------------------
// Pure helpers — no stateful data. Callers pass in IPC-loaded records.
// Static config lives in `./config`.
// ---------------------------------------------------------------------------

export function jobLink(job: { code?: string; url?: string } | null): string {
  if (job && job.code)
    return `https://finance.naver.com/item/main.naver?code=${job.code}`;
  return (job && job.url) || "";
}

export function resolveTemplate(
  text: string,
  job: {
    platform?: PlatformId;
    targetName?: string;
    code?: string;
    url?: string;
  } | null,
  linkOverride?: string,
): string {
  if (!text) return text;
  // 종목 토큰(#{종목명}/#{종목코드})은 종목토론방 전용이다. 카페·밴드는 종목 개념이 없어
  // 변수명을 글에 그대로 남기지 않고 빈값으로 지운다(백엔드 resolve_cafe_band와 일치).
  // platform 미지정(기본 미리보기/레거시 호출)은 forum으로 본다.
  const isForum = !job?.platform || job.platform === "forum";
  const name = isForum ? job?.targetName || "" : "";
  const code = isForum ? job?.code || "" : "";
  const link = (linkOverride && linkOverride.trim()) || jobLink(job);
  return text
    .replace(/#\{\s*종목명\s*\}/g, name)
    .replace(/#\{\s*종목코드\s*\}/g, code)
    .replace(/#\{\s*링크\s*\}/g, link);
}

export function hasToken(
  text: string,
  kind?: "stock" | "code" | "link",
): boolean {
  if (!text) return false;
  const re =
    kind === "stock"
      ? /#\{\s*종목명\s*\}/
      : kind === "code"
        ? /#\{\s*종목코드\s*\}/
        : kind === "link"
          ? /#\{\s*링크\s*\}/
          : /#\{\s*(종목명|종목코드|링크)\s*\}/;
  return re.test(text);
}

/** epoch-ms를 "방금/N분 전/N시간 전/어제 HH:mm/M월 D일"로 포맷. */
export function formatRelative(at: number, now: number = Date.now()): string {
  const diff = now - at;
  if (diff < 60_000) return "방금";
  if (diff < 3_600_000) return `${Math.floor(diff / 60_000)}분 전`;
  if (diff < 86_400_000) return `${Math.floor(diff / 3_600_000)}시간 전`;
  const d = new Date(at);
  if (dayBucket(at, now) === "어제") {
    const hh = String(d.getHours()).padStart(2, "0");
    const mm = String(d.getMinutes()).padStart(2, "0");
    return `어제 ${hh}:${mm}`;
  }
  return `${d.getMonth() + 1}월 ${d.getDate()}일`;
}

/** epoch-ms를 캘린더 날짜 기준 오늘/어제/이전으로 분류. */
export function dayBucket(
  at: number,
  now: number = Date.now(),
): "오늘" | "어제" | "이전" {
  const startOf = (ms: number) => {
    const d = new Date(ms);
    return new Date(d.getFullYear(), d.getMonth(), d.getDate()).getTime();
  };
  const today = startOf(now);
  const day = 86_400_000;
  const atDay = startOf(at);
  if (atDay >= today) return "오늘";
  if (atDay >= today - day) return "어제";
  return "이전";
}

export function batchStatus(
  b: LogBatch,
): "running" | "success" | "fail" | "partial" {
  if (
    b.state === "running" ||
    b.items.some((i) => i.status === "running" || i.status === "waiting")
  )
    return "running";
  const fails = b.items.filter((i) => i.status === "fail").length;
  // 차단으로 건너뛴 글(skip, #267-9)은 게시되지 않은 것이라 성공이 아니다. 실패와 합쳐,
  // 전부 실패/건너뜀이면 "fail", 일부만 성공이면 "partial"로 본다(전부 skip+fail이 성공으로
  // 오판되지 않게). skip이 없으면 기존 동작과 동일하다.
  const skips = b.items.filter((i) => i.status === "skip").length;
  if (fails === 0 && skips === 0) return "success";
  if (fails + skips === b.items.length) return "fail";
  return "partial";
}

/** Distinct platforms covered by the given account ids, in first-seen order. */
export function acctPlatforms(
  ids: string[],
  accounts: Account[],
): PlatformId[] {
  const seen: PlatformId[] = [];
  ids.forEach((id) => {
    const a = accounts.find((x) => x.id === id);
    if (a && !seen.includes(a.platform)) seen.push(a.platform);
  });
  return seen;
}
