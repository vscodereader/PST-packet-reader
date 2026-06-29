import {
  Anchor,
  Badge,
  Box,
  Button,
  Divider,
  Group,
  Paper,
  ScrollArea,
  SegmentedControl,
  Stack,
  Text,
  ThemeIcon,
} from "@mantine/core";
import { IconDeviceDesktop } from "@tabler/icons-react";
import { useState } from "react";

import type { PlatformId } from "@/shared/data/types";
import { Icon } from "@/shared/ui/icons";
import { PlatformLogo } from "@/shared/ui/platform-logo";

// 결과 보고 화면(§10-4). Admin은 하위에서 일어난 일을 전부 본다:
//  ① 로그인 결과(성공/보류/대기초과/실패 + 누적)
//  ② 게시 결과 — 어디에 게시했는지 + 성공 시 게시 내용·링크 / 실패 시 사유 + "자세히 보기" 백트레이스.
// 기존 데스크톱 앱 알림(notifications.tsx의 BatchItem/SubLog)과 같은 모델을 그대로 쓴다.

// ───────────────────────── 로그인 결과 ─────────────────────────

interface Line {
  loginId: string;
  pw: string;
  reason?: string;
}

interface DeviceReport {
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

// §10-4 예시를 그대로 더미화. pw는 실제처럼 두고 화면에서 마스킹(앞 2글자만 노출).
const REPORTS: DeviceReport[] = [
  {
    device: "하위-001",
    batch: {
      success: 3,
      onhold: [{ loginId: "stock_id041", pw: "ik7!naver22", reason: "캡차" }],
      timedout: [
        { loginId: "stock_id052", pw: "vp@2024kr" },
        { loginId: "stock_id058", pw: "mlab2024!!" },
      ],
      failed: [
        { loginId: "stock_id063", pw: "daily#stock1", reason: "비번오류" },
        { loginId: "stock_id067", pw: "stockpw22", reason: "보호조치" },
        { loginId: "stock_id071", pw: "naverabc1", reason: "비번오류" },
        { loginId: "stock_id074", pw: "qwer1234!", reason: "잠금" },
      ],
    },
    cumulative: { received: 20, success: 6, onhold: 3, timedout: 5, failed: 6 },
  },
  {
    device: "하위-003",
    batch: {
      success: 5,
      onhold: [
        { loginId: "stock_id102", pw: "phone1234", reason: "전화번호 입력" },
        { loginId: "stock_id108", pw: "cap!2024", reason: "캡차" },
      ],
      timedout: [{ loginId: "stock_id115", pw: "wait9999" }],
      failed: [
        { loginId: "stock_id121", pw: "chal0001", reason: "추가인증 필요" },
      ],
    },
    cumulative: { received: 9, success: 5, onhold: 2, timedout: 1, failed: 1 },
  },
];

// 앞 2글자만 보이고 나머지는 마스킹(•). ID·PW 공통.
function maskHead(s: string, visible = 2): string {
  if (s.length <= visible) return s;
  return s.slice(0, visible) + "•".repeat(s.length - visible);
}

// 모든 섹션이 같은 고정폭을 써서 ID·PW 열이 세로로 정렬되게 한다.
function LineRow({ line, withReason }: { line: Line; withReason: boolean }) {
  return (
    <Group gap="md" wrap="nowrap" style={{ fontSize: 12 }}>
      <Text w={150} ff="monospace" truncate>
        {maskHead(line.loginId)}
      </Text>
      <Text w={120} ff="monospace" c="dimmed" truncate>
        {maskHead(line.pw)}
      </Text>
      {withReason && (
        <Text c="dimmed" style={{ flex: 1 }} truncate>
          {line.reason ?? ""}
        </Text>
      )}
    </Group>
  );
}

function Section({
  title,
  color,
  lines,
  withReason,
}: {
  title: string;
  color: string;
  lines: Line[];
  withReason: boolean;
}) {
  if (lines.length === 0) return null;
  return (
    <Box>
      <Text fw={700} size="xs" c={color} mb={4}>
        {title} {lines.length}
      </Text>
      <Stack gap={2}>
        {lines.map((l, i) => (
          <LineRow key={i} line={l} withReason={withReason} />
        ))}
      </Stack>
    </Box>
  );
}

function LoginReportCard({ r }: { r: DeviceReport }) {
  const { batch, cumulative: c } = r;
  return (
    <Paper withBorder radius="md" p="lg">
      <Group justify="space-between" mb="xs">
        <Group gap="sm">
          <ThemeIcon size={38} radius="md" variant="light" color="blue">
            <IconDeviceDesktop size={22} />
          </ThemeIcon>
          <Text fw={800} size="lg">
            {r.device}
          </Text>
        </Group>
        <Group gap={6}>
          <Badge color="green" variant="light">
            성공 {batch.success}
          </Badge>
          <Badge color="yellow" variant="light">
            보류 {batch.onhold.length}
          </Badge>
          <Badge color="gray" variant="light">
            대기초과 {batch.timedout.length}
          </Badge>
          <Badge color="red" variant="light">
            실패 {batch.failed.length}
          </Badge>
        </Group>
      </Group>

      <Text size="xs" c="dimmed" mb="sm">
        이번 배치 — 성공은 개수만, 보류·대기초과·실패만 ID·PW(앞 2글자만
        노출)+사유 표시. 실패는 보고 후 자동 삭제됨.
      </Text>

      {/* 결과가 누적돼 길어지면 일정 높이까지만 보여주고 나머지는 스크롤(드래그바). */}
      <ScrollArea.Autosize mah={260} type="auto" offsetScrollbars>
        <Stack gap="sm" pr="sm">
          <Section
            title="보류"
            color="yellow.7"
            lines={batch.onhold}
            withReason
          />
          <Section
            title="대기초과"
            color="gray.7"
            lines={batch.timedout}
            withReason={false}
          />
          <Section title="실패" color="red.6" lines={batch.failed} withReason />
        </Stack>
      </ScrollArea.Autosize>

      <Divider my="sm" />

      <Group gap="xs">
        <Text fw={700} size="sm">
          누적
        </Text>
        <Text size="sm" c="dimmed">
          총 받은 계정 {c.received} · 성공 {c.success} / 보류 {c.onhold} /
          대기초과 {c.timedout} / 실패 {c.failed}
        </Text>
      </Group>
    </Paper>
  );
}

// ───────────────────────── 게시 결과 ─────────────────────────
// 데스크톱 앱 알림(notifications.tsx)의 BatchItem/PostedContent/SubLog 모델 그대로.
//  - 성공: posted(제목/본문/댓글/URL) → [게시 내용] 펼치면 내용 + 글 링크.
//  - 실패: trace(백트레이스) → [자세히 보기] 펼치면 어두운 콘솔 박스에 그대로.

interface Posted {
  title: string;
  body: string;
  comment?: string;
  url?: string;
}

interface PostItem {
  platform: PlatformId;
  target: string; // 어디에 게시했는지(종목토론방·카페 이름 등)
  loginId: string;
  status: "success" | "fail";
  msg: string; // 메인 사유(친절한 한국어)
  trace?: string; // 실패 시 "자세히 보기"용 백트레이스(util.rs transport_error_message! 형식)
  posted?: Posted; // 성공 시 실제 게시 내용 + 링크
}

interface PostBatch {
  device: string;
  title: string;
  at: string;
  items: PostItem[];
}

// 더미 게시 결과 — 실제로는 하위가 자기 로컬 LogBatch(게시 완료 로그)를 Admin에 보고한 것.
// trace는 실제 백트레이스 형식(원인 체인 + at 함수(파일:줄) + 스택)을 그대로 재현.
function ok(target: string, loginId: string, posted: Posted): PostItem {
  return {
    platform: "forum",
    target,
    loginId,
    status: "success",
    msg: "게시 완료",
    posted,
  };
}

const POST_BATCHES: PostBatch[] = [
  {
    // 한 컴퓨터에 10곳 게시 → 8개까지 보이고 나머지는 스크롤(드래그바)로 확인.
    device: "하위-001",
    title: "10개 종목토론방 게시",
    at: "2026-06-28 10:31",
    items: [
      ok("삼성전자 종목토론방", "chol_invest", {
        title: "삼성전자 오늘 흐름 정리",
        body: "오전 외국인 순매수 전환… (종목별 토큰 치환 후 실제 본문)",
        url: "https://finance.naver.com/item/board_read.naver?code=005930&nid=298451023",
      }),
      ok("카카오 종목토론방", "moa_stock7", {
        title: "카카오 반등 가능할까",
        body: "전일 종가 대비… (실제 게시된 본문)",
        comment: "지표상 단기 바닥 신호 보이네요",
        url: "https://finance.naver.com/item/board_read.naver?code=035720&nid=298451044",
      }),
      ok("SK하이닉스 종목토론방", "nara_pick", {
        title: "하이닉스 HBM 수급",
        body: "HBM 단가 상승 기대… (실제 본문)",
        url: "https://finance.naver.com/item/board_read.naver?code=000660&nid=298451077",
      }),
      ok("현대차 종목토론방", "sun_invest", {
        title: "현대차 배당 매력",
        body: "배당수익률 기준… (실제 본문)",
        url: "https://finance.naver.com/item/board_read.naver?code=005380&nid=298451090",
      }),
      ok("LG에너지솔루션 종목토론방", "hana_trade", {
        title: "엘솔 수주 모멘텀",
        body: "북미 공장 가동… (실제 본문)",
        url: "https://finance.naver.com/item/board_read.naver?code=373220&nid=298451101",
      }),
      ok("셀트리온 종목토론방", "kim_value", {
        title: "셀트리온 바이오시밀러",
        body: "신규 품목 허가… (실제 본문)",
        url: "https://finance.naver.com/item/board_read.naver?code=068270&nid=298451115",
      }),
      ok("NAVER 종목토론방", "park_long", {
        title: "네이버 광고 회복",
        body: "검색 광고 단가… (실제 본문)",
        url: "https://finance.naver.com/item/board_read.naver?code=035420&nid=298451128",
      }),
      {
        platform: "forum",
        target: "POSCO홀딩스 종목토론방",
        loginId: "viptrade77",
        status: "fail",
        msg: "[연결 실패] 게시 요청 전송 오류 — 잠시 후 자동 재시도",
        trace:
          "HTTP 전송 오류가 발생했습니다: [연결 실패] error sending request for url (https://finance.naver.com/item/board_write.naver) → 원인: connection reset by peer (os error 104)\n\n" +
          "at pstmacro_lib::forum_stocks::post::create_discussion (src-tauri/src/forum_stocks/post.rs:212)\n\n" +
          "   0: pstmacro_lib::util::backtrace_string\n" +
          "   1: pstmacro_lib::forum_stocks::post::create_discussion\n" +
          "   2: pstmacro_lib::ipc::queue_runner::run_post_job\n" +
          "   3: pstmacro_lib::ipc::queue_runner::process_account\n" +
          "   4: tokio::runtime::task::harness::poll\n",
      },
      {
        platform: "forum",
        target: "기아 종목토론방",
        loginId: "blue_chip",
        status: "fail",
        msg: "[타임아웃] 게시 응답 지연 — 다음 차례에 재시도",
        trace:
          "HTTP 전송 오류가 발생했습니다: [타임아웃] error sending request for url (https://finance.naver.com/item/board_write.naver) → 원인: operation timed out\n\n" +
          "at pstmacro_lib::forum_stocks::post::create_discussion (src-tauri/src/forum_stocks/post.rs:212)\n\n" +
          "   0: pstmacro_lib::util::backtrace_string\n" +
          "   1: pstmacro_lib::forum_stocks::post::create_discussion\n" +
          "   2: pstmacro_lib::ipc::queue_runner::run_post_job\n",
      },
      {
        platform: "forum",
        target: "카카오뱅크 종목토론방",
        loginId: "value_kim",
        status: "fail",
        msg: "글쓰기 폼을 찾지 못했습니다 — 로그인 세션 만료 의심",
        trace:
          "글쓰기 페이지 진입 실패: 글쓰기 폼(textarea)을 찾지 못했습니다\n\n" +
          "at pstmacro_lib::forum_stocks::post::open_write_form (src-tauri/src/forum_stocks/post.rs:148)\n\n" +
          "   0: pstmacro_lib::util::backtrace_string\n" +
          "   1: pstmacro_lib::forum_stocks::post::open_write_form\n" +
          "   2: pstmacro_lib::forum_stocks::post::create_discussion\n" +
          "   3: pstmacro_lib::ipc::queue_runner::run_post_job\n",
      },
    ],
  },
  {
    device: "하위-003",
    title: "3개 종목토론방 게시",
    at: "2026-06-28 10:42",
    items: [
      ok("LG화학 종목토론방", "good_pick", {
        title: "LG화학 소재 전망",
        body: "양극재 출하… (실제 본문)",
        url: "https://finance.naver.com/item/board_read.naver?code=051910&nid=298452010",
      }),
      ok("두산에너빌리티 종목토론방", "steady7", {
        title: "두산 원전 수주",
        body: "체코 계약 기대… (실제 본문)",
        url: "https://finance.naver.com/item/board_read.naver?code=034020&nid=298452021",
      }),
      {
        platform: "forum",
        target: "한미반도체 종목토론방",
        loginId: "trade_min",
        status: "fail",
        msg: "[연결 실패] 게시 요청 전송 오류 — 잠시 후 자동 재시도",
        trace:
          "HTTP 전송 오류가 발생했습니다: [연결 실패] error sending request for url (https://finance.naver.com/item/board_write.naver) → 원인: connection refused (os error 111)\n\n" +
          "at pstmacro_lib::forum_stocks::post::create_discussion (src-tauri/src/forum_stocks/post.rs:212)\n\n" +
          "   0: pstmacro_lib::util::backtrace_string\n" +
          "   1: pstmacro_lib::forum_stocks::post::create_discussion\n" +
          "   2: pstmacro_lib::ipc::queue_runner::run_post_job\n",
      },
    ],
  },
];

function statusColor(s: PostItem["status"]) {
  return s === "success" ? "green" : "red";
}

// 데스크톱 앱 SubLog와 동일한 한 행: 상태 아이콘 + 플랫폼 + 어디에 + 계정 + 사유,
// 성공이면 [게시 내용], 실패면 [자세히 보기] 토글.
function PostSubLog({ item }: { item: PostItem }) {
  const [showTrace, setShowTrace] = useState(false);
  const [showPosted, setShowPosted] = useState(false);
  const ok = item.status === "success";
  const color = statusColor(item.status);
  return (
    <Box
      px={14}
      py={9}
      style={{ borderTop: "1px solid var(--mantine-color-gray-2)" }}
    >
      <Group gap={11} wrap="nowrap">
        <ThemeIcon size={22} radius="xl" variant="light" color={color}>
          {ok ? <Icon.check size={13} /> : <Icon.x size={13} />}
        </ThemeIcon>
        <PlatformLogo id={item.platform} size={20} />
        <Group gap={6} style={{ flex: 1, minWidth: 0 }} wrap="nowrap">
          <Text fz={12.5} fw={700} truncate>
            {item.target}
          </Text>
          <Text fz={11.5} c="dimmed" ff="monospace">
            · {maskHead(item.loginId)}
          </Text>
        </Group>
        <Text fz={11.5} c={ok ? "dimmed" : "red"} truncate maw="40%">
          {item.msg}
        </Text>
        {item.status === "fail" && item.trace && (
          <Button
            size="compact-xs"
            variant="default"
            radius="xl"
            onClick={() => setShowTrace((s) => !s)}
          >
            {showTrace ? "접기" : "자세히 보기"}
          </Button>
        )}
        {item.posted && (
          <Button
            size="compact-xs"
            variant="default"
            radius="xl"
            onClick={() => setShowPosted((s) => !s)}
          >
            {showPosted ? "접기" : "게시 내용"}
          </Button>
        )}
      </Group>

      {/* 성공 — 실제 게시된 제목/본문/댓글 + 글 링크(클릭하면 새 탭). */}
      {item.posted && showPosted && (
        <Box
          ml={33}
          mt={9}
          p="sm"
          style={{
            background: "var(--mantine-color-gray-1)",
            borderRadius: "var(--mantine-radius-sm)",
          }}
        >
          {item.posted.title && (
            <Text fz={13} fw={700} mb={6} style={{ whiteSpace: "pre-wrap" }}>
              {item.posted.title}
            </Text>
          )}
          {item.posted.body && (
            <Text
              fz={12.5}
              mb={item.posted.comment ? 8 : 0}
              style={{ whiteSpace: "pre-wrap" }}
            >
              {item.posted.body}
            </Text>
          )}
          {item.posted.comment && (
            <>
              <Text fz={11.5} c="dimmed" mb={2}>
                댓글
              </Text>
              <Text
                fz={12.5}
                mb={item.posted.url ? 8 : 0}
                style={{ whiteSpace: "pre-wrap" }}
              >
                {item.posted.comment}
              </Text>
            </>
          )}
          {item.posted.url && (
            <Anchor
              href={item.posted.url}
              target="_blank"
              rel="noreferrer"
              fz={11.5}
              ff="monospace"
              style={{ wordBreak: "break-all" }}
            >
              {item.posted.url}
            </Anchor>
          )}
        </Box>
      )}

      {/* 실패 — 백트레이스(원인 체인 + at 함수(파일:줄) + 스택)를 그대로. */}
      {item.status === "fail" && item.trace && showTrace && (
        <Box
          component="pre"
          ml={33}
          mt={9}
          p="sm"
          style={{
            background: "#1f2329",
            color: "#e6e8eb",
            borderRadius: "var(--mantine-radius-sm)",
            fontSize: 11.5,
            lineHeight: 1.6,
            fontFamily: "ui-monospace, SFMono-Regular, Menlo, monospace",
            whiteSpace: "pre-wrap",
            overflowX: "auto",
          }}
        >
          {item.trace}
        </Box>
      )}
    </Box>
  );
}

function PostBatchCard({ b }: { b: PostBatch }) {
  // 성공/실패 배지를 누르면 그 상태만 필터(다시 누르면 전체). 컴퓨터(카드)마다 따로.
  const [filter, setFilter] = useState<"all" | "success" | "fail">("all");
  const okN = b.items.filter((i) => i.status === "success").length;
  const failN = b.items.filter((i) => i.status === "fail").length;
  const shown = b.items.filter((i) => filter === "all" || i.status === filter);
  const toggle = (f: "success" | "fail") =>
    setFilter((cur) => (cur === f ? "all" : f));
  return (
    <Paper withBorder radius="md" p={0} style={{ overflow: "hidden" }}>
      <Group justify="space-between" p="md">
        <Group gap="sm">
          <ThemeIcon size={38} radius="md" variant="light" color="blue">
            <IconDeviceDesktop size={22} />
          </ThemeIcon>
          <Box>
            <Text fw={800} size="lg" lh={1.2}>
              {b.device}
            </Text>
            <Text size="xs" c="dimmed">
              {b.title} · {b.at}
            </Text>
          </Box>
        </Group>
        <Group gap={6}>
          <Badge
            color="green"
            variant={filter === "success" ? "filled" : "light"}
            style={{ cursor: "pointer" }}
            onClick={() => toggle("success")}
          >
            성공 {okN}
          </Badge>
          {failN > 0 && (
            <Badge
              color="red"
              variant={filter === "fail" ? "filled" : "light"}
              style={{ cursor: "pointer" }}
              onClick={() => toggle("fail")}
            >
              실패 {failN}
            </Badge>
          )}
          {filter !== "all" && (
            <Button
              size="compact-xs"
              variant="subtle"
              color="gray"
              onClick={() => setFilter("all")}
            >
              전체 보기
            </Button>
          )}
        </Group>
      </Group>
      {/* 한 컴퓨터당 8개까지 보이고, 그 이상은 세로 스크롤(드래그바)로 내려서 확인. */}
      <ScrollArea.Autosize mah={336} type="auto">
        {shown.map((it, i) => (
          <PostSubLog key={i} item={it} />
        ))}
      </ScrollArea.Autosize>
    </Paper>
  );
}

// ───────────────────────── 화면 ─────────────────────────

export function ResultReport() {
  const [view, setView] = useState<"login" | "post">("login");
  return (
    <Box p="lg">
      <Group justify="space-between" mb="md">
        <Text fw={800} size="xl">
          결과 보고
        </Text>
        <Text size="sm" c="dimmed">
          하위에서 일어난 일을 Admin이 모두 확인 (§10-4)
        </Text>
      </Group>

      <SegmentedControl
        mb="md"
        value={view}
        onChange={(v) => setView(v as "login" | "post")}
        data={[
          { value: "login", label: "로그인 결과" },
          { value: "post", label: "게시 결과" },
        ]}
      />

      {view === "login" ? (
        <Stack gap="md">
          {REPORTS.map((r) => (
            <LoginReportCard key={r.device} r={r} />
          ))}
        </Stack>
      ) : (
        <Stack gap="md">
          <Text size="xs" c="dimmed">
            어디에 게시했는지 + 성공 시 [게시 내용]·링크 / 실패 시 사유 +
            [자세히 보기] 백트레이스. (데스크톱 앱 알림과 동일 모델)
          </Text>
          {POST_BATCHES.map((b, i) => (
            <PostBatchCard key={i} b={b} />
          ))}
        </Stack>
      )}
    </Box>
  );
}
