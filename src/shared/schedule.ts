// 예약(스케줄) 시각 변환·표시 헬퍼. 게시 모달(예약 게시)과 게시 큐(재예약)가 공용으로
// 쓴다 — 동일한 epoch 계산·한국어 표시 문자열을 한 곳에서 관리한다(단일 출처).

const pad2 = (n: number) => String(n).padStart(2, "0");

/** Current date/time as the picker's `{ date, time }` strings (minute precision). */
export function nowParts(): { date: string; time: string } {
  const n = new Date();
  return {
    date: `${n.getFullYear()}-${pad2(n.getMonth() + 1)}-${pad2(n.getDate())}`,
    time: `${pad2(n.getHours())}:${pad2(n.getMinutes())}`,
  };
}

/** Local epoch-ms for a `YYYY-MM-DD` + `HH:MM` pair (for the IPC time guard). */
export function toEpochMs(date: string, time: string): number {
  const [y, m, d] = date.split("-").map(Number);
  const [h, mi] = time.split(":").map(Number);
  return new Date(y ?? 1970, (m ?? 1) - 1, d ?? 1, h ?? 0, mi ?? 0).getTime();
}

/** Turn the picked date/time into the queue's `{ when, rel }` display strings. */
export function scheduleMoment(
  date: string,
  time: string,
): { label: string; when: string } {
  const [y, m, d] = date.split("-").map(Number);
  const target = new Date(y ?? 1970, (m ?? 1) - 1, d ?? 1);
  const today = new Date();
  today.setHours(0, 0, 0, 0);
  const diff = Math.round((target.getTime() - today.getTime()) / 86400000);
  const label =
    diff <= 0
      ? "오늘"
      : diff === 1
        ? "내일"
        : diff === 2
          ? "모레"
          : `${m}/${d}`;
  return { label, when: `${label} ${time}` };
}
