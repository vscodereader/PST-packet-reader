import {
  ActionIcon,
  Badge,
  Box,
  Button,
  Divider,
  Group,
  Paper,
  Stack,
  Text,
  ThemeIcon,
  Tooltip,
} from "@mantine/core";
import { notifications } from "@mantine/notifications";
import { IconDeviceDesktop } from "@tabler/icons-react";
import { useEffect, useRef, useState } from "react";

import { Icon } from "@/shared/ui/icons";

import { api, isOffline, type DeviceDto } from "../../api";

const CODE_TTL_SECONDS = 600; // 10분 만료(§6)

// 하위에 입력할 서버 주소(§6-1). 서버의 public_server_url은 "배포 전 결정"이라, 비어 있으면
// 우선 현재 API 주소를 보여준다(운영 배포 시 서버가 자기 공인 주소를 내려줌).
const DEFAULT_SERVER_ADDRESS = api.baseUrl;

// 서버 DeviceDto → 화면 Device. lastSeen은 상대 시각 문구로.
function fromDto(d: DeviceDto): Device {
  return {
    id: d.id,
    name: d.name,
    connected: d.connected,
    ip: d.ip,
    lastSeen: relativeTime(d.lastSeen),
  };
}
function relativeTime(iso: string): string {
  const t = Date.parse(iso);
  if (Number.isNaN(t)) return "—";
  const sec = Math.max(0, Math.floor((Date.now() - t) / 1000));
  if (sec < 5) return "방금 전";
  if (sec < 60) return `${sec}초 전`;
  if (sec < 3600) return `${Math.floor(sec / 60)}분 전`;
  return `${Math.floor(sec / 3600)}시간 전`;
}

interface Device {
  id: string;
  name: string;
  connected: boolean;
  ip: string | null;
  lastSeen: string;
}

// 더미 데이터 — 백엔드 미연결, 화면 확인용(§01-기기연결.md).
const INITIAL_DEVICES: Device[] = [
  {
    id: "d1",
    name: "하위-001",
    connected: true,
    ip: "1.2.3.4",
    lastSeen: "방금 전",
  },
  {
    id: "d2",
    name: "하위-002",
    connected: false,
    ip: null,
    lastSeen: "3분 전",
  },
  {
    id: "d3",
    name: "하위-003",
    connected: true,
    ip: "5.6.7.8",
    lastSeen: "1초 전",
  },
  {
    id: "d4",
    name: "하위-004",
    connected: false,
    ip: null,
    lastSeen: "10분 전",
  },
];

function mmss(total: number): string {
  const m = Math.floor(total / 60);
  const s = total % 60;
  return `${m}:${String(s).padStart(2, "0")}`;
}

/** 좌측 컴퓨터 아이콘 + (이름 / 상태 텍스트 + 상태 원) + 우측 삭제 버튼 한 행. */
function DeviceRow({
  device,
  onDelete,
}: {
  device: Device;
  onDelete: () => void;
}) {
  return (
    <Paper withBorder radius="md" p="sm">
      <Group gap="md" wrap="nowrap">
        <ThemeIcon
          size={42}
          radius="md"
          variant="light"
          color={device.connected ? "blue" : "gray"}
        >
          <IconDeviceDesktop size={24} />
        </ThemeIcon>
        <Box style={{ flex: 1, minWidth: 0 }}>
          <Group justify="space-between" wrap="nowrap">
            <Text fw={700} size="sm" truncate>
              {device.name}
            </Text>
            <Text size="xs" c="dimmed">
              {device.ip ? `IP ${device.ip}` : "—"} · {device.lastSeen}
            </Text>
          </Group>
          <Group gap={7} mt={4}>
            {/* 상태 원: connected=초록, disconnected=회색 */}
            <Box
              w={10}
              h={10}
              style={{
                borderRadius: 999,
                background: device.connected
                  ? "var(--mantine-color-green-6)"
                  : "var(--mantine-color-gray-4)",
              }}
            />
            {/* 상태 텍스트: disconnected는 빨간색 */}
            <Text size="xs" fw={600} c={device.connected ? "gray.7" : "red.6"}>
              {device.connected ? "connected" : "disconnected"}
            </Text>
          </Group>
        </Box>
        {/* 기기 삭제: 실제로는 DELETE /devices/{id} → 기기표에서 줄 삭제(=옛 기기토큰 자동 거부, §6). */}
        <Tooltip label="기기 삭제 (등록 해제)" withArrow>
          <ActionIcon
            variant="subtle"
            color="red"
            size="lg"
            aria-label={`${device.name} 삭제`}
            onClick={onDelete}
          >
            <Icon.trash size={18} />
          </ActionIcon>
        </Tooltip>
      </Group>
    </Paper>
  );
}

export function DeviceConnection() {
  const [code, setCode] = useState<string | null>(null);
  const [remaining, setRemaining] = useState(0);
  const [serverAddress, setServerAddress] = useState(DEFAULT_SERVER_ADDRESS);
  const [devices, setDevices] = useState<Device[]>(INITIAL_DEVICES);
  const [lastRefreshed, setLastRefreshed] = useState("방금 전");
  const timer = useRef<ReturnType<typeof setInterval> | null>(null);

  // 기기 목록 로드(서버 연결 시 실데이터, 오프라인이면 더미 유지). 마운트 + 5초 폴링(§6-3 자동 갱신).
  const loadDevices = () => {
    api.devices
      .list()
      .then((list) => {
        setDevices(list.map(fromDto));
        setLastRefreshed("방금 전");
      })
      .catch(() => {
        /* 오프라인 → 더미 유지 */
      });
  };
  useEffect(() => {
    loadDevices();
    const id = window.setInterval(loadDevices, 5000);
    return () => window.clearInterval(id);
  }, []);

  // 기기코드 만료 카운트다운.
  useEffect(() => {
    if (code == null) return;
    timer.current = setInterval(() => {
      setRemaining((r) => (r <= 1 ? 0 : r - 1));
    }, 1000);
    return () => {
      if (timer.current) clearInterval(timer.current);
    };
  }, [code]);

  const issueCode = async () => {
    try {
      const r = await api.devices.issueCode(); // POST /admin/device-codes(§10)
      setCode(r.code);
      setRemaining(r.expiresInSecs);
      if (r.serverUrl) setServerAddress(r.serverUrl);
      notifications.show({ message: "기기코드를 발급했어요", color: "blue" });
    } catch (e) {
      if (isOffline(e)) {
        // 오프라인 미리보기: 더미 코드.
        setCode(
          crypto
            .getRandomValues(new Uint32Array(1))[0]!
            .toString()
            .slice(0, 6)
            .padStart(6, "0"),
        );
        setRemaining(CODE_TTL_SECONDS);
        notifications.show({
          message: "기기코드를 발급했어요(미리보기)",
          color: "blue",
        });
      } else {
        notifications.show({
          message: e instanceof Error ? e.message : "발급 실패",
          color: "red",
        });
      }
    }
  };

  const copy = (text: string, label: string) => {
    void navigator.clipboard?.writeText(text);
    notifications.show({ message: `${label} 복사했어요`, color: "gray" });
  };

  const refresh = () => {
    // 수동 새로고침(§6-3) — 서버 재조회.
    loadDevices();
    notifications.show({ message: "목록을 새로고침했어요", color: "gray" });
  };

  const deleteDevice = async (id: string, name: string) => {
    // DELETE /devices/{id} → 서버가 기기표 줄 삭제(옛 토큰 자동 무효, §6-4). 그 뒤 새 코드로 재등록.
    try {
      await api.devices.remove(id);
    } catch (e) {
      if (!isOffline(e)) {
        notifications.show({
          message: e instanceof Error ? e.message : "삭제 실패",
          color: "red",
        });
        return;
      }
    }
    setDevices((prev) => prev.filter((d) => d.id !== id));
    notifications.show({
      message: `${name} 기기를 삭제했어요 (등록 해제)`,
      color: "red",
    });
  };

  // 연결 시 자동 새로고침 + 토스트 시연. 실제로는 admin SSE의 online 이벤트가 트리거(§6-3).
  const simulateConnect = () => {
    const target = devices.find((d) => !d.connected);
    if (!target) {
      notifications.show({
        message: "이미 모든 기기가 연결됨(데모)",
        color: "gray",
      });
      return;
    }
    setDevices((prev) =>
      prev.map((d) =>
        d.id === target.id
          ? { ...d, connected: true, ip: "10.0.0.9", lastSeen: "방금 전" }
          : d,
      ),
    );
    setLastRefreshed("방금 전"); // 자동 새로고침
    notifications.show({
      message: `${target.name} 기기가 연결되었습니다.`,
      color: "green",
    });
  };

  const expired = code != null && remaining === 0;
  const connectedCount = devices.filter((d) => d.connected).length;

  return (
    <Box
      p="lg"
      style={{
        display: "flex",
        flexDirection: "column",
        gap: 16,
        height: "100%",
      }}
    >
      {/* ── 상단 절반: 발급/복사 ── */}
      <Paper withBorder radius="md" p="lg">
        <Text fw={800} size="lg" mb={4}>
          기기 연결
        </Text>
        <Text size="sm" c="dimmed" mb="md">
          아래 두 값을 각 하위 앱 설정에 입력하세요.
        </Text>

        <Stack gap="sm">
          {/* 서버 주소 — 항상 표시(읽기 전용) */}
          <Group justify="space-between" wrap="nowrap">
            <Box>
              <Text size="xs" c="dimmed" fw={600}>
                서버 주소 (모든 하위 공통) · 미리보기 예시값
              </Text>
              <Text fw={700} ff="monospace">
                {serverAddress}
              </Text>
            </Box>
            <Button
              variant="light"
              leftSection={<Icon.copy size={16} />}
              onClick={() => copy(serverAddress, "서버 주소를")}
            >
              복사
            </Button>
          </Group>

          <Divider />

          {/* 기기코드 — 발급 후 표시 */}
          <Group justify="space-between" wrap="nowrap" align="flex-end">
            <Box>
              <Text size="xs" c="dimmed" fw={600}>
                기기코드 (1회용 · 하위 1대당 1개)
              </Text>
              {code == null ? (
                <Text c="dimmed">— 아직 발급 안 됨 —</Text>
              ) : (
                <Group gap="xs" align="center">
                  <Text fw={800} size="xl" ff="monospace" lh={1}>
                    {code}
                  </Text>
                  {expired ? (
                    <Badge color="red" variant="light">
                      만료됨
                    </Badge>
                  ) : (
                    <Badge color="blue" variant="light">
                      {mmss(remaining)} 남음
                    </Badge>
                  )}
                </Group>
              )}
            </Box>
            <Group gap="xs">
              {code != null && (
                <Button
                  variant="light"
                  leftSection={<Icon.copy size={16} />}
                  disabled={expired}
                  onClick={() => copy(code, "기기코드를")}
                >
                  복사
                </Button>
              )}
              <Button
                leftSection={<Icon.plus size={16} />}
                onClick={() => void issueCode()}
              >
                기기코드 발급
              </Button>
            </Group>
          </Group>
        </Stack>
      </Paper>

      {/* ── 하단 절반: 연결 현황 ── */}
      <Paper
        withBorder
        radius="md"
        p="lg"
        style={{
          flex: 1,
          minHeight: 0,
          display: "flex",
          flexDirection: "column",
        }}
      >
        <Group justify="space-between" mb="sm">
          <Group gap="xs">
            <Text fw={800} size="lg">
              연결된 컴퓨터
            </Text>
            <Badge variant="light" color="gray" radius="sm">
              {connectedCount}/{devices.length}
            </Badge>
            <Text size="xs" c="dimmed">
              마지막 갱신 {lastRefreshed}
            </Text>
          </Group>
          <Group gap="xs">
            <Button
              variant="default"
              size="sm"
              color="green"
              onClick={simulateConnect}
            >
              데모: 기기 연결 시뮬레이션
            </Button>
            <Button
              variant="light"
              size="sm"
              leftSection={<Icon.refresh size={16} />}
              onClick={refresh}
            >
              새로고침
            </Button>
          </Group>
        </Group>

        <Box style={{ flex: 1, minHeight: 0, overflowY: "auto" }}>
          <Stack gap="xs">
            {devices.map((d) => (
              <DeviceRow
                key={d.id}
                device={d}
                onDelete={() => void deleteDevice(d.id, d.name)}
              />
            ))}
          </Stack>
        </Box>
      </Paper>
    </Box>
  );
}
