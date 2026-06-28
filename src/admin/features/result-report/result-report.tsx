import {
  Badge,
  Box,
  Divider,
  Group,
  Paper,
  ScrollArea,
  Stack,
  Text,
  ThemeIcon,
} from "@mantine/core";
import { IconDeviceDesktop } from "@tabler/icons-react";

// 로그인 결과 보고 화면(§10-4). device별로 ①이번 배치 분류·상세 + ②누적 합계.
// 성공은 개수만, 보류/대기초과/실패만 ID·PW(+사유) 명시. 실패는 보고 후 자동 삭제됨.

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

function ReportCard({ r }: { r: DeviceReport }) {
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

export function ResultReport() {
  return (
    <Box p="lg">
      <Group justify="space-between" mb="md">
        <Text fw={800} size="xl">
          로그인 결과 보고
        </Text>
        <Text size="sm" c="dimmed">
          device별 배치 상세 + 누적 합계 (§10-4)
        </Text>
      </Group>
      <Stack gap="md">
        {REPORTS.map((r) => (
          <ReportCard key={r.device} r={r} />
        ))}
      </Stack>
    </Box>
  );
}
