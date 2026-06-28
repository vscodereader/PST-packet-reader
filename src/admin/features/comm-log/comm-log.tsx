import {
  Box,
  Button,
  Group,
  Paper,
  ScrollArea,
  Select,
  Text,
} from "@mantine/core";
import { notifications } from "@mantine/notifications";
import { useMemo, useState } from "react";

import { Icon } from "@/shared/ui/icons";

// 통신 로그 화면 — Admin↔하위 사이의 모든 통신(SSE 명령 / POST 결과 / 하트비트 /
// 등록 / IP변경)을 콘솔처럼 보여준다. 설계 §7 감사로그(명령/결과 이력)의 조회 화면.
// 기존 tracing 컨벤션(태그+마스킹 ID+이모지, 비밀 미노출)을 그대로 따른다.

type Level = "cmd" | "ok" | "fail" | "info" | "warn";

interface LogLine {
  ts: string;
  tag: string; // [CMD] [RESULT] [HEARTBEAT] [SSE] [REGISTER] [STATE]
  dir: string; // "Admin → 하위-001" 등
  device: string; // 필터용 device id ("all" 제외)
  msg: string;
  level: Level;
}

// 더미 통신 로그 — 실제로는 서버 감사로그 DB에서 내려온다. 비밀(PW/쿠키)은 찍지 않는다.
const LOG_LINES: LogLine[] = [
  {
    ts: "2026-06-28 10:20:01.102",
    tag: "[SSE]",
    dir: "하위-001 → 서버",
    device: "하위-001",
    msg: "스트림 연결됨 (device_id=d1)",
    level: "info",
  },
  {
    ts: "2026-06-28 10:20:01.340",
    tag: "[REGISTER]",
    dir: "하위-002 → 서버",
    device: "하위-002",
    msg: "기기코드 8237 등록 성공, 토큰 발급 ✅",
    level: "ok",
  },
  {
    ts: "2026-06-28 10:20:03.001",
    tag: "[HEARTBEAT]",
    dir: "하위-001 → 서버",
    device: "하위-001",
    msg: "online · IP 1.2.3.4",
    level: "info",
  },
  {
    ts: "2026-06-28 10:20:03.220",
    tag: "[HEARTBEAT]",
    dir: "하위-002 → 서버",
    device: "하위-002",
    msg: "online · IP 9.10.11.12",
    level: "info",
  },
  {
    ts: "2026-06-28 10:20:05.220",
    tag: "[CMD]",
    dir: "Admin → 하위-001",
    device: "하위-001",
    msg: "import_then_login_all (commandId=c-1001)",
    level: "cmd",
  },
  {
    ts: "2026-06-28 10:20:05.998",
    tag: "[STATE]",
    dir: "하위-001 → 서버",
    device: "하위-001",
    msg: "ROTATING — IP 변경 시작, 명령 버튼 비활성",
    level: "warn",
  },
  {
    ts: "2026-06-28 10:20:09.430",
    tag: "[SSE]",
    dir: "하위-001 ✗",
    device: "하위-001",
    msg: "스트림 끊김 (비행기모드)",
    level: "warn",
  },
  {
    ts: "2026-06-28 10:20:12.770",
    tag: "[SSE]",
    dir: "하위-001 → 서버",
    device: "하위-001",
    msg: "재연결 성공 · new IP 5.6.7.8 (토큰 동일)",
    level: "ok",
  },
  {
    ts: "2026-06-28 10:20:13.010",
    tag: "[HEARTBEAT]",
    dir: "하위-001 → 서버",
    device: "하위-001",
    msg: "online · IP 5.6.7.8",
    level: "info",
  },
  {
    ts: "2026-06-28 10:20:18.770",
    tag: "[RESULT]",
    dir: "하위-001 → Admin",
    device: "하위-001",
    msg: "c-1001 진행 3/10 (st**** 로그인 성공 ✅)",
    level: "ok",
  },
  {
    ts: "2026-06-28 10:20:31.220",
    tag: "[RESULT]",
    dir: "하위-001 → Admin",
    device: "하위-001",
    msg: "c-1001 진행 7/10 (vp**** 캡차 → 보류)",
    level: "info",
  },
  {
    ts: "2026-06-28 10:20:45.010",
    tag: "[RESULT]",
    dir: "하위-001 → Admin",
    device: "하위-001",
    msg: "c-1001 완료: 성공3 / 보류2 / 대기초과3 / 실패2 ✅",
    level: "ok",
  },
  {
    ts: "2026-06-28 10:20:45.300",
    tag: "[CMD]",
    dir: "Admin → 하위-001",
    device: "하위-001",
    msg: "delete_accounts (실패 2건 자동삭제, commandId=c-1001)",
    level: "cmd",
  },
  {
    ts: "2026-06-28 10:21:02.300",
    tag: "[CMD]",
    dir: "Admin → 하위-003",
    device: "하위-003",
    msg: "distribute_accounts (4건, commandId=c-1002)",
    level: "cmd",
  },
  {
    ts: "2026-06-28 10:21:02.880",
    tag: "[RESULT]",
    dir: "하위-003 → Admin",
    device: "하위-003",
    msg: "c-1002 import 완료: imported 4 / skipped 0",
    level: "ok",
  },
  {
    ts: "2026-06-28 10:21:40.120",
    tag: "[RESULT]",
    dir: "하위-003 → Admin",
    device: "하위-003",
    msg: "c-1002 로그인 완료: 성공2 / 보류1 / 대기초과1 / 실패0",
    level: "ok",
  },
  {
    ts: "2026-06-28 10:22:05.660",
    tag: "[HEARTBEAT]",
    dir: "하위-002 ✗",
    device: "하위-002",
    msg: "하트비트 타임아웃 → offline(꺼짐)",
    level: "fail",
  },
  {
    ts: "2026-06-28 10:22:30.900",
    tag: "[CMD]",
    dir: "Admin → 하위-002",
    device: "하위-002",
    msg: "명령 거부 — 대상 offline (commandId=c-1003)",
    level: "fail",
  },
];

const LEVEL_COLOR: Record<Level, string> = {
  cmd: "var(--mantine-color-blue-4)",
  ok: "var(--mantine-color-teal-4)",
  fail: "var(--mantine-color-red-4)",
  warn: "var(--mantine-color-yellow-4)",
  info: "var(--mantine-color-gray-5)",
};

export function CommLog() {
  const [device, setDevice] = useState<string>("all");

  const devices = useMemo(
    () => Array.from(new Set(LOG_LINES.map((l) => l.device))),
    [],
  );

  const lines = useMemo(
    () => LOG_LINES.filter((l) => device === "all" || l.device === device),
    [device],
  );

  const toText = (rows: LogLine[]) =>
    rows
      .map((l) => `${l.ts}  ${l.tag.padEnd(11)} ${l.dir.padEnd(18)} ${l.msg}`)
      .join("\n");

  const exportTxt = () => {
    // 웹이라 Tauri 파일 다이얼로그 대신 브라우저 다운로드(Blob)로 .txt 저장.
    const blob = new Blob([toText(lines)], {
      type: "text/plain;charset=utf-8",
    });
    const url = URL.createObjectURL(blob);
    const a = document.createElement("a");
    a.href = url;
    a.download = "comm-log.txt";
    a.click();
    URL.revokeObjectURL(url);
    notifications.show({
      message: "통신 로그를 내보냈어요 (.txt)",
      color: "blue",
    });
  };

  return (
    <Box
      p="lg"
      style={{
        display: "flex",
        flexDirection: "column",
        gap: 12,
        height: "100%",
      }}
    >
      <Group justify="space-between">
        <Group gap="xs">
          <Text fw={800} size="xl">
            통신 로그
          </Text>
          <Text size="sm" c="dimmed">
            Admin↔하위 모든 통신(명령·결과·하트비트·IP변경) · 감사로그 (§7)
          </Text>
        </Group>
        <Group gap="xs">
          <Select
            size="sm"
            w={150}
            value={device}
            onChange={(v) => setDevice(v ?? "all")}
            data={[
              { value: "all", label: "전체 컴퓨터" },
              ...devices.map((d) => ({ value: d, label: d })),
            ]}
            comboboxProps={{ withinPortal: true }}
            aria-label="컴퓨터 필터"
          />
          <Button leftSection={<Icon.download size={16} />} onClick={exportTxt}>
            내보내기
          </Button>
        </Group>
      </Group>

      {/* 콘솔(터미널) 스타일 — PowerShell/명령창처럼 모노스페이스 + 어두운 배경 */}
      <Paper
        radius="md"
        p={0}
        style={{
          flex: 1,
          minHeight: 0,
          background: "var(--mantine-color-dark-8)",
          border: "1px solid var(--mantine-color-dark-4)",
          overflow: "hidden",
        }}
      >
        <ScrollArea h="100%" type="auto" p="md">
          <Box
            style={{
              fontFamily: "monospace",
              fontSize: 12.5,
              lineHeight: 1.7,
              whiteSpace: "pre",
            }}
          >
            {lines.map((l, i) => (
              <Box key={i} style={{ color: "var(--mantine-color-gray-3)" }}>
                <Text span c="dimmed" inherit>
                  {l.ts}
                </Text>
                {"  "}
                <Text span inherit style={{ color: LEVEL_COLOR[l.level] }}>
                  {l.tag.padEnd(11)}
                </Text>
                <Text span c="gray.5" inherit>
                  {l.dir.padEnd(18)}
                </Text>
                {"  "}
                {l.msg}
              </Box>
            ))}
            {lines.length === 0 && (
              <Text c="dimmed">선택한 컴퓨터의 통신 로그가 없습니다.</Text>
            )}
          </Box>
        </ScrollArea>
      </Paper>

      <Text size="xs" c="dimmed">
        ※ 비밀번호·쿠키 등 자격증명은 로그에 남기지 않습니다(기존 컨벤션). ID는
        앞 2글자만 노출(st****).
      </Text>
    </Box>
  );
}
