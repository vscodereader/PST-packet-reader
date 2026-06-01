import type { Mode, Platform } from "./types";

// ---------------------------------------------------------------------------
// Static app config — constant lookup tables only.
//
// All *stateful* domain data (accounts, posts, queue, stocks, activity, stats,
// scheduled, log batches, cafes, bands) lives in Rust and is served over Tauri
// IPC via `src/shared/ipc/*`. Pure helpers live in `./helpers`.
// ---------------------------------------------------------------------------

export const PLATFORMS: Platform[] = [
  {
    id: "forum",
    name: "종합토론방",
    short: "토론방",
    color: "forum",
    soon: false,
    targetLabel: "종목",
  },
  {
    id: "naver",
    name: "네이버 카페",
    short: "카페",
    color: "naver",
    soon: false,
    targetLabel: "카페",
  },
  {
    id: "band",
    name: "밴드",
    short: "밴드",
    color: "band",
    soon: false,
    targetLabel: "밴드",
  },
  {
    id: "instagram",
    name: "인스타그램",
    short: "인스타",
    color: "pink",
    soon: true,
    targetLabel: "계정",
  },
  {
    id: "threads",
    name: "스레드",
    short: "스레드",
    color: "dark",
    soon: true,
    targetLabel: "계정",
  },
];

export const PLATFORM: Record<string, Platform> = Object.fromEntries(
  PLATFORMS.map((p) => [p.id, p]),
);
export const ACTIVE_PLATFORMS = PLATFORMS.filter((p) => !p.soon);

export const MODES: Mode[] = [
  { v: "post", t: "글 작성", s: "게시글을 새로 등록", icon: "fileText" },
  {
    v: "comment",
    t: "댓글 작성",
    s: "기존 게시글에 댓글 등록",
    icon: "comment",
  },
  { v: "both", t: "글 + 댓글", s: "글 등록 후 댓글까지", icon: "layers" },
];

export const KIND: Record<string, { t: string; c: string }> = {
  post: { t: "글", c: "blue" },
  comment: { t: "댓글", c: "forum" },
  both: { t: "글+댓글", c: "green" },
};
export const KIND_ICON: Record<string, string> = {
  post: "fileText",
  comment: "comment",
  both: "layers",
};

export const BOARDS: Record<string, string[]> = {
  forum: ["종목토론방"],
  naver: ["자유게시판", "종목분석", "질문/답변", "공지사항", "정보 공유"],
  band: ["전체글", "공지", "사진첩", "일정"],
};

export const STATUS_ACCOUNT: Record<string, { t: string; c: string }> = {
  new: { t: "사용전", c: "gray" },
  active: { t: "활성", c: "green" },
  error: { t: "에러", c: "red" },
};
export const STATUS_ACCOUNT_ORDER = ["new", "active", "error"];

export const STATUS_LABEL: Record<string, { t: string; c: string }> = {
  draft: { t: "임시저장", c: "gray" },
  ready: { t: "작성완료", c: "blue" },
  scheduled: { t: "예약됨", c: "yellow" },
  published: { t: "게시완료", c: "green" },
};
