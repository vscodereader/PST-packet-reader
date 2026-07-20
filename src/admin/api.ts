// Admin 웹 → 중앙 서버(`server/`) HTTP 클라이언트. 설계 §8 transport(HttpSse)의 Admin측 구현.
//
// 핵심: 서버가 떠 있지 않은 **오프라인 미리보기**에서도 화면이 깨지지 않게, fetch 실패(연결 불가)는
// `OfflineError`로 던진다. 각 화면은 이를 잡아 기존 더미 데이터로 폴백한다(UI 무손상).
// 서버 주소 결정: `VITE_ADMIN_API`(명시 주입) 우선 → 없으면 dev는 로컬 서버(localhost:8080),
// 프로덕션 빌드는 **자기를 서빙한 origin**(window.location.origin). 서버(server/)가 Admin 웹을
// 자기 origin에서 서빙하므로(routes.rs), 배포본은 별도 URL 주입 없이 같은 도메인으로 요청이 가고
// (CORS 불필요), device-connection 화면에 뜨는 "하위 접속 주소"도 실제 배포 URL로 정확해진다.
function defaultBase(): string {
  if (import.meta.env.DEV) return "http://localhost:8080";
  return typeof window !== "undefined" && window.location.origin
    ? window.location.origin
    : "";
}

const BASE =
  (import.meta.env.VITE_ADMIN_API as string | undefined)?.replace(/\/+$/, "") ??
  defaultBase();

const TOKEN_KEY = "pstmacro.admin.token";
const LOGIN_KEY = "pstmacro.admin.login";
const ROLE_KEY = "pstmacro.admin.role";

export type Role = "super" | "operator";
export type DeviceState = "online" | "rotating" | "reconnecting" | "offline";

/** 서버가 4xx/5xx로 거부(사유 메시지 포함). */
export class ApiError extends Error {}
/** 서버에 연결 자체가 안 됨(미리보기 오프라인) → 더미 폴백 신호. */
export class OfflineError extends Error {}

export function getToken(): string | null {
  return localStorage.getItem(TOKEN_KEY);
}
export function getRole(): Role | null {
  return localStorage.getItem(ROLE_KEY) as Role | null;
}
export function getLoginId(): string | null {
  return localStorage.getItem(LOGIN_KEY);
}
export function isLoggedIn(): boolean {
  return getToken() != null;
}
export function logout(): void {
  localStorage.removeItem(TOKEN_KEY);
  localStorage.removeItem(LOGIN_KEY);
  localStorage.removeItem(ROLE_KEY);
}
/** 에러가 "서버 연결 불가"인지 — 화면이 더미 폴백할지 판단. */
export function isOffline(e: unknown): boolean {
  return e instanceof OfflineError;
}

async function request<T>(
  method: string,
  path: string,
  body?: unknown,
): Promise<T> {
  const headers: Record<string, string> = {};
  const token = getToken();
  if (token != null) headers["Authorization"] = `Bearer ${token}`;
  if (body !== undefined) headers["Content-Type"] = "application/json";

  // exactOptionalPropertyTypes: body가 undefined면 키 자체를 넣지 않는다.
  const init: RequestInit = { method, headers };
  if (body !== undefined) init.body = JSON.stringify(body);
  let res: Response;
  try {
    res = await fetch(`${BASE}${path}`, init);
  } catch {
    // 네트워크 도달 실패(서버 미기동 등) → 오프라인.
    throw new OfflineError("서버에 연결할 수 없습니다");
  }

  if (!res.ok) {
    let msg = `요청 실패 (${res.status})`;
    try {
      const j: unknown = await res.json();
      if (j && typeof j === "object" && "error" in j) {
        const e = (j as { error?: unknown }).error;
        if (typeof e === "string") msg = e;
      }
    } catch {
      /* 본문 파싱 실패는 무시 */
    }
    throw new ApiError(msg);
  }
  if (res.status === 204) return undefined as T;
  return (await res.json()) as T;
}

// ── 응답/요청 타입(서버 DTO와 일치, camelCase) ──
export interface LoginResult {
  token: string;
  loginId: string;
  role: Role;
  mustChangePassword: boolean;
}
export interface OperatorsResp {
  operators: { loginId: string; role: Role }[];
  pending: string[];
}
export interface DeviceDto {
  id: string;
  name: string;
  connected: boolean;
  ip: string | null;
  lastSeen: string;
  state: DeviceState;
}
// 등록 이력(#444) — 통신로그가 하위com(기기)별 등록 이름을 날짜순으로 보여준다.
export interface DeviceRegistrationDto {
  machineId: string | null;
  deviceId: string;
  name: string;
  registeredAt: string;
}
export interface DeviceCodeResp {
  code: string;
  serverUrl: string | null;
  expiresInSecs: number;
}
export interface AccountDto {
  id: string;
  loginId: string;
  // 계정 플랫폼("forum"/"naver"/"blog"/"clip"/"band"). 빈값=forum. 카페=naver.
  platform?: string;
}
export interface ImportResult {
  imported: number;
  skipped: number;
  total: number;
}
export interface DistributeResult {
  assignments: { deviceId: string; deviceName: string; count: number }[];
  moved: number;
}
// 예약 게시(07-게시명령 4단계) — 서버가 보관한 예약 목록(표시 전용). 프론트 ScheduledItem과 동일 모양.
export interface ScheduledDto {
  id: string;
  deviceName: string;
  postTitle: string;
  targetLabel: string;
  detail: string;
  at: number;
}
// 하위 인벤토리(§8 신규 데이터흐름, 07-게시명령 3단계) — 하위가 보고한 글목록·성공계정.
export interface InvPostDto {
  id: string;
  title: string;
  // 글 종류: "post"(글)·"comment"(댓글)·"both"(글+댓글). 옛 하위는 빈값 → post로 본다.
  kind?: string;
  // 댓글 내용 미리보기 — 댓글은 제목이 없어(당연) 이 내용을 제목 대신 보여준다(글이 제목 보여주듯).
  excerpt?: string;
  // 작성한 댓글 수(≥2 게이트용, 15-기타명령 §3) — 이 값이 2 이상일 때만 닉네임 랜덤 체크박스를 보인다.
  commentCount?: number;
}
// 인벤토리 계정 1건(전체 — loginId·platform·status). 카페 게시명령이 로그인 무관 카페 계정을
// 전부 쓰기 위함. 옛 하위는 안 보낼 수 있어 optional.
export interface InvAccountDto {
  loginId: string;
  platform?: string;
  status?: string;
}
export interface DeviceInventoryDto {
  posts: InvPostDto[];
  accounts: string[]; // 로그인 성공(Active) loginId — 종토 게시명령용(기존)
  accountRows?: InvAccountDto[]; // 전체 계정(platform·status) — 카페 게시명령용
  receivedAt: string | null;
}
// 실행큐 스냅샷(설계서 08 §10-2) — 하위가 보고한 실행/대기 게시큐. 중지 명령 화면이 폴링해
// 하위 화면과 동일한 내용을 실시간으로 보여주고, 각 큐 옆 [중지]가 그 id로 kill을 보낸다.
export interface QueueItemDto {
  id: string;
  title: string;
  kind: string; // 종토/카페/밴드/블로그/클립/로그인/게시
  state: string; // running | waiting
  done: number;
  total: number;
  loginIds: string[];
}
export interface DeviceQueueStateDto {
  items: QueueItemDto[];
  receivedAt: string | null;
}
// 종목 프록시(§8 신규 데이터흐름, 07-게시명령 2단계) — 서버 DTO와 일치(camelCase).
export interface ForumStockDto {
  code: string;
  name: string;
  exchange: string;
  price: string;
  changeRate: string;
  changeType: string;
  isHotDiscussion: boolean;
}
export interface ForumStockPageDto {
  stocks: ForumStockDto[];
  totalCount: number;
  page: number;
  hasNext: boolean;
}
export interface AuditDto {
  ts: string;
  tag: string;
  dir: string;
  device: string;
  msg: string;
  level: string;
}
// 게시 결과 보고(§10-4-2) — 하위 LogBatch/BatchItem 모델 그대로(서버 DTO와 일치, camelCase).
export interface PostedDto {
  title: string;
  body: string;
  comment?: string;
  url?: string;
}
export interface PostItemDto {
  platform: string;
  target: string;
  loginId: string;
  status: string; // success | fail | skip | …(데스크톱 모델)
  msg: string;
  trace?: string;
  posted?: PostedDto;
}
export interface PostReportDto {
  device: string;
  deviceId: string;
  batchId: string;
  title: string;
  at: number; // 게시 완료 epoch ms
  // 결과 종류 태그(15-기타명령 §6-3) — 게시/좋아요/싫어요/조회수/IP. 옛 서버/게시는 기본 "게시".
  kind?: string;
  receivedAt: string;
  items: PostItemDto[];
}
// 로그인 결과 보고(§10-4-1) — 4분류 + 누적(서버 DTO와 일치, camelCase).
export interface LoginLineDto {
  loginId: string;
  pw: string;
  reason?: string; // 보류사유·실패사유. 대기초과는 없음.
  trace?: string; // 실패 백트레이스(게시 결과와 동일하게 "자세히 보기"용).
}
export interface LoginReportDto {
  device: string;
  deviceId: string;
  receivedAt: string;
  batch: {
    success: number;
    onhold: LoginLineDto[];
    timedout: LoginLineDto[];
    failed: LoginLineDto[];
  };
  cumulative: {
    received: number;
    success: number;
    onhold: number;
    timedout: number;
    failed: number;
  };
  // 이 분배에서 등록된 계정 수와, 그중 로그인 엔진이 본 수(§10-1 등록 확인).
  registered: number;
  registeredVisible: number;
}

// 중지(kill) 요약(설계서 08 §10-3) — 결과보고 "중지" 섹션. "N개 중 M개 진행 후 중지".
export interface StopLineDto {
  loginId: string;
  pw: string;
  title: string;
  done: number;
  total: number;
}
export interface StopReportDto {
  device: string;
  deviceId: string;
  receivedAt: string;
  stopped: StopLineDto[];
}

// 날짜별 결과(결과보고 날짜 분류) — 그 날(KST)의 로그인 4분류 + 중지. 날짜를 골라 그 날만 본다.
export interface DailyResultDto {
  date: string; // YYYY-MM-DD
  success: number;
  onhold: LoginLineDto[];
  timedout: LoginLineDto[];
  failed: LoginLineDto[];
  stopped: StopLineDto[];
}
export interface DeviceDailyDto {
  device: string;
  deviceId: string;
  days: DailyResultDto[]; // 최신 날짜 우선
}

export const api = {
  baseUrl: BASE,
  auth: {
    async login(loginId: string, pw: string): Promise<LoginResult> {
      const r = await request<LoginResult>("POST", "/auth/login", {
        loginId,
        pw,
      });
      localStorage.setItem(TOKEN_KEY, r.token);
      localStorage.setItem(LOGIN_KEY, r.loginId);
      localStorage.setItem(ROLE_KEY, r.role);
      return r;
    },
    signup(loginId: string, pw: string): Promise<unknown> {
      return request("POST", "/auth/signup", { loginId, pw });
    },
    changePassword(currentPw: string, newPw: string): Promise<unknown> {
      return request("POST", "/auth/change-password", { currentPw, newPw });
    },
    logout,
  },
  operators: {
    list(): Promise<OperatorsResp> {
      return request("GET", "/admin/operators");
    },
    approve(id: string): Promise<unknown> {
      return request(
        "POST",
        `/admin/operators/${encodeURIComponent(id)}/approve`,
      );
    },
    reject(id: string): Promise<unknown> {
      return request(
        "POST",
        `/admin/operators/${encodeURIComponent(id)}/reject`,
      );
    },
    remove(id: string): Promise<unknown> {
      return request("DELETE", `/admin/operators/${encodeURIComponent(id)}`);
    },
    resetPassword(id: string, newPw: string): Promise<unknown> {
      return request(
        "POST",
        `/admin/operators/${encodeURIComponent(id)}/reset-password`,
        {
          newPw,
        },
      );
    },
  },
  devices: {
    issueCode(): Promise<DeviceCodeResp> {
      return request("POST", "/admin/device-codes");
    },
    list(): Promise<DeviceDto[]> {
      return request("GET", "/devices");
    },
    // 등록 이력(#444) — 하위com별 등록 이름·등록시각.
    registrations(): Promise<DeviceRegistrationDto[]> {
      return request("GET", "/admin/device-registrations");
    },
    remove(id: string): Promise<unknown> {
      return request("DELETE", `/devices/${encodeURIComponent(id)}`);
    },
    command(id: string, type: string, commandId?: string): Promise<unknown> {
      return request("POST", `/devices/${encodeURIComponent(id)}/commands`, {
        type,
        commandId,
      });
    },
    // 게시명령 화면 실데이터(07-게시명령 3단계) — 이 하위의 글목록·성공계정.
    inventory(id: string): Promise<DeviceInventoryDto> {
      return request("GET", `/devices/${encodeURIComponent(id)}/inventory`);
    },
    // 중지 명령 화면 실데이터(설계서 08 §10-2) — 이 하위의 실행/대기 게시큐 스냅샷.
    queueState(id: string): Promise<DeviceQueueStateDto> {
      return request("GET", `/devices/${encodeURIComponent(id)}/queue-state`);
    },
    // 닉네임 잔여 횟수 실시간 조회 요청(15-기타명령 §3·§6-2) — 하위가 계정별 remainingEditCount를
    // 조회해 회신하도록 SSE 명령을 내려보낸다. 회신은 nicknameRemaining(GET) 폴링으로 읽는다.
    queryNicknameRemaining(
      id: string,
      loginIds: string[],
    ): Promise<{ ok: boolean; commandId: string }> {
      return request(
        "POST",
        `/devices/${encodeURIComponent(id)}/nickname-remaining`,
        { loginIds },
      );
    },
    // 닉네임 잔여 횟수 회신 폴링(15-기타명령 §3) — loginId → 남은횟수 | null(조회 실패/미회신).
    nicknameRemaining(id: string): Promise<Record<string, number | null>> {
      return request(
        "GET",
        `/devices/${encodeURIComponent(id)}/nickname-remaining`,
      );
    },
  },
  // 기타 명령(15-기타명령 §2) — 고른 하위 1대에 좋아요/싫어요/조회수/IP변경을 내려보낸다. 전부
  // 서버 generic issue_command(`/devices/:id/commands`)를 재사용한다(온라인 게이트 409 공통). 결과는
  // post-report(종류 태그)로 결과 보고에, 원시 로그는 통신 로그에 뜬다. 페이로드: 좋아요/싫어요=
  // links×loginIds, 조회수=links×repeats, IP변경=없음.
  etc: {
    like(
      deviceId: string,
      links: string[],
      loginIds: string[],
    ): Promise<{ ok: boolean; commandId: string }> {
      return request(
        "POST",
        `/devices/${encodeURIComponent(deviceId)}/commands`,
        { type: "like_posts", links, loginIds },
      );
    },
    dislike(
      deviceId: string,
      links: string[],
      loginIds: string[],
    ): Promise<{ ok: boolean; commandId: string }> {
      return request(
        "POST",
        `/devices/${encodeURIComponent(deviceId)}/commands`,
        { type: "dislike_posts", links, loginIds },
      );
    },
    boostView(
      deviceId: string,
      links: string[],
      repeats: number,
    ): Promise<{ ok: boolean; commandId: string }> {
      return request(
        "POST",
        `/devices/${encodeURIComponent(deviceId)}/commands`,
        { type: "boost_view", links, repeats },
      );
    },
    rotateIp(deviceId: string): Promise<{ ok: boolean; commandId: string }> {
      return request(
        "POST",
        `/devices/${encodeURIComponent(deviceId)}/commands`,
        { type: "rotate_ip" },
      );
    },
  },
  stop: {
    // 중지 명령(설계서 08 §10) — 실행 중 게시큐 완전 종료. queueId=그 큐 1개, all=디바이스
    // 전체, loginId=계정. 서버가 그 하위 SSE로 kill_publish를 내려보내고 payload 원문을 통신로그에 남긴다.
    kill(req: {
      deviceId: string;
      commandId?: string;
      queueId?: string;
      all?: boolean;
      loginId?: string;
    }): Promise<{ ok: boolean; commandId: string }> {
      return request("POST", "/admin/kill", req);
    },
  },
  forumStocks: {
    // 종목 프록시(07-게시명령 2단계) — 서버가 네이버 공개 front-api를 프록시해 실제 종목 목록을 준다.
    // 서버가 종목 코드의 원천. Admin은 이 목록에서 불꽃우선 N을 골라 게시 명령을 만든다.
    list(req: {
      category: string;
      exchange?: string;
      market?: string;
      page?: number;
      q?: string; // 검색어(있으면 서버가 검색 경로 — §18-8-1 fetch_search_page 프록시).
    }): Promise<ForumStockPageDto> {
      const qs = new URLSearchParams({ category: req.category });
      if (req.exchange != null) qs.set("exchange", req.exchange);
      if (req.market != null) qs.set("market", req.market);
      if (req.page != null) qs.set("page", String(req.page));
      if (req.q != null && req.q !== "") qs.set("q", req.q);
      return request("GET", `/admin/forum-stocks?${qs.toString()}`);
    },
    // 검색 편의 래퍼(수동 종목 선택 모달용) — 카테고리는 서버가 검색 시 무시하나 필수 필드라 기본값.
    search(query: string, page: number): Promise<ForumStockPageDto> {
      return this.list({ category: "tradingValue", q: query, page });
    },
  },
  publish: {
    // 게시 명령(07-게시명령) — 하위 1대당 1묶음. 서버가 확정한 계정×종목을 그 하위로 내려보낸다.
    send(req: {
      deviceId: string;
      commandId?: string;
      postId: string;
      postTitle: string;
      targetLabel: string;
      split: boolean;
      mode?: string; // "post"|"comment"|"both" (빈값=post)
      target?: string; // "forum"(기본)|"naver"(카페)
      cafeBoards?: {
        cafeId: number;
        menuId?: number;
        articleId?: number;
        link?: string;
      }[]; // 카페 게시판/글 링크 파싱 결과(target=="naver")
      commentUrls?: string[]; // 댓글 모드(종토=특정게시글) URL들
      forumCommentDistribute?: boolean; // 종토 특정글 댓글을 계정들에 1개씩 나눠 답기(#403)
      blogLinks?: {
        blogId: string;
        logNo?: string; // 특정 글(있으면). 없으면 최신 N개.
        categoryNo?: number; // 최신 N개 글 목록 카테고리(있으면).
        count?: number; // 최신 N개(logNo 없을 때).
        link: string;
      }[]; // 블로그 댓글 대상(target=="blog")
      clipLinks?: {
        handle: string; // 창작자 핸들(@ 제외)
        mediaType?: string; // "video"면 영상만, 그 외/빈값=전체
        count?: number; // 최신 미디어 개수
        link: string;
      }[]; // 클립 댓글 대상(target=="clip")
      bandTargets?: {
        bandNo: string; // 밴드 식별자(라벨용)
        link: string; // 밴드 홈 또는 특정 글 URL(백엔드가 band_no/post_no 추출)
      }[]; // 밴드 게시 대상(target=="band")
      commentMode?: string; // 카페·밴드 댓글 대상: "url"(특정글)|"latest"(최신)|"popular"(인기)
      commentCount?: number; // 카페·밴드 최신/인기 댓글 개수(상위 N)
      commentNicknameRandom?: boolean; // 종토 댓글 닉네임 랜덤(15-기타명령 §3)
      contentChange?: {
        title: string;
        body: string;
        delaySec: number;
      } | null; // 종토 글 게시 후 내용변경(15-기타명령 §4)
      assignments: {
        loginId: string;
        stocks: { code: string; name: string }[];
      }[];
    }): Promise<{ ok: boolean; commandId: string }> {
      return request("POST", "/admin/publish", req);
    },
  },
  blogWrite: {
    // 블로그 새 글 발행(16-블로그새글) — Admin 편집기 툴바로 작성한 새 글을 그 하위로 내려보낸다.
    // 블로그 댓글(publish 경로)과 별개의 직접 발행 명령이다. blocks는 편집기 블록 배열(blocks.ts) —
    // 서버는 해석하지 않고 그대로 하위에 통과시키고, 하위가 documentModel로 변환해 RabbitWrite 발행한다.
    // targets는 계정별 (loginId, 블로그명). blogId가 비면 하위가 loginId를 블로그명으로 쓴다.
    send(req: {
      deviceId: string;
      commandId?: string;
      title: string;
      blocks: unknown[];
      settings: {
        openType: number; // 0=전체공개·1=이웃공개·2=서로이웃공개·3=비공개
        commentYn: boolean;
        searchYn: boolean;
        tags: string;
      };
      targets: { loginId: string; blogId: string }[];
    }): Promise<{ ok: boolean; commandId: string }> {
      return request("POST", "/admin/blog-write", req);
    },
  },
  scheduled: {
    // 예약 게시(07-게시명령 4단계) — 서버가 보관하고 스케줄러가 시각되면 발송. 목록/삭제도 서버가.
    create(req: {
      deviceId: string;
      postId: string;
      postTitle: string;
      targetLabel: string;
      split: boolean;
      mode?: string; // "post"|"comment"|"both" (빈값=post)
      target?: string; // "forum"(기본)|"naver"(카페)
      cafeBoards?: {
        cafeId: number;
        menuId?: number;
        articleId?: number;
        link?: string;
      }[];
      commentUrls?: string[];
      forumCommentDistribute?: boolean; // 종토 특정글 댓글을 계정들에 1개씩 나눠 답기(#403)
      blogLinks?: {
        blogId: string;
        logNo?: string;
        categoryNo?: number;
        count?: number;
        link: string;
      }[]; // 블로그 댓글 대상(target=="blog")
      clipLinks?: {
        handle: string;
        mediaType?: string;
        count?: number;
        link: string;
      }[]; // 클립 댓글 대상(target=="clip")
      bandTargets?: {
        bandNo: string;
        link: string;
      }[]; // 밴드 게시 대상(target=="band")
      commentMode?: string; // 카페·밴드 댓글 대상: "url"|"latest"|"popular"
      commentCount?: number; // 카페·밴드 최신/인기 댓글 개수(상위 N)
      commentNicknameRandom?: boolean; // 종토 댓글 닉네임 랜덤(15-기타명령 §3)
      contentChange?: {
        title: string;
        body: string;
        delaySec: number;
      } | null; // 종토 글 게시 후 내용변경(15-기타명령 §4)
      assignments: {
        loginId: string;
        stocks: { code: string; name: string }[];
      }[];
      at: number; // 발송 시각 epoch ms
      detail: string;
    }): Promise<{ ok: boolean; id: string }> {
      return request("POST", "/admin/scheduled", req);
    },
    list(): Promise<ScheduledDto[]> {
      return request("GET", "/admin/scheduled");
    },
    remove(id: string): Promise<unknown> {
      return request("DELETE", `/admin/scheduled/${encodeURIComponent(id)}`);
    },
  },
  accounts: {
    list(): Promise<AccountDto[]> {
      return request("GET", "/admin/accounts");
    },
    import(
      accounts: { loginId: string; pw: string; platform?: string }[],
    ): Promise<ImportResult> {
      return request("POST", "/admin/accounts/import", { accounts });
    },
    distribute(
      accountIds: string[],
      deviceIds: string[],
    ): Promise<DistributeResult> {
      return request("POST", "/admin/accounts/distribute", {
        accountIds,
        deviceIds,
      });
    },
    // 계정 상태/플랫폼 원격 편집(14-계정상태-관리) — 하위 accountRows 폴링본 대비 바뀐 행만 보낸다.
    // platform/status는 선택(생략하면 그 필드 유지). status는 하위가 active/waiting/onHold만 허용.
    updateMeta(
      deviceId: string,
      updates: { loginId: string; platform?: string; status?: string }[],
    ): Promise<{ ok: boolean; commandId: string }> {
      return request("POST", "/admin/accounts/update-meta", {
        deviceId,
        updates,
      });
    },
    // 계정 삭제(14-계정상태-관리 휴지통) — Admin(서버 staged)과 그 하위 PC 양쪽에서 지운다. 하위엔
    // delete_accounts 명령이 내려가고, 서버 staged에서도 같은 loginId가 제거돼 다시 분배·표시되지 않는다.
    delete(
      deviceId: string,
      loginIds: string[],
    ): Promise<{ ok: boolean; commandId: string }> {
      return request("POST", "/admin/accounts/delete", {
        deviceId,
        loginIds,
      });
    },
  },
  audit: {
    list(): Promise<AuditDto[]> {
      return request("GET", "/admin/audit-log");
    },
  },
  postReports: {
    // 게시 결과 보고(§10-4-2) — 모든 하위의 게시 완료 로그(최신순).
    list(): Promise<PostReportDto[]> {
      return request("GET", "/admin/post-reports");
    },
  },
  loginReports: {
    // 로그인 결과 보고(§10-4-1) — 컴퓨터당 최신 1건(최신순).
    list(): Promise<LoginReportDto[]> {
      return request("GET", "/admin/login-reports");
    },
  },
  stopReports: {
    // 중지 요약(설계서 08 §10-3) — 모든 하위의 kill 요약(디바이스별 누적, 최신순).
    list(): Promise<StopReportDto[]> {
      return request("GET", "/admin/stop-reports");
    },
  },
  dailyResults: {
    // 날짜별 결과(결과보고 날짜 분류) — 하위별로 그 날(KST) 로그인 4분류 + 중지.
    list(): Promise<DeviceDailyDto[]> {
      return request("GET", "/admin/daily-results");
    },
  },
};
