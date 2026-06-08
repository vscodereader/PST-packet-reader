import type { PublishPlan } from "@/shared/bindings/PublishPlan";

export type ViewId = "dashboard" | "posts" | "queue" | "log" | "accounts";

export interface LogFilter {
  loginId?: string;
  platform?: PlatformId;
  batchId?: string;
}

export interface GoOpts {
  logFilter?: LogFilter | null;
}

/** Navigate to a top-level view, optionally carrying view-specific options. */
export type GoFn = (view: ViewId, opts?: GoOpts) => void;

export type PlatformId = "forum" | "naver" | "band" | "instagram" | "threads";

export interface Platform {
  id: PlatformId;
  name: string;
  short: string;
  /** Mantine theme color name */
  color: string;
  soon: boolean;
  targetLabel: string;
}

export type ModeValue = "post" | "comment" | "both";

export interface Mode {
  v: ModeValue;
  t: string;
  s: string;
  icon: string;
}

export interface Stock {
  code: string;
  name: string;
  market: string;
  posts: string;
  price: string;
  chg: number;
}

/** A live Naver stock-search result (backend `search_stocks` → `StockCandidate`). */
export interface StockCandidate {
  name: string;
  code: string;
  link: string;
}

export type AccountStatus = "new" | "active" | "error";

export interface Account {
  id: string;
  platform: PlatformId;
  loginId: string;
  pw: string;
  status: AccountStatus;
  last: string;
  tags: string[];
}

// Cafe/Board are generated from Rust (ts-rs) — re-exported here so UI code can
// keep importing domain types from one place. A cafe's boards are now rich
// objects ({ name, menuId, boardType }), not plain strings.
export type { Board } from "@/shared/bindings/Board";
export type { Cafe } from "@/shared/bindings/Cafe";
// 게시 실행 페이로드(ts-rs 생성). 큐 아이템의 `plan?` 필드와 게시 모달이 사용한다.
export type { PublishPlan };

export interface Band {
  name: string;
}

export type PostStatus = "draft" | "ready" | "scheduled" | "published";

export type CommentTarget = "latest" | "popular" | "url";

export interface LibraryPost {
  id: string;
  title: string;
  kind: ModeValue;
  updated: string;
  words: number;
  status: PostStatus;
  excerpt: string;
  body?: string;
  comments?: string[];
  commentTarget?: CommentTarget;
  commentUrl?: string;
  commentCount?: number;
}

/** A single account×destination upload target, expanded from a publish request. */
export interface PublishJob {
  key: string;
  platform: PlatformId;
  loginId: string;
  targetName: string;
  code?: string;
  board: string;
  status: AccountStatus;
}

export interface PublishResult extends PublishJob {
  ok: boolean;
  msg: string;
}

export interface ActivityItem {
  id: string;
  type: "success" | "error" | "info";
  text: string;
  at: number;
}

export interface QueueLocation {
  p: PlatformId;
  name: string;
  code?: string;
}

export interface QueueNowItem {
  id: string;
  title: string;
  kind: ModeValue;
  state: "running" | "waiting";
  batchId?: string;
  progress?: [number, number];
  locs: QueueLocation[];
  /** 워커가 실제 게시에 쓰는 실행 페이로드. 표시 전용 아이템은 없음. */
  plan?: PublishPlan;
}

export interface QueueScheduledItem {
  id: string;
  title: string;
  kind: ModeValue;
  when: string;
  rel: string;
  locs: QueueLocation[];
  /** 워커가 실제 게시에 쓰는 실행 페이로드. 표시 전용 아이템은 없음. */
  plan?: PublishPlan;
}

export interface BatchItem {
  platform: PlatformId;
  target: string;
  code?: string;
  board?: string;
  loginId: string;
  status: "success" | "fail" | "running" | "waiting";
  msg: string;
  trace?: string;
}

export interface LogBatch {
  id: string;
  title: string;
  body?: string;
  comment?: string;
  kind: ModeValue;
  at: number;
  state?: "running";
  items: BatchItem[];
}

export interface DashStat {
  key: string;
  label: string;
  value: string | number;
  sub: string;
  icon: string;
  color: string;
}
