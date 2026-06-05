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
  job: { targetName?: string; code?: string; url?: string } | null,
  linkOverride?: string,
): string {
  if (!text) return text;
  const name = (job && job.targetName) || "";
  const code = (job && job.code) || "";
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
  if (fails === 0) return "success";
  if (fails === b.items.length) return "fail";
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
