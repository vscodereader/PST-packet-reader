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

/** "좋아요"의 (계정×링크)별 결과(백엔드 LikeOutcome 미러). accountId는 loginId. */
export interface LikeOutcome {
  accountId: string;
  postUrl: string;
  success: boolean;
  message: string;
}

/** "조회수" 부스트의 링크별 결과(백엔드 ViewBoostOutcome 미러). */
export interface ViewBoostOutcome {
  link: string;
  /** 요청한 반복 횟수(N). */
  requested: number;
  /** 실제로 "열기→완전로딩→종료"까지 끝낸 횟수. */
  completed: number;
  /** requested 전량 성공 여부. */
  success: boolean;
  message: string;
}

/** 종목토론방 글 신고 사유(백엔드 REPORT_REASONS 미러, service=FIN 실측 7개). */
export interface ReportReason {
  /** 신고 사유 코드(예: "AA01") — reportReasonCode로 전송된다. */
  code: string;
  /** 사용자 표시 문구. */
  label: string;
}

/** 종목토론방 신고 사유 7개(설계서 §2.4 실측). UI 라디오·검증에 그대로 쓴다. */
export const REPORT_REASONS: ReportReason[] = [
  { code: "AA01", label: "혐오/차별적/생명경시/욕설 표현입니다" },
  { code: "AA29", label: "스팸홍보/도배입니다" },
  { code: "AA14", label: "음란물입니다" },
  { code: "AA68", label: "불법정보를 포함하고 있습니다" },
  { code: "AA33", label: "청소년에게 유해한 내용입니다" },
  { code: "AA24", label: "개인정보가 노출되었습니다" },
  { code: "AB28", label: "불쾌한 표현이 있습니다" },
];

/** "신고하기"의 (계정×링크)별 결과(백엔드 ReportOutcome 미러). accountId는 loginId. */
export interface ReportOutcome {
  accountId: string;
  link: string;
  success: boolean;
  message: string;
}

/** 신고 배치 완료 이벤트(report-finished) 페이로드(백엔드 ReportFinished 미러). */
export interface ReportFinished {
  total: number;
  succeeded: number;
  outcomes: ReportOutcome[];
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
/** 링크(oglink) 메타(백엔드 OglinkMeta, camelCase). */
export interface OglinkMeta {
  /** 정규화된 URL(oglinkSign이 서명한 값) — 링크 블록의 link로 써야 발행이 통과. */
  url: string;
  title: string;
  domain: string;
  description: string;
  thumbnailSrc: string;
  thumbnailWidth: number;
  thumbnailHeight: number;
  oglinkSign: string;
}

/** 장소 검색 결과 1건(백엔드 PlaceResult, camelCase). */
export interface PlaceResult {
  id: string;
  name: string;
  tel: string;
  roadAddress: string;
  address: string;
  /** 경도(x). */
  x: string;
  /** 위도(y). */
  y: string;
  /** place.type(예: "s"). */
  placeType: string;
  thumUrl: string;
}

/** 정적 지도 이미지 URL(백엔드 StaticMapResult). */
export interface StaticMapResult {
  src: string;
}

/** 스티커 팩 1개(백엔드 StickerPack). */
export interface StickerPack {
  packCode: string;
  stickerCount: number;
  isFree: boolean;
}

/** 파일 업로드 결과(백엔드 UploadedFile). */
export interface UploadedFile {
  fileId: string;
  fileName: string;
  fileSize: number;
}

/** 사진 업로드 결과(백엔드 UploadedImage). */
export interface UploadedImage {
  src: string;
  path: string;
  domain: string;
  fileSize: number;
  width: number;
  height: number;
  originalWidth: number;
  originalHeight: number;
  fileName: string;
}

/** 블로그 새 글 발행 결과(백엔드 BlogWriteResult, camelCase). */
export interface BlogWriteResult {
  /** 게시글 번호. 예약 발행은 아직 없어 null일 수 있다. */
  logNo: string | null;
  /** 게시글/리다이렉트 URL. */
  redirectUrl: string;
}

export const ipc = {
  accounts: {
    list: () => call<Account[]>("list_accounts"),
    add: (account: Account) => call<Account[]>("add_account", { account }),
    update: (account: Account) =>
      call<Account[]>("update_account", { account }),
    remove: (ids: string[]) => call<Account[]>("delete_accounts", { ids }),
    /**
     * 계정관리 "쿠키만료" 카운트다운용: 로그인 유지 쿠키의 만료 시각(unix seconds) 최댓값.
     * 세션 쿠키만 있거나 로그인 이력이 없으면 null. `id`는 쿠키 파일 키(loginId).
     */
    cookieExpiry: (id: string) =>
      call<number | null>("account_cookie_expiry", { id }),
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
    /**
     * 실행 중인 게시큐 1개를 완전 종료(kill)한다(설계서 08). 취소 신호를 켜서 실행 중 게시
     * 루프가 종목 사이·대기 중에 스스로 멈추고(Chrome은 정상 정리, 고아 없음), 큐에서 항목을
     * 제거해 다음 대기 큐가 즉시 승계된다. 대기 아이템 취소는 `cancelNow`.
     */
    killNow: (id: string) => call<QueueNowItem[]>("kill_queue_now", { id }),
    // 종료(완료/실패) 아이템을 모두 큐에서 치운다(#1, "완료 항목 지우기").
    clearDoneNow: () => call<QueueNowItem[]>("clear_done_queue_now"),
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
  // 네이버 블로그 새 글 발행 — 순수 HTTP(RabbitWrite). 로그인 저장 쿠키로 게시(크롬 안 뜸, 종토 미러).
  blog: {
    /** 블로그명(도메인) 사용 가능 여부. true=사용가능 / false=이미 사용중. */
    checkName: (accountId: string, domainId: string) =>
      call<boolean>("blog_check_name", { accountId, domainId }),
    /** 기존 블로그에 새 글 발행. 성공 시 게시글 번호(logNo)·링크를 돌려준다. */
    publish: (input: {
      accountId: string;
      blogId: string;
      title: string;
      content: string;
      /** 공개설정: 0=전체공개·1=이웃·2=서로이웃·3=비공개(생략 시 전체공개). */
      openType?: number;
      commentYn?: boolean;
      searchYn?: boolean;
      sympathyYn?: boolean;
      /** 태그: # 없이 공백구분 단어("첫글 인생"). */
      tags?: string;
      noticePostYn?: boolean;
      /** 예약 발행 시각(생략/undefined면 현재 발행). */
      reserve?:
        | {
            year: number;
            month: number;
            date: number;
            hour: number;
            minute: number;
          }
        | undefined;
      /** 편집기 툴바 블록(있으면 documentModel components[]로 발행, 없으면 content 문단). */
      blocks?: unknown[] | undefined;
    }) => call<BlogWriteResult>("blog_publish", { ...input }),
    /** 링크(oglink) 메타데이터 조회(링크 블록 삽입). */
    oglink: (accountId: string, url: string) =>
      call<OglinkMeta>("blog_oglink", { accountId, url }),
    /** 장소 검색(장소 블록 삽입). */
    places: (accountId: string, query: string) =>
      call<PlaceResult[]>("blog_places", { accountId, query }),
    /** 장소 좌표의 정적 지도 URL(장소 블록 썸네일). */
    staticmap: (accountId: string, latitude: string, longitude: string) =>
      call<StaticMapResult>("blog_staticmap", {
        accountId,
        latitude,
        longitude,
      }),
    /** 스티커 팩 목록(스티커 블록 삽입). */
    stickers: (accountId: string) =>
      call<StickerPack[]>("blog_stickers", { accountId }),
    /** 스티커 팩 내 seq 목록(스티커 블록 삽입). */
    stickerSeqs: (accountId: string, packCode: string) =>
      call<number[]>("blog_sticker_seqs", { accountId, packCode }),
    /** 로컬 파일 업로드(파일 블록 삽입). */
    uploadFile: (accountId: string, filePath: string) =>
      call<UploadedFile>("blog_upload_file", { accountId, filePath }),
    /** 로컬 이미지 업로드(사진 블록 삽입). */
    uploadPhoto: (accountId: string, filePath: string) =>
      call<UploadedImage>("blog_upload_photo", { accountId, filePath }),
  },
  diagnostics: {
    /** Probe Chrome install/version + ADB device connection (UI 새로고침). */
    getStatus: () => call<EnvironmentStatus>("get_environment_status"),
    /** Chrome 미설치 안내 카드의 "설치 페이지 열기" — 공식 다운로드 페이지를 기본 브라우저로 연다. */
    openChromeDownload: () => call<void>("open_chrome_download"),
    openUrl: (url: string) => call<void>("open_url", { url }),
  },
  system: {
    /**
     * 우리 임시 프로필로 아직 실행 중인 Chrome 프로세스 개수(고아 헬퍼 포함). 사용자가
     * 작업관리자를 열지 않아도 "실행 중 크롬 N개"를 앱에서 보게 하는 지표. 조회 실패 시 0.
     */
    runningChromeCount: () => call<number>("running_chrome_count"),
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
    /** 닉네임 랜덤(설계서 §2) UI용: 이 계정의 닉네임 변경 잔여 횟수(5회 상한 중 남은 횟수)를
     * 조회한다. 조회 실패/필드 없음이면 null. loginId는 쿠키 파일 키. */
    nicknameRemaining: (loginId: string) =>
      call<number | null>("forum_nickname_remaining", { loginId }),
    publishNow: (request: ForumPublishRequest) =>
      call<ForumPublishResult[]>("run_forum_publish_now", { request }),
    /** 여러 게시글 링크 × 선택한 계정들의 모든 조합에 좋아요를 누른다(페이지 이동 없이 API 전용).
     * accountIds는 계정의 loginId(쿠키 파일 키). (계정×링크)별 성공/실패를 돌려준다. */
    like: (postUrls: string[], accountIds: string[]) =>
      call<LikeOutcome[]>("like_discussion_post", { postUrls, accountIds }),
    /** 좋아요와 동일한 경로로 **싫어요**(reactionType="bad")를 누른다 — 패킷상 API만 다르다. */
    dislike: (postUrls: string[], accountIds: string[]) =>
      call<LikeOutcome[]>("dislike_discussion_post", { postUrls, accountIds }),
  },
  // 조회수 부스트 — 각 링크를 시크릿창으로 repeats번 여닫아 조회수를 올린다(#400). 로그인 불필요.
  viewCount: {
    /** 여러 게시글 링크를 각각 `repeats`번 시크릿창으로 여닫는다(열기→완전로딩→종료).
     * 링크별 성공/진행 결과를 돌려준다. */
    boost: (links: string[], repeats: number) =>
      call<ViewBoostOutcome[]>("boost_view_count", { links, repeats }),
  },
  // 종목토론방 글 신고하기(설계서 naver-report-design.md). 비차단 — 커맨드는 즉시 반환하고
  // 백그라운드로 n×m건을 신고한다. 결과는 report-finished 이벤트로 전달된다.
  report: {
    /** 링크 n개 × 계정 m개를 신고한다. accountIds는 loginId(쿠키 키), reasonCode는 사유 7개 중 하나.
     * rotateIp가 true면 계정 사이에 ADB로 IP를 회전하고 새 IP에서 재로그인한다. 즉시 반환(비차단). */
    submit: (
      links: string[],
      accountIds: string[],
      reasonCode: string,
      rotateIp: boolean,
    ) =>
      call<void>("report_posts", { links, accountIds, reasonCode, rotateIp }),
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
    /** '수동추가': headed Chrome을 띄워 사람이 직접 네이버 로그인한다(자동 타이핑·IP 회전 없음).
     * 성공하면 쿠키를 자동로그인과 동일하게 저장하고 계정 행을 status=Active로 추가한 뒤 그
     * 계정을 돌려준다. 취소/타임아웃/창 닫힘이면 오류를 던진다(호출부가 중립 토스트 표시). */
    manualAdd: () => call<Account>("manual_add_account"),
  },
  // 원격제어 에이전트(§6-2·§9). 하위 앱이 서버주소+기기코드로 등록하면 토큰을 받아 SSE 연결.
  agent: {
    register: (serverUrl: string, code: string) =>
      call<AgentStatus>("agent_register", { serverUrl, code }),
    status: () => call<AgentStatus>("agent_status"),
    unregister: () => call<void>("agent_unregister"),
  },
};

/** 에이전트 등록 상태(`src-tauri/src/agent`). */
export interface AgentStatus {
  configured: boolean;
  serverUrl: string;
  deviceName: string;
}
