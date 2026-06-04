import { invoke } from "@tauri-apps/api/core";

import type { Account } from "@/shared/bindings/Account";
import type { ActivityItem } from "@/shared/bindings/ActivityItem";
import type { Band } from "@/shared/bindings/Band";
import type { Cafe } from "@/shared/bindings/Cafe";
import type { DashStat } from "@/shared/bindings/DashStat";
import type { EnvironmentStatus } from "@/shared/bindings/EnvironmentStatus";
import type { LibraryPost } from "@/shared/bindings/LibraryPost";
import type { LogBatch } from "@/shared/bindings/LogBatch";
import type { QueueNowItem } from "@/shared/bindings/QueueNowItem";
import type { QueueScheduledItem } from "@/shared/bindings/QueueScheduledItem";
import type { Stock } from "@/shared/bindings/Stock";

export type {
  Account,
  ActivityItem,
  Band,
  Cafe,
  DashStat,
  EnvironmentStatus,
  LibraryPost,
  LogBatch,
  QueueNowItem,
  QueueScheduledItem,
  Stock,
};

/** A forum (종목토론방) target for the packet posting engine. */
export interface ForumStock {
  name: string;
  code: string;
  link: string;
}

/** "지금 바로 게시 + 종목토론방" request for the packet posting engine. */
export interface ForumPublishRequest {
  host: string;
  port: number;
  /** Account loginId; selects the saved login cookies (cookies/{loginId}.json). */
  accountId: string;
  runPost: boolean;
  runComment: boolean;
  title: string;
  body: string;
  comment: string;
  stocks: ForumStock[];
}

/** Per-stock result of a forum publish. */
export interface ForumPublishResult {
  code: string;
  name: string;
  ok: boolean;
  message: string;
}

/** A naver-login account (auth module): keyed by loginId so cookies land at cookies/{loginId}.json. */
export interface AuthAccount {
  id: string;
  password: string;
  label: string;
}

export type LoginJobStatus =
  | "pending"
  | "expired"
  | "running"
  | "success"
  | "failed";

export interface LoginJob {
  accountId: string;
  status: LoginJobStatus;
  message: string;
}

export interface LoginQueueStatus {
  isRunning: boolean;
  currentAccountId: string | null;
  jobs: LoginJob[];
}

/** Thin typed wrapper around a single Tauri command channel. */
function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  return invoke<T>(cmd, args);
}

/**
 * Single entry point for every Tauri IPC channel, grouped by domain.
 *
 * Read commands return the full list; mutations return the full *updated* list
 * so callers can replace their state in one step (mirrors the Rust contract).
 * Outside Tauri (browser dev / Vitest) `@tauri-apps/api/core` is mocked by the
 * in-memory backend in `src/test/ipc.ts`.
 */
export const ipc = {
  accounts: {
    list: () => call<Account[]>("list_accounts"),
    add: (account: Account) => call<Account[]>("add_account", { account }),
    update: (account: Account) =>
      call<Account[]>("update_account", { account }),
    remove: (ids: string[]) => call<Account[]>("delete_accounts", { ids }),
  },
  posts: {
    list: () => call<LibraryPost[]>("list_posts"),
    upsert: (post: LibraryPost) => call<LibraryPost[]>("upsert_post", { post }),
    remove: (id: string) => call<LibraryPost[]>("delete_post", { id }),
  },
  queue: {
    listNow: () => call<QueueNowItem[]>("list_queue_now"),
    listScheduled: () => call<QueueScheduledItem[]>("list_queue_scheduled"),
    cancelNow: (id: string) => call<QueueNowItem[]>("cancel_queue_now", { id }),
    cancelScheduled: (id: string) =>
      call<QueueScheduledItem[]>("cancel_queue_scheduled", { id }),
    /**
     * Append a scheduled item at local epoch-ms `atMs`; returns the list.
     * Rejects if the backend deems the time already past.
     */
    addScheduled: (item: QueueScheduledItem, atMs: number) =>
      call<QueueScheduledItem[]>("add_queue_scheduled", { item, at: atMs }),
    /** Move a scheduled item into the immediate queue; returns the new now-list. */
    promote: (id: string) =>
      call<QueueNowItem[]>("promote_queue_scheduled", { id }),
  },
  stocks: { list: () => call<Stock[]>("list_stocks") },
  activity: { list: () => call<ActivityItem[]>("list_activity") },
  stats: { list: () => call<DashStat[]>("list_stats") },
  logBatches: { list: () => call<LogBatch[]>("list_log_batches") },
  cafes: { list: () => call<Cafe[]>("list_cafes") },
  bands: { list: () => call<Band[]>("list_bands") },
  diagnostics: {
    /** Probe Chrome install/version + ADB device connection (UI 새로고침). */
    getStatus: () => call<EnvironmentStatus>("get_environment_status"),
    /** Chrome 미설치 안내 카드의 "설치 페이지 열기" — 공식 다운로드 페이지를 기본 브라우저로 연다. */
    openChromeDownload: () => call<void>("open_chrome_download"),
  },
  // 종목토론방(forum) 즉시 게시 — 네이버 증권 토론방 패킷 게시 엔진 호출.
  forum: {
    /** 게시 엔진이 붙을 Chrome DevTools 엔드포인트. 백엔드가 단일 출처(프론트 상수 아님). */
    endpoint: () => call<{ host: string; port: number }>("forum_endpoint"),
    publishNow: (request: ForumPublishRequest) =>
      call<ForumPublishResult[]>("run_forum_publish_now", { request }),
  },
  // 네이버 로그인 자동화(CDP). 계정 ID/PW로 로그인해 쿠키를 저장한다.
  auth: {
    bootstrap: () => call<unknown>("bootstrap_runtime"),
    saveAccounts: (accounts: AuthAccount[]) =>
      call<AuthAccount[]>("save_accounts", { accounts }),
    enqueueLogin: (accountIds: string[], headless = false) =>
      call<LoginQueueStatus>("enqueue_cookie_refresh", {
        accountIds,
        headless,
        useAdb: false,
      }),
    queueStatus: () => call<LoginQueueStatus>("get_queue_status"),
  },
};
