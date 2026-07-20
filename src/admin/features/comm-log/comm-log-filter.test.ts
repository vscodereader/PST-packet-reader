import { describe, expect, it } from "vitest";

import {
  ADMIN_SCOPE,
  SYSTEM_DEVICE,
  comLabel,
  comOrder,
  dateLabel,
  datesForDevice,
  deviceOptions,
  filterLines,
  formatTs,
  logDateKey,
  regOptionLabel,
  regTimeLabel,
  registrationsForDevice,
  type DeviceReg,
  type LogRow,
} from "./comm-log-filter";

// 실데이터 형태(UTC ISO). KST(+9)로 변환됐을 때의 날짜/시각을 기대한다.
const rows: LogRow[] = [
  { ts: "2026-07-01T02:00:00.000+00:00", device: "dev-A" }, // KST 07-01 11:00
  { ts: "2026-07-01T20:00:00.000+00:00", device: "dev-A" }, // KST 07-02 05:00
  { ts: "2026-07-03T01:00:00.000+00:00", device: "dev-B" }, // KST 07-03 10:00
  { ts: "2026-07-04T05:00:00.000+00:00", device: "dev-B" }, // KST 07-04 14:00
  { ts: "2026-07-01T00:30:00.000+00:00", device: SYSTEM_DEVICE }, // KST 07-01 09:30
];

const regs: DeviceReg[] = [
  { deviceId: "dev-B", name: "PC-B", registeredAt: "2026-07-02T00:00:00Z" },
  {
    deviceId: "dev-A",
    name: "PC-옛이름",
    registeredAt: "2026-06-30T00:00:00Z",
  },
  {
    deviceId: "dev-A",
    name: "PC-새이름",
    registeredAt: "2026-07-01T00:00:00Z",
  },
];

describe("logDateKey", () => {
  it("UTC ISO를 KST 날짜로 변환한다", () => {
    expect(logDateKey("2026-07-01T02:00:00.000+00:00")).toBe("2026-07-01");
  });
  it("자정 근처 UTC 로그가 다음날 KST로 넘어간다", () => {
    expect(logDateKey("2026-07-01T20:00:00.000+00:00")).toBe("2026-07-02");
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
  it("더미는 원문 그대로", () => {
    expect(formatTs("2026-06-28 10:20:01.102")).toBe("2026-06-28 10:20:01.102");
  });
});

describe("dateLabel", () => {
  it("YYYY-MM-DD를 M/D로", () => {
    expect(dateLabel("2026-07-01")).toBe("7/1");
    expect(dateLabel("2026-12-25")).toBe("12/25");
  });
});

describe("regTimeLabel / regOptionLabel", () => {
  it("등록시각을 KST M/D HH:mm으로", () => {
    // 2026-07-01T20:00Z + 9h = 07-02 05:00 KST
    expect(regTimeLabel("2026-07-01T20:00:00Z")).toBe("7/2 05:00");
  });
  it("목록2 라벨은 '이름 · M/D HH:mm'", () => {
    expect(
      regOptionLabel({
        deviceId: "d",
        name: "PC-사무실",
        registeredAt: "2026-07-01T00:00:00Z",
      }),
    ).toBe("PC-사무실 · 7/1 09:00");
  });
});

describe("deviceOptions", () => {
  it("시스템을 빼고 등장 순서로 device_id만", () => {
    expect(deviceOptions(rows)).toEqual(["dev-A", "dev-B"]);
  });
});

describe("datesForDevice", () => {
  it("그 컴퓨터가 기록을 가진 날짜만 최신순", () => {
    expect(datesForDevice(rows, "dev-A")).toEqual(["2026-07-02", "2026-07-01"]);
    expect(datesForDevice(rows, "dev-B")).toEqual(["2026-07-04", "2026-07-03"]);
  });
});

describe("registrationsForDevice", () => {
  it("그 기기의 등록 이력을 등록일 오름차순(오래된 위)으로", () => {
    const out = registrationsForDevice(regs, "dev-A");
    expect(out.map((r) => r.name)).toEqual(["PC-옛이름", "PC-새이름"]);
  });
  it("이력 없는 기기는 빈 배열", () => {
    expect(registrationsForDevice(regs, "dev-Z")).toEqual([]);
  });
});

describe("comOrder / comLabel", () => {
  it("등록 이른 기기가 앞(하위com1). dev-A(6/30) < dev-B(7/2)", () => {
    const order = comOrder(["dev-B", "dev-A"], regs);
    expect(order).toEqual(["dev-A", "dev-B"]);
    expect(comLabel("dev-A", order)).toBe("하위com1");
    expect(comLabel("dev-B", order)).toBe("하위com2");
  });
  it("이력 없는 기기는 뒤에 로그 등장 순으로", () => {
    const order = comOrder(["dev-C", "dev-A"], regs);
    expect(order).toEqual(["dev-A", "dev-C"]); // dev-A 이력있음 앞, dev-C 뒤
  });
});

describe("filterLines", () => {
  it("Admin = 아무것도(초기 빈 화면)", () => {
    expect(filterLines(rows, ADMIN_SCOPE, null)).toHaveLength(0);
  });
  it("시스템 = 시스템 로그만", () => {
    const out = filterLines(rows, SYSTEM_DEVICE, null);
    expect(out).toHaveLength(1);
    expect(out[0]?.device).toBe(SYSTEM_DEVICE);
  });
  it("device_id = 그 컴퓨터 전체", () => {
    expect(filterLines(rows, "dev-A", null)).toHaveLength(2);
  });
  it("device_id + 날짜 = 그 날짜만", () => {
    const out = filterLines(rows, "dev-A", "2026-07-01");
    expect(out).toHaveLength(1);
    expect(out[0]?.ts).toBe("2026-07-01T02:00:00.000+00:00");
  });
  it("기록 없는 날짜는 빈 결과", () => {
    expect(filterLines(rows, "dev-A", "2026-07-03")).toHaveLength(0);
  });
});
