import { describe, it, expect, vi, afterEach } from "vitest";

import { nowParts, scheduleMoment, toEpochMs } from "./schedule";

describe("schedule util", () => {
  afterEach(() => vi.useRealTimers());

  it("toEpochMs round-trips a local date/time", () => {
    const ms = toEpochMs("2026-06-08", "14:05");
    const d = new Date(ms);
    expect(d.getFullYear()).toBe(2026);
    expect(d.getMonth()).toBe(5); // 0-based June
    expect(d.getDate()).toBe(8);
    expect(d.getHours()).toBe(14);
    expect(d.getMinutes()).toBe(5);
  });

  it("scheduleMoment labels today/tomorrow/dayafter and dates", () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date(2026, 5, 8, 9, 0)); // 2026-06-08
    expect(scheduleMoment("2026-06-08", "18:30")).toEqual({
      label: "오늘",
      when: "오늘 18:30",
    });
    expect(scheduleMoment("2026-06-09", "09:00").label).toBe("내일");
    expect(scheduleMoment("2026-06-10", "09:00").label).toBe("모레");
    // 3일 이상 뒤는 월/일 라벨.
    expect(scheduleMoment("2026-06-20", "09:00").label).toBe("6/20");
  });

  it("nowParts returns minute-precision strings for the current time", () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date(2026, 0, 3, 7, 9)); // 2026-01-03 07:09
    expect(nowParts()).toEqual({ date: "2026-01-03", time: "07:09" });
  });
});
