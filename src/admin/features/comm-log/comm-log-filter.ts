// 통신 로그 일자별 필터 — 순수 로직(테스트 대상).
//
// 화면(comm-log.tsx)은 이 헬퍼들만 조합해 3단 종속 드롭다운을 그린다.
//   목록1(컴퓨터): Admin(전체) · 하위COM들
//   목록2:
//     - 목록1 = 특정 하위COM → 그 COM이 기록을 가진 "날짜" 목록(없는 날짜는 안 나옴)
//     - 목록1 = Admin        → "시스템 · 하위COM들"(컴퓨터 목록)
//   목록3(평소 비활성): 목록1=Admin & 목록2=하위COM 일 때만 활성 → 그 COM의 "날짜" 목록
//
// 서버 감사로그의 ts는 UTC ISO(Utc::now().to_rfc3339())로 내려온다. 날짜 추출·표시는
// 모두 KST(UTC+9)로 변환한다 — 서버의 결과보고 일자집계(state.rs, KST 버킷)와 같은 기준이라
// 자정 근처 로그가 엉뚱한 날짜로 새지 않는다. 오프라인 미리보기용 더미(ts에 'T' 없음,
// 이미 표시형 문자열)는 변환하지 않고 그대로 쓴다.

/** 목록1의 "Admin(전체)" 옵션 값. 실제 하위 이름과 충돌하지 않도록 센티넬을 쓴다. */
export const ADMIN_SCOPE = "__admin__";

/** 서버 감사로그에서 device가 빈 값("")인 시스템 로그의 표시 이름(comm-log.tsx와 동일). */
export const SYSTEM_DEVICE = "시스템";

/** 필터가 필요로 하는 로그 한 줄의 최소 형태. 실제 LogLine은 이 슈퍼셋이라 그대로 넘길 수 있다. */
export interface LogRow {
  ts: string;
  device: string;
}

const KST_OFFSET_MS = 9 * 60 * 60 * 1000;

const pad2 = (n: number): string => String(n).padStart(2, "0");
const pad3 = (n: number): string => String(n).padStart(3, "0");

// ISO 형식(서버 실데이터)인지 판별 — 더미는 "YYYY-MM-DD HH:mm:ss.SSS"(공백, 'T' 없음).
const isIso = (ts: string): boolean => ts.includes("T");

/** ts를 KST 벽시계로 옮긴 Date(UTC 게터로 읽으면 KST 값). ISO가 아니면 null. */
function toKst(ts: string): Date | null {
  const ms = Date.parse(ts);
  if (Number.isNaN(ms)) return null;
  return new Date(ms + KST_OFFSET_MS);
}

/** KST 기준 날짜 키 "YYYY-MM-DD". 그룹핑·필터의 단위. */
export function logDateKey(ts: string): string {
  if (isIso(ts)) {
    const k = toKst(ts);
    if (k) {
      return `${k.getUTCFullYear()}-${pad2(k.getUTCMonth() + 1)}-${pad2(k.getUTCDate())}`;
    }
  }
  return ts.slice(0, 10);
}

/** 각 줄에 표시할 KST 시각 "YYYY-MM-DD HH:mm:ss.SSS". 더미는 원문 그대로. */
export function formatTs(ts: string): string {
  if (isIso(ts)) {
    const k = toKst(ts);
    if (k) {
      return (
        `${k.getUTCFullYear()}-${pad2(k.getUTCMonth() + 1)}-${pad2(k.getUTCDate())} ` +
        `${pad2(k.getUTCHours())}:${pad2(k.getUTCMinutes())}:${pad2(k.getUTCSeconds())}.${pad3(k.getUTCMilliseconds())}`
      );
    }
  }
  return ts;
}

/** 날짜 키 "YYYY-MM-DD" → 드롭다운 라벨 "M/D"(예: 2026-07-01 → "7/1"). */
export function dateLabel(key: string): string {
  const m = /^(\d{4})-(\d{2})-(\d{2})$/.exec(key);
  if (!m) return key;
  return `${Number(m[2])}/${Number(m[3])}`;
}

/** 목록1(컴퓨터)용 — 시스템을 제외한 실제 하위 목록(등장 순서 유지). */
export function deviceOptions(lines: readonly LogRow[]): string[] {
  const seen = new Set<string>();
  const out: string[] = [];
  for (const l of lines) {
    if (l.device === SYSTEM_DEVICE) continue;
    if (seen.has(l.device)) continue;
    seen.add(l.device);
    out.push(l.device);
  }
  return out;
}

/** 목록2(Admin 선택 시)용 — [시스템(있으면 맨 앞), ...하위들]. */
export function computerOptions(lines: readonly LogRow[]): string[] {
  const devices = deviceOptions(lines);
  const hasSystem = lines.some((l) => l.device === SYSTEM_DEVICE);
  return hasSystem ? [SYSTEM_DEVICE, ...devices] : devices;
}

/** 특정 컴퓨터가 기록을 가진 날짜 키 목록(최신순). 기록 없는 날짜는 포함되지 않는다. */
export function datesForDevice(
  lines: readonly LogRow[],
  device: string,
): string[] {
  const keys = new Set<string>();
  for (const l of lines) {
    if (l.device === device) keys.add(logDateKey(l.ts));
  }
  return Array.from(keys).sort((a, b) => (a < b ? 1 : a > b ? -1 : 0));
}

/** 목록3(마지막 날짜 드롭다운)이 활성화되는 조건: 목록1=Admin & 목록2=하위COM. */
export function isDate3Enabled(sel1: string, sel2: string | null): boolean {
  return sel1 === ADMIN_SCOPE && !!sel2 && sel2 !== SYSTEM_DEVICE;
}

/**
 * 3단 선택값으로 로그를 거른다.
 * - sel1 = ADMIN_SCOPE:
 *     sel2 없음        → 전체
 *     sel2 = 시스템     → 시스템 로그만(날짜 필터 없음)
 *     sel2 = 하위COM    → 그 COM 로그, sel3(날짜) 있으면 그 날짜만
 * - sel1 = 하위COM:
 *     sel2(날짜) 없음   → 그 COM 전체
 *     sel2 = 날짜       → 그 COM의 그 날짜만
 */
export function filterLines<T extends LogRow>(
  lines: readonly T[],
  sel1: string,
  sel2: string | null,
  sel3: string | null,
): T[] {
  if (sel1 === ADMIN_SCOPE) {
    if (!sel2) return [...lines];
    if (sel2 === SYSTEM_DEVICE) {
      return lines.filter((l) => l.device === SYSTEM_DEVICE);
    }
    const dev = lines.filter((l) => l.device === sel2);
    if (!sel3) return dev;
    return dev.filter((l) => logDateKey(l.ts) === sel3);
  }
  const dev = lines.filter((l) => l.device === sel1);
  if (!sel2) return dev;
  return dev.filter((l) => logDateKey(l.ts) === sel2);
}
