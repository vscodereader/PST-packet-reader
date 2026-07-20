import { describe, expect, it } from "vitest";

import {
  ADMIN_SCOPE,
  SYSTEM_DEVICE,
  computerOptions,
  dateLabel,
  datesForDevice,
  deviceOptions,
  filterLines,
  formatTs,
  isDate3Enabled,
  logDateKey,
  type LogRow,
} from "./comm-log-filter";

// 실데이터 형태(UTC ISO). KST(+9)로 변환됐을 때의 날짜/시각을 기대한다.
const rows: LogRow[] = [
  { ts: "2026-07-01T02:00:00.000+00:00", device: "하위-001" }, // KST 07-01 11:00
  { ts: "2026-07-01T20:00:00.000+00:00", device: "하위-001" }, // KST 07-02 05:00
  { ts: "2026-07-03T01:00:00.000+00:00", device: "하위-002" }, // KST 07-03 10:00
  { ts: "2026-07-04T05:00:00.000+00:00", device: "하위-002" }, // KST 07-04 14:00
  { ts: "2026-07-01T00:30:00.000+00:00", device: SYSTEM_DEVICE }, // KST 07-01 09:30
];

describe("logDateKey", () => {
  it("UTC ISO를 KST 날짜로 변환한다", () => {
    expect(logDateKey("2026-07-01T02:00:00.000+00:00")).toBe("2026-07-01");
  });

  it("자정 근처 UTC 로그가 다음날 KST로 넘어간다", () => {
    // 20:00 UTC + 9h = 다음날 05:00 KST
    expect(logDateKey("2026-07-01T20:00:00.000+00:00")).toBe("2026-07-02");
  });

  it("Z 표기도 KST로 변환한다", () => {
    // 23:30 UTC + 9h = 다음날 08:30 KST
    expect(logDateKey("2026-07-01T23:30:00Z")).toBe("2026-07-02");
  });

  it("더미(공백 형식)는 앞 10자리를 그대로 쓴다", () => {
    expect(logDateKey("2026-06-28 10:20:01.102")).toBe("2026-06-28");
  });
});

describe("formatTs", () => {
  it("UTC ISO를 KST 벽시계 문자열로 표시한다", () => {
    expect(formatTs("2026-07-01T20:00:00.000+00:00")).toBe(
      "2026-07-02 05:00:00.000",
    );
  });

  it("더미(공백 형식)는 원문 그대로 둔다", () => {
    expect(formatTs("2026-06-28 10:20:01.102")).toBe("2026-06-28 10:20:01.102");
  });
});

describe("dateLabel", () => {
  it("YYYY-MM-DD를 M/D로 만든다", () => {
    expect(dateLabel("2026-07-01")).toBe("7/1");
    expect(dateLabel("2026-12-25")).toBe("12/25");
  });

  it("형식이 안 맞으면 원문을 반환한다", () => {
    expect(dateLabel("weird")).toBe("weird");
  });
});

describe("deviceOptions / computerOptions", () => {
  it("deviceOptions는 시스템을 빼고 등장 순서로 하위만 준다", () => {
    expect(deviceOptions(rows)).toEqual(["하위-001", "하위-002"]);
  });

  it("computerOptions는 시스템을 맨 앞에 두고 하위를 잇는다", () => {
    expect(computerOptions(rows)).toEqual([
      SYSTEM_DEVICE,
      "하위-001",
      "하위-002",
    ]);
  });

  it("시스템 로그가 없으면 computerOptions에 시스템이 없다", () => {
    const noSys: LogRow[] = [
      { ts: "2026-07-01T00:00:00Z", device: "하위-001" },
    ];
    expect(computerOptions(noSys)).toEqual(["하위-001"]);
  });
});

describe("datesForDevice", () => {
  it("그 컴퓨터가 기록을 가진 날짜만 최신순으로 준다", () => {
    expect(datesForDevice(rows, "하위-001")).toEqual([
      "2026-07-02",
      "2026-07-01",
    ]);
    expect(datesForDevice(rows, "하위-002")).toEqual([
      "2026-07-04",
      "2026-07-03",
    ]);
  });

  it("기록 없는 컴퓨터는 빈 배열", () => {
    expect(datesForDevice(rows, "하위-999")).toEqual([]);
  });
});

describe("isDate3Enabled", () => {
  it("Admin이고 목록2가 하위COM일 때만 활성", () => {
    expect(isDate3Enabled(ADMIN_SCOPE, "하위-001")).toBe(true);
  });
  it("목록2가 시스템이면 비활성", () => {
    expect(isDate3Enabled(ADMIN_SCOPE, SYSTEM_DEVICE)).toBe(false);
  });
  it("목록2가 비었으면 비활성", () => {
    expect(isDate3Enabled(ADMIN_SCOPE, null)).toBe(false);
  });
  it("목록1이 하위COM이면(=Admin 아님) 비활성", () => {
    expect(isDate3Enabled("하위-001", "2026-07-01")).toBe(false);
  });
});

describe("filterLines", () => {
  it("Admin + 선택없음 = 전체", () => {
    expect(filterLines(rows, ADMIN_SCOPE, null, null)).toHaveLength(
      rows.length,
    );
  });

  it("Admin + 시스템 = 시스템 로그만(날짜 무관)", () => {
    const out = filterLines(rows, ADMIN_SCOPE, SYSTEM_DEVICE, null);
    expect(out).toHaveLength(1);
    expect(out[0]?.device).toBe(SYSTEM_DEVICE);
  });

  it("Admin + 하위COM(날짜 없음) = 그 COM 전체", () => {
    expect(filterLines(rows, ADMIN_SCOPE, "하위-001", null)).toHaveLength(2);
  });

  it("Admin + 하위COM + 날짜 = 그 COM의 그 날짜만", () => {
    const out = filterLines(rows, ADMIN_SCOPE, "하위-001", "2026-07-01");
    expect(out).toHaveLength(1);
    expect(out[0]?.ts).toBe("2026-07-01T02:00:00.000+00:00");
  });

  it("하위COM 직접선택(날짜 없음) = 그 COM 전체", () => {
    expect(filterLines(rows, "하위-002", null, null)).toHaveLength(2);
  });

  it("하위COM 직접선택 + 날짜 = 그 COM의 그 날짜만", () => {
    const out = filterLines(rows, "하위-002", "2026-07-04", null);
    expect(out).toHaveLength(1);
    expect(out[0]?.device).toBe("하위-002");
  });

  it("기록 없는 날짜를 고르면 빈 결과", () => {
    expect(filterLines(rows, "하위-001", "2026-07-03", null)).toHaveLength(0);
  });
});
