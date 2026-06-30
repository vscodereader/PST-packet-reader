import { describe, expect, it } from "vitest";

import type { LoginReportDto, PostReportDto } from "../../api";

import { fmtAt, toDeviceReport, toPostBatch } from "./mapping";

// 순수 매핑 함수만 검증한다(렌더/네트워크 비의존). 서버 PostReportDto(하위 LogBatch 사본) →
// 화면 PostBatch 변환 규칙(§10-4-2)을 고정한다.
describe("result-report 매핑", () => {
  describe("fmtAt", () => {
    it("0(또는 미설정)은 빈 문자열", () => {
      expect(fmtAt(0)).toBe("");
    });
    it("epoch ms를 YYYY-MM-DD HH:MM 형태로(로컬) 포맷", () => {
      const out = fmtAt(1_719_700_000_000);
      expect(out).toMatch(/^\d{4}-\d{2}-\d{2} \d{2}:\d{2}$/);
    });
  });

  describe("toPostBatch", () => {
    const base: PostReportDto = {
      device: "하위-001",
      deviceId: "d1",
      batchId: "lb-q-1",
      title: "3개 종목토론방 게시",
      at: 1_719_700_000_000,
      receivedAt: "2026-06-30T06:20:40Z",
      items: [],
    };

    it("device/title을 그대로, at은 포맷 문자열로 옮긴다", () => {
      const b = toPostBatch(base);
      expect(b.device).toBe("하위-001");
      expect(b.title).toBe("3개 종목토론방 게시");
      expect(b.at).toMatch(/^\d{4}-\d{2}-\d{2} \d{2}:\d{2}$/);
    });

    it("성공 item은 success + posted(제목/본문/댓글/URL) 보존", () => {
      const b = toPostBatch({
        ...base,
        items: [
          {
            platform: "forum",
            target: "삼성전자 종목토론방",
            loginId: "chol_invest",
            status: "success",
            msg: "게시 완료",
            posted: {
              title: "T",
              body: "B",
              comment: "C",
              url: "https://x",
            },
          },
        ],
      });
      expect(b.items[0]?.status).toBe("success");
      expect(b.items[0]?.posted).toEqual({
        title: "T",
        body: "B",
        comment: "C",
        url: "https://x",
      });
      expect(b.items[0]?.trace).toBeUndefined();
    });

    it("success가 아닌 status(skip 포함)는 fail로 묶고 trace를 보존", () => {
      const b = toPostBatch({
        ...base,
        items: [
          {
            platform: "forum",
            target: "기아 종목토론방",
            loginId: "blue_chip",
            status: "skip",
            msg: "건너뜀",
            trace: "at post.rs:212",
          },
        ],
      });
      expect(b.items[0]?.status).toBe("fail");
      expect(b.items[0]?.trace).toBe("at post.rs:212");
      expect(b.items[0]?.posted).toBeUndefined();
    });

    it("posted.comment/url 미설정 시 키 자체를 넣지 않는다(exactOptionalPropertyTypes)", () => {
      const b = toPostBatch({
        ...base,
        items: [
          {
            platform: "forum",
            target: "NAVER 종목토론방",
            loginId: "park_long",
            status: "success",
            msg: "게시 완료",
            posted: { title: "T", body: "B" },
          },
        ],
      });
      const posted = b.items[0]?.posted;
      expect(posted).toBeDefined();
      expect("comment" in (posted ?? {})).toBe(false);
      expect("url" in (posted ?? {})).toBe(false);
    });
  });

  describe("toDeviceReport(로그인 결과)", () => {
    const dto: LoginReportDto = {
      device: "하위-001",
      deviceId: "d1",
      receivedAt: "2026-06-30T06:20:40Z",
      batch: {
        success: 3,
        onhold: [{ loginId: "a", pw: "p1", reason: "캡차" }],
        timedout: [{ loginId: "b", pw: "p2" }],
        failed: [
          { loginId: "c", pw: "p3", reason: "연결 실패", trace: "at x.rs:1:1" },
        ],
      },
      cumulative: {
        received: 20,
        success: 6,
        onhold: 3,
        timedout: 5,
        failed: 6,
      },
      registered: 4,
      registeredVisible: 4,
    };

    it("batch 4분류와 누적을 그대로 옮긴다", () => {
      const r = toDeviceReport(dto);
      expect(r.device).toBe("하위-001");
      expect(r.batch.success).toBe(3);
      expect(r.batch.onhold[0]?.reason).toBe("캡차");
      expect(r.batch.failed[0]?.reason).toBe("연결 실패");
      expect(r.cumulative).toEqual({
        received: 20,
        success: 6,
        onhold: 3,
        timedout: 5,
        failed: 6,
      });
    });

    it("실패 줄의 trace(자세히 보기용)와 등록 정보를 옮긴다", () => {
      const r = toDeviceReport(dto);
      expect(r.batch.failed[0]?.trace).toBe("at x.rs:1:1");
      expect(r.registered).toBe(4);
      expect(r.registeredVisible).toBe(4);
    });

    it("대기초과 줄은 reason/trace 키를 넣지 않는다(exactOptionalPropertyTypes)", () => {
      const r = toDeviceReport(dto);
      const line = r.batch.timedout[0];
      expect(line?.loginId).toBe("b");
      expect("reason" in (line ?? {})).toBe(false);
      expect("trace" in (line ?? {})).toBe(false);
    });
  });
});
