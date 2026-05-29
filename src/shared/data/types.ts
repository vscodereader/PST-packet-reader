export type ViewId = "dashboard" | "posts" | "queue" | "log" | "accounts";

export interface LogFilter {
  loginId?: string;
  platform?: PlatformId;
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

export interface Cafe {
  name: string;
  boards: string[];
}

export interface Band {
  name: string;
}

export type PostStatus = "draft" | "ready" | "scheduled" | "published";

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
}

export interface Scheduled {
  id: string;
  title: string;
  accounts: string[];
  kind: ModeValue;
  when: string;
  rel: string;
}

export interface ActivityItem {
  id: string;
  type: "success" | "error" | "info";
  text: string;
  time: string;
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
}

export interface QueueScheduledItem {
  id: string;
  title: string;
  kind: ModeValue;
  when: string;
  rel: string;
  locs: QueueLocation[];
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
  kind: ModeValue;
  time: string;
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
