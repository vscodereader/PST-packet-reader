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
import { useEffect, useMemo, useRef, useState } from "react";

import { Icon } from "@/shared/ui/icons";

import { api } from "../../api";

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
  regOptionLabel,
  registrationsForDevice,
  type DeviceReg,
} from "./comm-log-filter";

// 통신로그 스크롤 위치를 화면 전환(언마운트) 후에도 기억한다. 다른 페이지 갔다 와도 맨 위로
// 튕기지 않고 마지막으로 읽던 위치에서 이어 읽게 한다(모듈 변수라 세션 동안 유지).
let savedScrollTop = 0;

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

// 더미 통신 로그 — 실제로는 서버 감사로그 DB에서 내려온다.
// 표시 규칙(설계 §10-5): ★통신 로그에 한해★ ID·PW를 마스킹 없이 그대로 표시한다(운영자 결정).
// (다른 화면 — 결과 보고 §10-4-1 등 — 은 마스킹 유지. 통신 로그만 예외.)
// 계정별로 한 줄씩(어느 ID/PW가 성공/보류/실패인지) + IP는 기존→바뀐, 명령은 무엇을 보냈는지 전부 명시.
const LOG_LINES: LogLine[] = [
  {
    ts: "2026-06-28 10:20:01.102",
    tag: "[SSE]",
    dir: "하위-001 → 서버",
    device: "하위-001",
    msg: "스트림 연결됨 (device_id=d1, 기기토큰 검증 OK)",
    level: "info",
  },
  {
    ts: "2026-06-28 10:20:01.340",
    tag: "[REGISTER]",
    dir: "하위-003 → 서버",
    device: "하위-003",
    msg: "기기코드 8237 등록 성공 → 기기토큰 발급 ✅",
    level: "ok",
  },
  // 하트비트: 각 하위가 주기적으로 "살아있음 + 현재 IP"를 보고. 두 하위의 IP가
  // 서로 다른 건 서로 다른 컴퓨터/폰이라 그런 것(같은 하위의 IP가 바뀌는 게 'IP 변경').
  {
    ts: "2026-06-28 10:20:03.001",
    tag: "[HEARTBEAT]",
    dir: "하위-001 → 서버",
    device: "하위-001",
    msg: "online · IP 211.234.194.24",
    level: "info",
  },
  {
    ts: "2026-06-28 10:20:03.220",
    tag: "[HEARTBEAT]",
    dir: "하위-003 → 서버",
    device: "하위-003",
    msg: "online · IP 121.165.10.77",
    level: "info",
  },
  // 분배: 어느 하위에 몇 건을, 어떤 계정(마스킹 ID)을 보냈는지 명시.
  {
    ts: "2026-06-28 10:20:05.220",
    tag: "[CMD]",
    dir: "Admin → 하위-001",
    device: "하위-001",
    msg: "distribute_accounts(계정 분배) 4건 → chol_invest/ch0lInvest!, moa_stock7/moaStock#22, viptrade77/vipTrade@77, dki_master/dkiMaster12 (commandId=c-1001, operator=admin)",
    level: "cmd",
  },
  {
    ts: "2026-06-28 10:20:05.880",
    tag: "[RESULT]",
    dir: "하위-001 → Admin",
    device: "하위-001",
    msg: "c-1001 import 완료: imported 4 / skipped 0 (chol_invest, moa_stock7, viptrade77, dki_master)",
    level: "ok",
  },
  // 자동 전체 로그인 시작 → 첫 계정에서 IP 로테이션(비행기모드). 이 구간엔 못 보내고 대기.
  {
    ts: "2026-06-28 10:20:06.300",
    tag: "[STATE]",
    dir: "하위-001 → 서버",
    device: "하위-001",
    msg: "ROTATING — IP 회전 시작 (기존 IP 211.234.194.24), 명령 버튼 비활성",
    level: "warn",
  },
  {
    ts: "2026-06-28 10:20:09.430",
    tag: "[SSE]",
    dir: "하위-001 ✗",
    device: "하위-001",
    msg: "스트림 끊김 (비행기모드 ON — 폰 인터넷 차단)",
    level: "warn",
  },
  {
    ts: "2026-06-28 10:20:12.770",
    tag: "[SSE]",
    dir: "하위-001 → 서버",
    device: "하위-001",
    msg: "재연결 성공 · 기존 IP 211.234.194.24 → 바뀐 IP 211.234.194.28 (✓ IP 변경됨, 토큰 동일)",
    level: "ok",
  },
  {
    ts: "2026-06-28 10:20:13.010",
    tag: "[HEARTBEAT]",
    dir: "하위-001 → 서버",
    device: "하위-001",
    msg: "online · IP 211.234.194.28",
    level: "info",
  },
  // 재연결 후, 대기했던 로그인 결과를 '계정별 한 줄씩' 같은 commandId로 올린다.
  {
    ts: "2026-06-28 10:20:14.220",
    tag: "[RESULT]",
    dir: "하위-001 → Admin",
    device: "하위-001",
    msg: "c-1001 로그인 chol_invest / ch0lInvest! → 성공(Active) ✅",
    level: "ok",
  },
  {
    ts: "2026-06-28 10:20:21.500",
    tag: "[STATE]",
    dir: "하위-001 → 서버",
    device: "하위-001",
    msg: "ROTATING — IP 회전 시작 (기존 IP 211.234.194.28), 다음 계정 로그인 전",
    level: "warn",
  },
  {
    ts: "2026-06-28 10:20:27.640",
    tag: "[SSE]",
    dir: "하위-001 → 서버",
    device: "하위-001",
    msg: "재연결 성공 · 기존 IP 211.234.194.28 → 바뀐 IP 211.234.194.31 (✓ IP 변경됨, 토큰 동일)",
    level: "ok",
  },
  {
    ts: "2026-06-28 10:20:28.900",
    tag: "[RESULT]",
    dir: "하위-001 → Admin",
    device: "하위-001",
    msg: "c-1001 로그인 moa_stock7 / moaStock#22 → 성공(Active) ✅",
    level: "ok",
  },
  {
    ts: "2026-06-28 10:20:35.100",
    tag: "[RESULT]",
    dir: "하위-001 → Admin",
    device: "하위-001",
    msg: "c-1001 로그인 viptrade77 / vipTrade@77 → 캡차 감지 → 보류(OnHold), 사람이 직접 처리",
    level: "info",
  },
  {
    ts: "2026-06-28 10:20:42.330",
    tag: "[RESULT]",
    dir: "하위-001 → Admin",
    device: "하위-001",
    msg: "c-1001 로그인 dki_master / dkiMaster12 → 비번오류(BadCredentials) → 실패",
    level: "fail",
  },
  {
    ts: "2026-06-28 10:20:43.010",
    tag: "[RESULT]",
    dir: "하위-001 → Admin",
    device: "하위-001",
    msg: "c-1001 배치 완료: 성공2 / 보류1 / 대기초과0 / 실패1 (총 받은 4)",
    level: "ok",
  },
  // 실패 계정만 자동 삭제 — 어떤 ID를, 왜 지웠는지 명시(되돌리기 불가라 회신 필수).
  {
    ts: "2026-06-28 10:20:45.300",
    tag: "[CMD]",
    dir: "Admin → 하위-001",
    device: "하위-001",
    msg: "delete_accounts(계정 삭제) 1건 → dki_master / dkiMaster12 (사유: 비번오류) (commandId=c-1001)",
    level: "cmd",
  },
  {
    ts: "2026-06-28 10:20:45.560",
    tag: "[RESULT]",
    dir: "하위-001 → Admin",
    device: "하위-001",
    msg: "c-1001 삭제 완료: dki_master / dkiMaster12 제거 · 유지(보류 viptrade77, 대기초과 0건)",
    level: "ok",
  },
  // 다른 컴퓨터가 꺼짐 → 그 하위로 간 명령은 거부. '무슨 명령'을 거부했는지 명시(§4-2).
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
    tag: "[REJECT]",
    dir: "Admin → 하위-002",
    device: "하위-002",
    msg: "거부: device_name=하위-002 명령=import_then_login_all(전체로그인) commandId=c-1003 사유=대상 컴퓨터 꺼짐(offline·거부코드 409) operator=admin src=/home/csw/projects/pstmacro/src-tauri/src/agent/commands.rs (예정 위치·핸들러 미구현)",
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

const LEVELS: Level[] = ["cmd", "ok", "fail", "info", "warn"];

export function CommLog() {
  // 기기 2단 필터(#444) — 목록1(컴퓨터) → 목록2(등록 이력) → 목록3(날짜).
  //  · comp: "Admin(전체)"(ADMIN_SCOPE) / 시스템 / 하위comN(=device_id)
  //  · reg : 목록1=하위comN 일 때 그 기기의 등록 이름 이력(정보 표시용, 필터엔 영향 없음)
  //  · date: 그 컴퓨터의 로그 날짜
  const [comp, setComp] = useState<string>(ADMIN_SCOPE);
  const [reg, setReg] = useState<string | null>(null);
  const [date, setDate] = useState<string | null>(null);
  // 서버 감사로그(§7) 로드. 연결 시 실데이터, 오프라인 미리보기면 더미 유지. 3초 폴링.
  const [allLines, setAllLines] = useState<LogLine[]>(LOG_LINES);
  // 등록 이력(#444) — 하위com별 등록 이름·등록시각. 오프라인이면 빈 배열(각 기기 개별).
  const [regs, setRegs] = useState<DeviceReg[]>([]);
  const viewportRef = useRef<HTMLDivElement>(null);
  const restoredRef = useRef(false);

  // 저장해둔 스크롤 위치 복원 — 단 **콘텐츠가 저장 위치까지 찬 뒤에** 딱 한 번만 복원한다.
  // 마운트 직후엔 짧은 더미 목록만 있어 곧바로 복원하면 최대 스크롤이 작아 위치가 맨 위로
  // 뭉개졌다(=이전 버그: 다른 페이지 갔다 오면 첫 줄로 튕김). 서버 실데이터(3초 폴링)가
  // 로드돼 스크롤 가능 높이가 저장 위치 이상이 될 때 복원해 마지막으로 읽던 곳에서 이어 읽는다.
  useEffect(() => {
    if (restoredRef.current) return;
    const v = viewportRef.current;
    if (!v) return;
    if (
      savedScrollTop === 0 ||
      v.scrollHeight - v.clientHeight >= savedScrollTop
    ) {
      v.scrollTop = savedScrollTop;
      restoredRef.current = true;
    }
  }, [allLines]);

  useEffect(() => {
    const load = () => {
      api.audit
        .list()
        .then((rows) => {
          if (rows.length === 0) return; // 빈 서버 → 더미 유지(미리보기)
          setAllLines(
            rows.map((r) => ({
              ts: r.ts,
              tag: r.tag,
              dir: r.dir,
              device: r.device || "시스템",
              msg: r.msg,
              level: (LEVELS as string[]).includes(r.level)
                ? (r.level as Level)
                : "info",
            })),
          );
        })
        .catch(() => {
          /* 오프라인 → 더미 유지 */
        });
    };
    load();
    const id = window.setInterval(load, 3000);
    return () => window.clearInterval(id);
  }, []);

  // 등록 이력 로드(#444) — 하위com별 등록 이름·시각. 오프라인이면 빈 배열(각 기기 개별로 뜸).
  useEffect(() => {
    const load = () => {
      api.devices
        .registrations()
        .then((rs) =>
          setRegs(
            rs.map((r) => ({
              deviceId: r.deviceId,
              name: r.name,
              registeredAt: r.registeredAt,
            })),
          ),
        )
        .catch(() => {
          /* 오프라인 → 빈 배열 유지 */
        });
    };
    load();
    const id = window.setInterval(load, 5000);
    return () => window.clearInterval(id);
  }, []);

  // 목록1(컴퓨터): Admin(전체) + 시스템(있으면) + 하위comN(로그의 device_id, 등록 이른 순으로 번호).
  const logDevices = useMemo(() => deviceOptions(allLines), [allLines]);
  const order = useMemo(() => comOrder(logDevices, regs), [logDevices, regs]);
  const hasSystem = useMemo(
    () => allLines.some((l) => l.device === SYSTEM_DEVICE),
    [allLines],
  );
  const compData = useMemo(
    () => [
      { value: ADMIN_SCOPE, label: "Admin(전체)" },
      ...(hasSystem ? [{ value: SYSTEM_DEVICE, label: "시스템" }] : []),
      ...order.map((d) => ({ value: d, label: comLabel(d, order) })),
    ],
    [hasSystem, order],
  );

  // 목록2(등록 이력) — 목록1이 하위comN(=device_id)일 때 그 기기의 등록 이름들(등록일 오름차순).
  const isDeviceComp = comp !== ADMIN_SCOPE && comp !== SYSTEM_DEVICE;
  const regData = useMemo(
    () =>
      isDeviceComp
        ? registrationsForDevice(regs, comp).map((r, i) => ({
            value: String(i),
            label: regOptionLabel(r),
          }))
        : [],
    [isDeviceComp, regs, comp],
  );

  // 목록3(날짜) — 특정 컴퓨터(하위comN·시스템) 선택 시 그 로그 날짜.
  const dateData = useMemo(
    () =>
      comp === ADMIN_SCOPE
        ? []
        : datesForDevice(allLines, comp).map((k) => ({
            value: k,
            label: dateLabel(k),
          })),
    [comp, allLines],
  );

  const lines = useMemo(
    () => filterLines(allLines, comp, date),
    [allLines, comp, date],
  );

  // 목록1 바뀌면 등록·날짜 초기화(종속 드롭다운).
  const onComp = (v: string | null) => {
    setComp(v ?? ADMIN_SCOPE);
    setReg(null);
    setDate(null);
  };

  const toText = (rows: LogLine[]) =>
    rows
      .map(
        (l) =>
          `${formatTs(l.ts)}  ${l.tag.padEnd(11)} ${l.dir.padEnd(18)} ${l.msg}`,
      )
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
          {/* 목록1 — 컴퓨터: Admin(전체) / 시스템 / 하위comN */}
          <Select
            size="sm"
            w={150}
            value={comp}
            onChange={onComp}
            data={compData}
            comboboxProps={{ withinPortal: true }}
            aria-label="컴퓨터 필터"
          />
          {/* 목록2 — 등록 이력: 그 하위com이 등록/재등록한 이름들(등록일 오름차순) */}
          <Select
            size="sm"
            w={190}
            value={reg}
            onChange={setReg}
            data={regData}
            clearable
            disabled={!isDeviceComp || regData.length === 0}
            placeholder="등록 이력"
            comboboxProps={{ withinPortal: true }}
            aria-label="등록 이력"
          />
          {/* 목록3 — 날짜: 그 컴퓨터의 로그 날짜 */}
          <Select
            size="sm"
            w={120}
            value={date}
            onChange={setDate}
            data={dateData}
            clearable
            disabled={comp === ADMIN_SCOPE}
            placeholder="날짜"
            comboboxProps={{ withinPortal: true }}
            aria-label="날짜 선택"
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
        <ScrollArea
          h="100%"
          type="always"
          scrollbarSize={12}
          p="md"
          classNames={{
            scrollbar: "commlog-scrollbar",
            thumb: "commlog-thumb",
          }}
          viewportRef={viewportRef}
          onScrollPositionChange={({ y }) => {
            savedScrollTop = y;
          }}
        >
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
                  {formatTs(l.ts)}
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
              <Text c="dimmed">
                {comp === ADMIN_SCOPE
                  ? "위에서 컴퓨터(하위com)를 선택하면 통신 로그가 표시됩니다."
                  : "선택한 조건의 통신 로그가 없습니다."}
              </Text>
            )}
          </Box>
        </ScrollArea>
      </Paper>

      <Text size="xs" c="dimmed">
        ※ <b>통신 로그에 한해</b> ID·PW를 마스킹 없이 그대로 표시합니다(운영자
        결정). 다른 화면(결과 보고 등)은 마스킹 유지. 계정별
        성공/보류/실패·사유, IP는 기존→바뀐, 명령은 무엇을 보냈는지 모두 표시.
      </Text>
    </Box>
  );
}
