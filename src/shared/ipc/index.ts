import { invoke } from "@tauri-apps/api/core";

import type { Account } from "@/shared/bindings/Account";
import type { ActivityItem } from "@/shared/bindings/ActivityItem";
import type { Band } from "@/shared/bindings/Band";
import type { Cafe } from "@/shared/bindings/Cafe";
import type { CommentJob } from "@/shared/bindings/CommentJob";
import type { CommentPublishOutcome } from "@/shared/bindings/CommentPublishOutcome";
import type { DashStat } from "@/shared/bindings/DashStat";
import type { JoinedCafe } from "@/shared/bindings/JoinedCafe";
import type { LibraryPost } from "@/shared/bindings/LibraryPost";
import type { LogBatch } from "@/shared/bindings/LogBatch";
import type { PostJob } from "@/shared/bindings/PostJob";
import type { PublishOutcome } from "@/shared/bindings/PublishOutcome";
import type { QueueNowItem } from "@/shared/bindings/QueueNowItem";
import type { QueueScheduledItem } from "@/shared/bindings/QueueScheduledItem";
import type { Stock } from "@/shared/bindings/Stock";

export type {
  Account,
  ActivityItem,
  Band,
  Cafe,
  CommentJob,
  CommentPublishOutcome,
  DashStat,
  JoinedCafe,
  LibraryPost,
  LogBatch,
  PostJob,
  PublishOutcome,
  QueueNowItem,
  QueueScheduledItem,
  Stock,
};

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
  cafes: {
    list: () => call<Cafe[]>("list_cafes"),
    /**
     * Resolve a cafe reference (URL/slug/numeric) into a registrable cafe,
     * using `accountId`'s session cookie. Rejects with the backend's error
     * envelope (`{ code, message }`) on failure. Backs "+ 카페 추가".
     */
    resolve: (input: string, accountId: string) =>
      call<Cafe>("resolve_cafe", { input, accountId }),
    /** Persist a resolved cafe (upsert by cafeId); returns the updated list. */
    upsert: (cafe: Cafe) => call<Cafe[]>("upsert_cafe", { cafe }),
    /** Run publish jobs sequentially; returns one slim outcome per job. */
    runPostJobs: (jobs: PostJob[]) =>
      call<PublishOutcome[]>("run_post_jobs", { jobs }),
    /**
     * Run comment jobs sequentially; returns one slim outcome per job. Each job
     * targets a numeric `cafeId`/`articleId` (from a just-posted article or a
     * parsed URL); one job failing does not stop the rest.
     */
    runCommentJobs: (jobs: CommentJob[]) =>
      call<CommentPublishOutcome[]>("run_comment_jobs", { jobs }),
    /**
     * List every cafe `accountId` has joined (crawled across all pages),
     * using its session cookie. Rejects with the backend's error envelope
     * on failure. Backs an account-driven "가입 카페 자동 로드" flow.
     */
    listJoined: (accountId: string) =>
      call<JoinedCafe[]>("list_joined_cafes", { accountId }),
  },
  bands: { list: () => call<Band[]>("list_bands") },
};
