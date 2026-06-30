import type { PlatformId } from "@/shared/data/types";

import type { LoginReportDto, PostReportDto } from "../../api";

// 게시 결과(§10-4-2) 데이터 모델 + 순수 변환. 데스크톱 앱 알림(notifications.tsx)의
// BatchItem/PostedContent 모델 그대로다. 컴포넌트 파일과 분리해 fast-refresh를 깨지 않고,
// 매핑 규칙을 단위테스트(result-report.test.tsx)로 고정한다.

export interface Posted {
  title: string;
  body: string;
  comment?: string;
  url?: string;
}

export interface PostItem {
  platform: PlatformId;
  target: string; // 어디에 게시했는지(종목토론방·카페 이름 등)
  loginId: string;
  status: "success" | "fail";
  msg: string; // 메인 사유(친절한 한국어)
  trace?: string; // 실패 시 "자세히 보기"용 백트레이스(util.rs transport_error_message! 형식)
  posted?: Posted; // 성공 시 실제 게시 내용 + 링크
}

export interface PostBatch {
  device: string;
  title: string;
  at: string;
  items: PostItem[];
}

// ───────────────────────── 로그인 결과(§10-4-1) ─────────────────────────

export interface Line {
  loginId: string;
  pw: string;
  reason?: string;
}

export interface DeviceReport {
  device: string;
  batch: {
    success: number;
    onhold: Line[];
    timedout: Line[];
    failed: Line[];
  };
  cumulative: {
    received: number;
    success: number;
    onhold: number;
    timedout: number;
    failed: number;
  };
}

// 서버 로그인 결과 보고(LoginReportDto) → 화면 DeviceReport. 모양이 사실상 동일해 줄만 옮긴다
// (마스킹은 표시 시점에). reason 미설정(대기초과)은 키를 넣지 않는다(exactOptionalPropertyTypes).
function toLine(l: { loginId: string; pw: string; reason?: string }): Line {
  return {
    loginId: l.loginId,
    pw: l.pw,
    ...(l.reason !== undefined ? { reason: l.reason } : {}),
  };
}

export function toDeviceReport(r: LoginReportDto): DeviceReport {
  return {
    device: r.device,
    batch: {
      success: r.batch.success,
      onhold: r.batch.onhold.map(toLine),
      timedout: r.batch.timedout.map(toLine),
      failed: r.batch.failed.map(toLine),
    },
    cumulative: { ...r.cumulative },
  };
}

// epoch ms → "YYYY-MM-DD HH:MM" (게시 완료 시각 표시용).
export function fmtAt(ms: number): string {
  if (!ms) return "";
  const d = new Date(ms);
  const p = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())} ${p(d.getHours())}:${p(d.getMinutes())}`;
}

// 서버 게시 결과 보고(PostReportDto, 하위 LogBatch 사본) → 화면 PostBatch.
// 데스크톱 모델 status는 success/fail/skip/… 인데 화면 배지는 성공/실패 둘이므로
// success가 아니면 실패로 묶는다(사유 msg·백트레이스 trace는 그대로 보존).
export function toPostBatch(r: PostReportDto): PostBatch {
  return {
    device: r.device,
    title: r.title,
    at: fmtAt(r.at),
    items: r.items.map((it) => ({
      platform: it.platform as PlatformId,
      target: it.target,
      loginId: it.loginId,
      status: it.status === "success" ? "success" : "fail",
      msg: it.msg,
      ...(it.trace !== undefined ? { trace: it.trace } : {}),
      ...(it.posted !== undefined
        ? {
            posted: {
              title: it.posted.title,
              body: it.posted.body,
              ...(it.posted.comment !== undefined
                ? { comment: it.posted.comment }
                : {}),
              ...(it.posted.url !== undefined ? { url: it.posted.url } : {}),
            },
          }
        : {}),
    })),
  };
}
