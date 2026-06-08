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
  badCredentials: { t: "비번오류", c: "orange" },
  challenge: { t: "인증필요", c: "yellow" },
  blocked: { t: "차단", c: "red" },
  error: { t: "에러", c: "red" },
};
export const STATUS_ACCOUNT_ORDER = [
  "new",
  "active",
  "badCredentials",
  "challenge",
  "blocked",
  "error",
];

// 사용자가 배지를 클릭해 순환시킬 수 있는 "사람이 정하는" 상태만 둔다. 비번오류/인증필요/
// 에러는 로그인 워커가 자동으로 설정하는 표시 전용 상태라 수동 순환에서 제외한다.
export const STATUS_ACCOUNT_CYCLE = ["new", "active", "blocked"];

// 상태별 사용자 조치 안내(정적). 백엔드 `auth::outcome::guide`와 의미를 맞춘다. 배지
// tooltip에 보여, 사용자가 다음에 무엇을 해야 할지 알 수 있게 한다.
export const STATUS_GUIDE: Record<string, string> = {
  active: "정상적으로 로그인되었습니다.",
  badCredentials:
    "아이디 또는 비밀번호가 올바르지 않습니다. 계정 정보를 확인하세요.",
  challenge:
    "추가 인증이 필요합니다. 열린 창에서 캡차/2차 인증을 완료한 뒤 다시 실행하세요.",
  blocked:
    "계정 접근이 차단되었습니다. 잠시 후 다시 시도하거나 계정 상태를 확인하세요.",
  error: "로그인 중 오류가 발생했습니다. 네트워크/환경을 확인하세요.",
  new: "아직 로그인하지 않은 계정입니다.",
};

export const STATUS_LABEL: Record<string, { t: string; c: string }> = {
  draft: { t: "임시저장", c: "gray" },
  ready: { t: "작성완료", c: "blue" },
  scheduled: { t: "예약됨", c: "yellow" },
  published: { t: "게시완료", c: "green" },
};
