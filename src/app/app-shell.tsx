import {
  AppShell,
  Badge,
  Box,
  Burger,
  Group,
  Indicator,
  Text,
  ThemeIcon,
  UnstyledButton,
} from "@mantine/core";
import { useDisclosure } from "@mantine/hooks";
import { useCallback, useEffect, useState } from "react";

import { Accounts } from "@/features/accounts/accounts";
import { Dashboard } from "@/features/dashboard/dashboard";
import { DeviceRegister } from "@/features/device-register/device-register";
import { Notifications } from "@/features/notifications/notifications";
import { Posts } from "@/features/posts/posts";
import { Queue } from "@/features/queue/queue";
import type { GoFn, LogFilter, ViewId } from "@/shared/data/types";
import { ipc } from "@/shared/ipc";
import { Icon, type IconName } from "@/shared/ui/icons";

const VIEWS: ViewId[] = [
  "dashboard",
  "posts",
  "queue",
  "log",
  "accounts",
  "device-register",
];

interface NavEntry {
  id: ViewId;
  icon: IconName;
  label: string;
  badge?: number;
}

function NavButton({
  entry,
  active,
  onClick,
}: {
  entry: NavEntry;
  active: boolean;
  onClick: () => void;
}) {
  const I = Icon[entry.icon];
  return (
    <UnstyledButton
      onClick={onClick}
      data-active={active || undefined}
      style={{
        display: "flex",
        alignItems: "center",
        gap: 11,
        width: "100%",
        height: 42,
        padding: "0 12px",
        borderRadius: "var(--mantine-radius-sm)",
        background: active ? "var(--mantine-color-blue-light)" : undefined,
        color: active
          ? "var(--mantine-color-blue-filled)"
          : "var(--mantine-color-gray-7)",
        fontSize: 14,
        fontWeight: active ? 700 : 600,
      }}
    >
      <I size={19} />
      <Box style={{ flex: 1 }}>{entry.label}</Box>
      {entry.badge != null && (
        <Badge
          size="sm"
          variant={active ? "filled" : "default"}
          radius="xl"
          color={active ? "blue" : "gray"}
        >
          {entry.badge}
        </Badge>
      )}
    </UnstyledButton>
  );
}

export function MacroApp() {
  const [view, setView] = useState<ViewId>(() => {
    const v = localStorage.getItem("mc-view");
    return v && VIEWS.includes(v as ViewId) ? (v as ViewId) : "dashboard";
  });

  const [logFilter, setLogFilter] = useState<LogFilter | null>(null);
  const [logNonce, setLogNonce] = useState(0);

  // Sidebar collapse: independent state for the mobile overlay and the desktop
  // fold, per Mantine's responsive AppShell pattern.
  const [mobileOpened, { toggle: toggleMobile, close: closeMobile }] =
    useDisclosure(false);
  const [desktopOpened, { toggle: toggleDesktop }] = useDisclosure(true);

  // Nav badge counts. Refreshed on mount and on every navigation so the badges
  // reflect the current data after adding/deleting accounts/posts/queue items
  // (previously loaded once on mount, so the numbers looked frozen).
  const [counts, setCounts] = useState({ posts: 0, queue: 0, accounts: 0 });
  // 알림 안읽음 표시: 가장 최근 알림(activity/log-batches의 at)과, 사용자가 마지막으로
  // 알림 화면을 본 시각(localStorage)을 비교해 벨의 빨간 점을 켠다.
  const [latestNotifAt, setLatestNotifAt] = useState(0);
  const [logSeenAt, setLogSeenAt] = useState<number>(() => {
    const v = Number(localStorage.getItem("mc-log-seen"));
    return Number.isFinite(v) ? v : 0;
  });
  // markLogSeen=true면(알림 화면을 보는 중) 최신 알림을 "읽음"으로 처리해 벨 점을 끈다.
  // setState는 effect 본문이 아니라 이 비동기 콜백 안에서만 호출한다.
  const refreshCounts = useCallback((markLogSeen = false) => {
    void Promise.all([
      ipc.posts.list(),
      ipc.queue.listNow(),
      ipc.queue.listScheduled(),
      ipc.accounts.list(),
      ipc.activity.list(),
      ipc.logBatches.list(),
    ]).then(([posts, now, sched, accounts, activity, batches]) => {
      setCounts({
        // 글관리 화면(posts.tsx)이 draft(임시저장)를 숨기므로 배지도 같은 기준으로 센다.
        posts: posts.filter((p) => p.status !== "draft").length,
        queue: now.length + sched.length,
        accounts: accounts.length,
      });
      const latest = Math.max(
        0,
        ...activity.map((a) => a.at),
        ...batches.map((b) => b.at),
      );
      setLatestNotifAt(latest);
      if (markLogSeen) {
        setLogSeenAt(latest);
        localStorage.setItem("mc-log-seen", String(latest));
      }
    });
  }, []);
  const hasUnread = latestNotifAt > logSeenAt;
  useEffect(() => {
    refreshCounts(localStorage.getItem("mc-view") === "log");
  }, [refreshCounts]);

  const go: GoFn = (v, opts) => {
    setView(v);
    localStorage.setItem("mc-view", v);
    refreshCounts(v === "log");
    if (v === "log") {
      setLogFilter(opts?.logFilter ?? null);
      setLogNonce((n) => n + 1);
    }
  };

  const nav: NavEntry[] = [
    { id: "dashboard", icon: "dashboard", label: "대시보드" },
    { id: "posts", icon: "pencil", label: "글 관리", badge: counts.posts },
    {
      id: "queue",
      icon: "layers",
      label: "게시 큐",
      badge: counts.queue,
    },
    { id: "log", icon: "bell", label: "알림" },
    {
      id: "accounts",
      icon: "users",
      label: "계정 관리",
      badge: counts.accounts,
    },
    { id: "device-register", icon: "globe", label: "원격제어" },
  ];

  return (
    <AppShell
      header={{ height: 62 }}
      navbar={{
        width: 248,
        breakpoint: "sm",
        collapsed: { mobile: !mobileOpened, desktop: !desktopOpened },
      }}
      padding={0}
    >
      <AppShell.Header>
        <Group h="100%" px="md" gap="sm">
          <Burger
            opened={mobileOpened}
            onClick={toggleMobile}
            hiddenFrom="sm"
            size="sm"
            aria-label="메뉴 열기"
          />
          <Burger
            opened={desktopOpened}
            onClick={toggleDesktop}
            visibleFrom="sm"
            size="sm"
            aria-label="사이드바 접기"
          />
          <Box style={{ flex: 1 }} />
          <Indicator color="red" size={8} offset={4} disabled={!hasUnread}>
            <ThemeIcon
              variant="subtle"
              color="gray"
              size="lg"
              onClick={() => go("log")}
              style={{ cursor: "pointer" }}
            >
              <Icon.bell size={20} />
            </ThemeIcon>
          </Indicator>
        </Group>
      </AppShell.Header>

      <AppShell.Navbar p="xs">
        <Group gap={10} px={6} py="sm">
          <ThemeIcon
            size={34}
            radius="md"
            variant="gradient"
            gradient={{ from: "#4dabf7", to: "#228be6", deg: 135 }}
          >
            <Icon.bolt size={19} />
          </ThemeIcon>
          <Box>
            <Text fw={800} size="lg" lh={1}>
              PLTMacro
            </Text>
            <Text size="xs" c="dimmed" fw={600} mt={2}>
              종토방·카페·밴드 등
            </Text>
          </Box>
        </Group>

        <Box
          mt="xs"
          style={{ display: "flex", flexDirection: "column", gap: 2 }}
        >
          {nav.map((n) => (
            <NavButton
              key={n.id}
              entry={n}
              active={view === n.id}
              onClick={() => {
                go(n.id);
                closeMobile();
              }}
            />
          ))}
        </Box>
      </AppShell.Navbar>

      <AppShell.Main
        h="100dvh"
        style={{ display: "flex", flexDirection: "column" }}
      >
        <Box style={{ flex: 1, minHeight: 0, overflowY: "auto" }}>
          {view === "dashboard" && <Dashboard go={go} />}
          {view === "posts" && <Posts go={go} />}
          {view === "queue" && <Queue go={go} />}
          {view === "log" && (
            <Notifications key={logNonce} filter={logFilter} />
          )}
          {view === "accounts" && <Accounts go={go} />}
          {view === "device-register" && <DeviceRegister />}
        </Box>
      </AppShell.Main>
    </AppShell>
  );
}
