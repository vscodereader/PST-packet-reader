import {
  AppShell,
  Badge,
  Box,
  Group,
  Indicator,
  Text,
  ThemeIcon,
  UnstyledButton,
} from "@mantine/core";
import { useEffect, useState } from "react";

import { Accounts } from "@/features/accounts/accounts";
import { Dashboard } from "@/features/dashboard/dashboard";
import { Notifications } from "@/features/notifications/notifications";
import { Posts } from "@/features/posts/posts";
import { Queue } from "@/features/queue/queue";
import type { GoFn, LogFilter, ViewId } from "@/shared/data/types";
import { ipc } from "@/shared/ipc";
import { Icon, type IconName } from "@/shared/ui/icons";

const VIEWS: ViewId[] = ["dashboard", "posts", "queue", "log", "accounts"];

interface NavEntry {
  id: ViewId;
  icon: IconName;
  label: string;
  badge?: number;
}

const TITLES: Record<ViewId, string> = {
  dashboard: "대시보드",
  posts: "글 관리",
  queue: "게시 큐",
  log: "알림",
  accounts: "계정 관리",
};

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

  // Nav badge counts, loaded once over IPC.
  const [counts, setCounts] = useState({ posts: 0, queue: 0, accounts: 0 });
  useEffect(() => {
    void Promise.all([
      ipc.posts.list(),
      ipc.queue.listNow(),
      ipc.queue.listScheduled(),
      ipc.accounts.list(),
    ]).then(([posts, now, sched, accounts]) =>
      setCounts({
        posts: posts.length,
        queue: now.length + sched.length,
        accounts: accounts.length,
      }),
    );
  }, []);

  const go: GoFn = (v, opts) => {
    setView(v);
    localStorage.setItem("mc-view", v);
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
  ];

  return (
    <AppShell
      header={{ height: 62 }}
      navbar={{ width: 248, breakpoint: "xs" }}
      padding={0}
    >
      <AppShell.Header>
        <Group h="100%" px="md" gap="sm">
          <Text fw={700} size="md">
            {TITLES[view]}
          </Text>
          <Box style={{ flex: 1 }} />
          <Indicator color="red" size={8} offset={4}>
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
              onClick={() => go(n.id)}
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
        </Box>
      </AppShell.Main>
    </AppShell>
  );
}
