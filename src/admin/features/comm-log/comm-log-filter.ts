// 통신 로그 기기 2단 필터 — 순수 로직(테스트 대상). #444
//
// 화면(comm-log.tsx)의 목록형 3단:
//   목록1(컴퓨터): Admin(전체) · 시스템(있으면) · 하위com1..N (각 = 로그의 device_id)
//   목록2(등록 이력): 목록1 = 하위comN 일 때 → 그 기기가 등록/재등록한 이름들(등록일 오름차순, 정보 표시)
//   목록3(날짜): 그 컴퓨터의 로그 날짜(KST, 최신순)
//
// 서버 감사로그의 device = device_id(UUID). #441 기기 안정 식별로 같은 PC는 같은 device_id를
// 유지하므로, 등록 이력을 device_id로 묶으면 "그 컴퓨터가 거쳐온 이름들"이 된다.
//
// 날짜/시각은 모두 KST(UTC+9)로 변환한다(서버 ts는 UTC ISO). 더미(공백 형식)는 원문 유지.

/** 목록1의 "Admin(전체)" 옵션 값. 실제 device_id와 충돌하지 않도록 센티넬. */
export const ADMIN_SCOPE = "__admin__";

/** device가 빈 값("")인 시스템 로그의 표시 이름(comm-log.tsx와 동일). */
export const SYSTEM_DEVICE = "시스템";

/** 필터가 필요로 하는 로그 한 줄의 최소 형태. 실제 LogLine은 이 슈퍼셋이라 그대로 넘길 수 있다. */
export interface LogRow {
  ts: string;
  device: string;
}

/** 등록 이력 1건(device_id로 그 컴퓨터에 귀속). */
export interface DeviceReg {
  deviceId: string;
  name: string;
  registeredAt: string; // UTC ISO
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

/** 등록시각 → "M/D HH:mm"(KST). 목록2 라벨용. */
export function regTimeLabel(iso: string): string {
  if (isIso(iso)) {
    const k = toKst(iso);
    if (k) {
      return `${k.getUTCMonth() + 1}/${k.getUTCDate()} ${pad2(k.getUTCHours())}:${pad2(k.getUTCMinutes())}`;
    }
  }
  return iso.slice(5, 16);
}

/** 목록2 항목 라벨 — "이름 · M/D HH:mm". */
export function regOptionLabel(reg: DeviceReg): string {
  return `${reg.name} · ${regTimeLabel(reg.registeredAt)}`;
}

/** 목록1(컴퓨터)용 — 시스템을 제외한 실제 하위(device_id) 목록(등장 순서 유지). */
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

/** 그 기기(device_id)의 등록 이력 — 등록시각 오름차순(오래된 위 → 최근 아래). */
export function registrationsForDevice(
  regs: readonly DeviceReg[],
  deviceId: string,
): DeviceReg[] {
  return regs
    .filter((r) => r.deviceId === deviceId)
    .sort((a, b) =>
      a.registeredAt < b.registeredAt
        ? -1
        : a.registeredAt > b.registeredAt
          ? 1
          : 0,
    );
}

/**
 * 하위com 정렬 순서(device_id 배열): 등록이 이른 기기가 앞(하위com1),
 * 등록 이력이 없는 기기는 로그 등장 순으로 뒤에. 하위com 번호의 기준.
 */
export function comOrder(
  logDevices: readonly string[],
  regs: readonly DeviceReg[],
): string[] {
  const earliest = new Map<string, string>();
  for (const r of regs) {
    const cur = earliest.get(r.deviceId);
    if (cur === undefined || r.registeredAt < cur) {
      earliest.set(r.deviceId, r.registeredAt);
    }
  }
  const withReg = logDevices.filter((d) => earliest.has(d));
  const without = logDevices.filter((d) => !earliest.has(d));
  withReg.sort((a, b) => {
    const ea = earliest.get(a) ?? "";
    const eb = earliest.get(b) ?? "";
    return ea < eb ? -1 : ea > eb ? 1 : 0;
  });
  return [...withReg, ...without];
}

/** 하위com 라벨: 순서 배열에서의 위치+1(예: 하위com1). 배열에 없으면 원문. */
export function comLabel(deviceId: string, order: readonly string[]): string {
  const i = order.indexOf(deviceId);
  return i >= 0 ? `하위com${i + 1}` : deviceId;
}

/**
 * 로그를 컴퓨터·날짜로 거른다.
 * - computer = ADMIN_SCOPE → 아무것도(초기 빈 화면. 대량 로그 즉시 렌더 방지)
 * - computer = 시스템        → 시스템 로그(device 미지정)
 * - computer = device_id     → 그 컴퓨터 로그
 * - date 있으면 그 날짜(KST)만.
 */
export function filterLines<T extends LogRow>(
  lines: readonly T[],
  computer: string,
  date: string | null,
): T[] {
  if (computer === ADMIN_SCOPE) return [];
  const dev = lines.filter((l) => l.device === computer);
  if (!date) return dev;
  return dev.filter((l) => logDateKey(l.ts) === date);
}
