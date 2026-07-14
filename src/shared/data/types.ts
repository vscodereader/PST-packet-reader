import type { PublishPlan } from "@/shared/bindings/PublishPlan";

export type ViewId =
  | "dashboard"
  | "posts"
  | "blog"
  | "queue"
  | "log"
  | "accounts"
  | "device-register";

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

export type PlatformId =
  | "forum"
  | "naver"
  | "blog"
  | "clip"
  | "band"
  | "instagram"
  | "threads";

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

// 백엔드 `AccountStatus`(ts-rs 생성 바인딩)와 일치시킨다. 이 손수 작성 union은 UI가
// 한곳에서 도메인 타입을 import하도록 유지하는 미러다.
export type AccountStatus =
  | "new"
  | "active"
  | "waiting"
  | "onHold"
  | "timedOut"
  | "badCredentials"
  | "challenge"
  | "relogin"
  | "blocked"
  | "error";

export interface Account {
  id: string;
  platform: PlatformId;
  loginId: string;
  pw: string;
  status: AccountStatus;
  /** 마지막 상태 변경 사유(차단/타임아웃 원문이나 조치 안내). 배지 tooltip에 표시. */
  statusMsg?: string;
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
  /** 특정 게시글(url) 댓글의 단일 대상 링크. 하위호환을 위해 유지한다(항상 commentUrls[0]과
   *  같은 값을 채운다). 새 코드는 여러 링크를 담는 commentUrls를 쓴다. */
  commentUrl?: string;
  /** 특정 게시글(url) 댓글의 대상 링크들. 각 링크의 글마다 작성한 댓글이 달린다. 비어있지
   *  않은 값만 의미가 있으며, 단일 링크(기존)는 길이 1 배열로 표현된다. */
  commentUrls?: string[];
  commentCount?: number;
}

/** A single account×destination upload target, expanded from a publish request. */
export interface PublishJob {
  key: string;
  platform: PlatformId;
  loginId: string;
  targetName: string;
  code?: string;
  /** band 플랫폼 전용: 가입·게시 링크. 잡 생성 시점에 동결해, 이후 밴드명이 같은
   *  다른 밴드와 헷갈려 링크를 잘못 재조회하는 일을 막는다(이름 lookup 제거). */
  bandLink?: string;
  /** naver(cafe) 전용: 게시판 링크에서 파싱한 카페·게시판 식별자(잡 생성 시 동결).
   *  게시판 목록은 쿠키 필수라 시드 로그인 없이 못 받으므로, 사용자가 붙여넣은
   *  게시판 URL에서 직접 뽑는다. boardType은 게시 시점 백엔드가 해결한다. */
  cafeId?: number;
  menuId?: number;
  /** naver(cafe) "특정 게시글" 댓글 전용: 대상 글의 articleId(잡 생성 시 동결). cafeId와 짝을
   *  이뤄 그 글에 댓글을 단다. 링크마다 다른 글이므로 잡(=링크)마다 따로 갖는다. */
  articleId?: number;
  /** forum(종목토론방) 전용: "특정 게시글" 댓글 대상의 글 URL(잡 생성 시 동결). 채워지면
   *  워커가 이 URL의 글에 직접 댓글을 단다(종목 선택·랜덤 글 없이). 그 외 forum 잡은 비운다. */
  commentUrl?: string;
  /** blog(네이버블로그) 전용(#271): 댓글 달 글의 blogId(문자열)+logNo(숫자 문자열, 잡 생성 시
   *  동결). 블로그는 댓글 전용이라 이 한 글이 곧 대상이다. "최신 N개" 모드(#279)면 logNo는
   *  비고 blogCount(+categoryNo)가 채워져, 워커가 그 블로그의 최신 글 상위 N개에 댓글을 단다. */
  blogId?: string;
  logNo?: string;
  /** blog "최신 N개" 모드(#279): 댓글을 달 최신 글 개수. 채워지면 logNo 없이 블로그 자체가 대상. */
  blogCount?: number;
  /** blog "최신 N개" 모드(#279): 글 목록을 좁힐 카테고리 번호(있으면). 없으면 전체. */
  categoryNo?: number;
  /** clip(네이버 클립) 전용(#클립): 창작자 핸들(@ 제외). 채워지면 워커가 그 창작자의 최신
   *  미디어 상위 clipCount개에 댓글을 단다(댓글 전 클립 프로필 생성은 백엔드가 보장). */
  clipHandle?: string;
  /** clip "최신 N개" 모드: 댓글을 달 최신 미디어 개수. */
  clipCount?: number;
  /** clip 탭: "video"면 영상만(?tab=video), 그 외/미지정이면 전체(?tab=all). */
  clipMediaType?: string;
  board: string;
  status: AccountStatus;
}

export interface PublishResult extends PublishJob {
  ok: boolean;
  msg: string;
  /** 실패 시 "자세히 보기"용 개발자 trace(런타임 backtrace 포함, #199). 밴드 즉시 게시는
   *  백엔드 command가 reason+trace를 분리해 주므로 이 필드에 trace를 싣는다. */
  trace?: string;
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
  // "done": 실행이 끝난(완료/실패/도중 차단) 종료 상태 — 큐에서 바로 지우지 않고 결과를
  // 카드로 남겨 사용자가 알림 없이 확인하게 한다(#1, Rust QueueState::Done과 일치).
  state: "running" | "waiting" | "done";
  batchId?: string;
  progress?: [number, number];
  locs: QueueLocation[];
  /** 워커가 실제 게시에 쓰는 실행 페이로드. 표시 전용 아이템은 없음. */
  plan?: PublishPlan;
  /**
   * 워커가 phase별로 갱신하는 대상별 실시간 상태(진행 전/중/완료/실패). 알림 로그와
   * 같은 BatchItem 모델을 재사용해 진행 중 큐를 펼치면 SubLog로 보여준다. 대기 아이템은 빈 배열.
   */
  items: BatchItem[];
}

export interface QueueScheduledItem {
  id: string;
  title: string;
  kind: ModeValue;
  when: string;
  rel: string;
  /** 예약 시각(epoch ms). 자동 트리거 스케줄러의 기준값. */
  at: number;
  /** 앱 종료 중 시각이 지나 미발행된 상태. 사용자가 재예약/취소한다. */
  missed: boolean;
  locs: QueueLocation[];
  /** 워커가 실제 게시에 쓰는 실행 페이로드. 표시 전용 아이템은 없음. */
  plan?: PublishPlan;
}

/** 한 대상에 실제로 게시된 내용(종목별 토큰 치환 후). 글이 빛삭돼도 무엇을 보냈는지
 *  완료 로그에서 확인할 수 있게 종목별로 보존한다. */
export interface PostedContent {
  title: string;
  body: string;
  comment?: string;
  url?: string;
}

export interface BatchItem {
  platform: PlatformId;
  target: string;
  code?: string;
  board?: string;
  loginId: string;
  status: "success" | "fail" | "running" | "waiting" | "skip" | "stopped";
  msg: string;
  trace?: string;
  /** 종목별 실제 게시 내용(제목/본문/댓글/URL). 게시 성공 시에만. */
  posted?: PostedContent;
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
