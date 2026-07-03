// Admin 웹 → 중앙 서버(`server/`) HTTP 클라이언트. 설계 §8 transport(HttpSse)의 Admin측 구현.
//
// 핵심: 서버가 떠 있지 않은 **오프라인 미리보기**에서도 화면이 깨지지 않게, fetch 실패(연결 불가)는
// `OfflineError`로 던진다. 각 화면은 이를 잡아 기존 더미 데이터로 폴백한다(UI 무손상).
// 서버 주소는 `VITE_ADMIN_API`로 주입(기본 http://localhost:8080). **배포 전 결정**(주소/포트/TLS).

const BASE =
  (import.meta.env.VITE_ADMIN_API as string | undefined)?.replace(/\/+$/, "") ??
  "http://localhost:8080";

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
export interface DeviceCodeResp {
  code: string;
  serverUrl: string | null;
  expiresInSecs: number;
}
export interface AccountDto {
  id: string;
  loginId: string;
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
    remove(id: string): Promise<unknown> {
      return request("DELETE", `/devices/${encodeURIComponent(id)}`);
    },
    command(id: string, type: string, commandId?: string): Promise<unknown> {
      return request("POST", `/devices/${encodeURIComponent(id)}/commands`, {
        type,
        commandId,
      });
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
    }): Promise<ForumStockPageDto> {
      const qs = new URLSearchParams({ category: req.category });
      if (req.exchange != null) qs.set("exchange", req.exchange);
      if (req.market != null) qs.set("market", req.market);
      if (req.page != null) qs.set("page", String(req.page));
      return request("GET", `/admin/forum-stocks?${qs.toString()}`);
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
      assignments: { loginId: string; stocks: { code: string; name: string }[] }[];
    }): Promise<{ ok: boolean; commandId: string }> {
      return request("POST", "/admin/publish", req);
    },
  },
  accounts: {
    list(): Promise<AccountDto[]> {
      return request("GET", "/admin/accounts");
    },
    import(accounts: { loginId: string; pw: string }[]): Promise<ImportResult> {
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
};
