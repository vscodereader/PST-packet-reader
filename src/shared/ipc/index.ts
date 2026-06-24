import { invoke } from "@tauri-apps/api/core";

import type { Account } from "@/shared/bindings/Account";
import type { ActivityItem } from "@/shared/bindings/ActivityItem";
import type { Article } from "@/shared/bindings/Article";
import type { ArticleListResponse } from "@/shared/bindings/ArticleListResponse";
import type { Band } from "@/shared/bindings/Band";
import type { Cafe } from "@/shared/bindings/Cafe";
import type { CommentDistributionRequest } from "@/shared/bindings/CommentDistributionRequest";
import type { CommentPublishOutcome } from "@/shared/bindings/CommentPublishOutcome";
import type { DashStat } from "@/shared/bindings/DashStat";
import type { EnvironmentStatus } from "@/shared/bindings/EnvironmentStatus";
import type { ForumStockCategory } from "@/shared/bindings/ForumStockCategory";
import type { ForumStockPage } from "@/shared/bindings/ForumStockPage";
import type { ImportSummary } from "@/shared/bindings/ImportSummary";
import type { JoinedCafe } from "@/shared/bindings/JoinedCafe";
import type { LibraryPost } from "@/shared/bindings/LibraryPost";
import type { LogBatch } from "@/shared/bindings/LogBatch";
import type { PostJob } from "@/shared/bindings/PostJob";
import type { PublishOutcome } from "@/shared/bindings/PublishOutcome";
import type { QueueNowItem } from "@/shared/bindings/QueueNowItem";
import type { QueueScheduledItem } from "@/shared/bindings/QueueScheduledItem";
import type { SortBy } from "@/shared/bindings/SortBy";
import type { Stock } from "@/shared/bindings/Stock";
import type { StockExchange } from "@/shared/bindings/StockExchange";
import type { StockMarket } from "@/shared/bindings/StockMarket";
import type { StockCandidate } from "@/shared/data/types";

export type {
  Account,
  ActivityItem,
  Article,
  ArticleListResponse,
  Band,
  Cafe,
  CommentDistributionRequest,
  CommentPublishOutcome,
  DashStat,
  EnvironmentStatus,
  ImportSummary,
  JoinedCafe,
  LibraryPost,
  LogBatch,
  PostJob,
  PublishOutcome,
  QueueNowItem,
  QueueScheduledItem,
  SortBy,
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

/** 밴드 가입+게시 요청. accountId는 band 로그인 쿠키 키(loginId). */
export interface BandPublishRequest {
  accountId: string;
  /** 가입할 밴드 링크(`https://band.us/band/{no}` 형태). */
  bandLink: string;
  title: string;
  content: string;
  /** 댓글 풀. 비어있지 않은 항목을 모두 같은 글에 단다. 비우면 댓글 미작성. */
  comments: string[];
}

/** 밴드 가입+게시 결과(band_post::BandPublishOutcome 미러). */
export interface BandPublishOutcome {
  joined: boolean;
  postNo: number;
  webUrl: string;
  /** 같은 글에 단 댓글 중 성공한 개수(0이면 미작성). */
  commentedCount: number;
  /** 시도한 댓글 수. commentedCount와 비교해 "N/M건"·부분 실패 판정에 쓴다. */
  commentTotal: number;
  /** 실제 게시된 밴드 이름(게시 응답 post.band.name). 응답에 없으면 null. */
  bandName: string | null;
}

/** 밴드 댓글 전용 요청 — 기존 글(최신글/인기글) 상위 count개에 댓글. */
export interface BandCommentRequest {
  accountId: string;
  bandLink: string;
  /** 대상 글 정렬: 최신글(latest)/인기글(popular). */
  mode: "latest" | "popular";
  /** 상위 몇 개 글에 댓글을 달지. */
  count: number;
  /** 댓글 풀(대상 글마다 1개씩 분배). */
  comments: string[];
}

/** 밴드 댓글 전용 결과(band_post::BandCommentOutcome 미러). */
export interface BandCommentOutcome {
  /** 댓글 대상으로 조회된 글 수(상위 N개). */
  targetCount: number;
  /** 성공한 댓글 개수. */
  commentedCount: number;
  /** 실제 밴드 이름. 없으면 null. */
  bandName: string | null;
}

/** A naver-login account (auth module): keyed by loginId so cookies land at cookies/{loginId}.json. */
export interface AuthAccount {
  id: string;
  password: string;
  label: string;
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
     * Append an item to the immediate-processing queue ("즉시 처리 대기열") and
     * kick the worker; returns the new now-list. This is the path "지금 바로
     * 게시" takes — the worker publishes it just like a promoted schedule (#198).
     */
    addNow: (item: QueueNowItem) =>
      call<QueueNowItem[]>("add_queue_now", { item }),
    /**
     * Append a scheduled item at local epoch-ms `atMs`; returns the list.
     * Rejects if the backend deems the time already past.
     */
    addScheduled: (item: QueueScheduledItem, atMs: number) =>
      call<QueueScheduledItem[]>("add_queue_scheduled", { item, at: atMs }),
    /** Move a scheduled item into the immediate queue; returns the new now-list. */
    promote: (id: string) =>
      call<QueueNowItem[]>("promote_queue_scheduled", { id }),
    /**
     * Re-schedule an item to a new local epoch-ms `atMs` (used to recover a
     * "missed" schedule, or change the time). Clears the missed flag. Rejects a
     * past time. `when`/`rel` are the display strings for the new moment.
     */
    reschedule: (id: string, atMs: number, when: string, rel: string) =>
      call<QueueScheduledItem[]>("reschedule_queue_scheduled", {
        id,
        at: atMs,
        when,
        rel,
      }),
    /**
     * Persist the now-queue order (drag / priority change); returns the list.
     * Running items stay pinned to the front by the backend.
     */
    reorderNow: (orderedIds: string[]) =>
      call<QueueNowItem[]>("reorder_queue_now", { orderedIds }),
    /**
     * 현재 "최대 작동가능 작업 수"(now 큐 동시 작업 상한)를 읽는다(#284). 0 = 무제한.
     */
    getConcurrencyLimit: () => call<number>("get_now_concurrency_limit", {}),
    /**
     * "최대 작동가능 작업 수"를 저장한다(#284). 0 = 무제한, N = 동시 작업을 N개로 제한.
     * 워커는 claim 시점마다 다시 읽으므로 낮춰도 이미 돌고 있는 작업은 멈추지 않는다.
     */
    setConcurrencyLimit: (limit: number) =>
      call<void>("set_now_concurrency_limit", { limit }),
  },
  stocks: {
    list: () => call<Stock[]>("list_stocks"),
    search: (query: string) =>
      call<StockCandidate[]>("search_stocks", { query }),
  },
  // 종목토론방 종목 선택 화면 — 네이버 모바일(m.stock.naver.com) 종목 데이터.
  forumStocks: {
    /**
     * 카테고리(토론/거래대금/인기/상승/하락/거래량) × 거래소(krx/nxt)
     * × 시장(all/kospi/kosdaq) 한 페이지. 토론은 시장 구분이 무시된다.
     */
    list: (
      category: ForumStockCategory,
      exchange: StockExchange,
      market: StockMarket,
      page: number,
    ) =>
      call<ForumStockPage>("list_forum_stocks", {
        category,
        exchange,
        market,
        page,
      }),
    /** 검색어 포함 국내 종목 한 페이지(80개 상한 없음). */
    search: (query: string, page: number) =>
      call<ForumStockPage>("search_forum_stocks", { query, page }),
  },
  activity: {
    list: () => call<ActivityItem[]>("list_activity"),
    append: (kind: "success" | "error" | "info", text: string) =>
      call<void>("append_activity", { kind, text }),
  },
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
     * Distribute the comment pool across the targets (backend shuffles & deals
     * one comment per target — issue #98), then run the jobs sequentially;
     * returns one slim outcome per job. Each target carries a numeric
     * `cafeId`/`articleId` (from a just-posted article or a parsed URL); one job
     * failing does not stop the rest.
     */
    runCommentJobs: (req: CommentDistributionRequest) =>
      call<CommentPublishOutcome[]>("run_comment_jobs", { req }),
    /**
     * List every cafe `accountId` has joined (crawled across all pages),
     * using its session cookie. Rejects with the backend's error envelope
     * on failure. Backs an account-driven "가입 카페 자동 로드" flow.
     */
    listJoined: (accountId: string) =>
      call<JoinedCafe[]>("list_joined_cafes", { accountId }),
    /**
     * List a cafe's articles sorted by `sortBy` ("latest" | "popular"), using
     * `accountId`'s session cookie. Backs the comment-target 최신글/인기글 flow:
     * the modal takes the top-N of the returned `articles`. Rejects with the
     * backend's error envelope on failure.
     */
    listArticles: (cafeId: number, sortBy: SortBy, accountId: string) =>
      call<ArticleListResponse>("list_cafe_articles", {
        cafeId: String(cafeId),
        sortBy,
        accountId,
      }),
  },
  bands: { list: () => call<Band[]>("list_bands") },
  // 밴드(band.us) 가입+게시 — 순수 HTTP(md 서명). 링크로 가입 후 글/댓글 게시.
  band: {
    publish: (request: BandPublishRequest) =>
      call<BandPublishOutcome>("band_publish", { ...request }),
    /** 밴드 댓글 전용 — 기존 글(최신글/인기글) 상위 count개에 댓글을 단다. */
    comment: (request: BandCommentRequest) =>
      call<BandCommentOutcome>("band_comment", { ...request }),
    /** 링크(band_no)로 실제 밴드명을 조회한다(저장 시 표시용). accountId=band 쿠키 키. */
    resolveName: (accountId: string, bandLink: string) =>
      call<string>("band_resolve_name", { accountId, bandLink }),
    /** 밴드 게시 결과를 알림(게시 배치)에 기록한다(종토방처럼 알림에 표시되도록). */
    recordBatch: (input: {
      title: string;
      body: string;
      comment: string;
      runPost: boolean;
      runComment: boolean;
      items: {
        target: string;
        loginId: string;
        ok: boolean;
        msg: string;
        /** 실패 행의 "자세히 보기" trace(런타임 backtrace 포함, #199). */
        trace?: string;
      }[];
    }) => call<void>("record_band_batch", { ...input }),
  },
  diagnostics: {
    /** Probe Chrome install/version + ADB device connection (UI 새로고침). */
    getStatus: () => call<EnvironmentStatus>("get_environment_status"),
    /** Chrome 미설치 안내 카드의 "설치 페이지 열기" — 공식 다운로드 페이지를 기본 브라우저로 연다. */
    openChromeDownload: () => call<void>("open_chrome_download"),
    openUrl: (url: string) => call<void>("open_url", { url }),
  },
  app: {
    /** 부팅 자동 시작(OS 로그인 시 자동 실행) 등록 여부. */
    getAutostart: () => call<boolean>("get_autostart_enabled"),
    /** 부팅 자동 시작 등록 on/off. 갱신된 상태를 돌려준다. */
    setAutostart: (enabled: boolean) =>
      call<boolean>("set_autostart", { enabled }),
  },
  // 종목토론방(forum) 즉시 게시 — 네이버 증권 토론방 패킷 게시 엔진 호출.
  forum: {
    /** 게시 엔진이 붙을 Chrome DevTools 엔드포인트. 백엔드가 단일 출처(프론트 상수 아님). */
    endpoint: () => call<{ host: string; port: number }>("forum_endpoint"),
    publishNow: (request: ForumPublishRequest) =>
      call<ForumPublishResult[]>("run_forum_publish_now", { request }),
  },
  // 엑셀(.xlsx) 내보내기/가져오기 — Rust에서 파일 처리, 프론트에서 경로 공급.
  excel: {
    exportAccounts: (path: string) =>
      call<void>("export_accounts_xlsx", { path }),
    exportActivity: (path: string) =>
      call<void>("export_activity_xlsx", { path }),
    importAccounts: (path: string) =>
      call<ImportSummary>("import_accounts_xlsx", { path }),
    importPosts: (path: string) =>
      call<ImportSummary>("import_posts_xlsx", { path }),
  },
  // 네이버 로그인 자동화(CDP). 계정 ID/PW로 로그인해 쿠키를 저장한다. 실제 로그인 실행은
  // 즉시 처리 대기열(now 큐)에서 처리된다 — `queue.addNow`에 plan.login을 담아 적재한다(#210).
  auth: {
    bootstrap: () => call<unknown>("bootstrap_runtime"),
    saveAccounts: (accounts: AuthAccount[]) =>
      call<AuthAccount[]>("save_accounts", { accounts }),
    /** 로그인 없이 연결된 폰의 비행기모드만 토글해 IP를 회전시킨다(#247).
     * 회전 전후 IP와 변경 여부를 돌려준다 — 호출부가 토스트·알림에 표시한다. */
    rotateIp: () =>
      call<{ before: string; after: string; changed: boolean }>("rotate_ip"),
  },
};
