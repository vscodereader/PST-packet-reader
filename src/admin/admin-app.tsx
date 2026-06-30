import {
  AppShell,
  Badge,
  Box,
  Group,
  Select,
  Text,
  ThemeIcon,
  UnstyledButton,
} from "@mantine/core";
import { notifications } from "@mantine/notifications";
import { useState } from "react";

import { Icon, type IconName } from "@/shared/ui/icons";

import { api, isOffline } from "./api";
import { ChangePassword } from "./auth/change-password";
import { Login } from "./auth/login";
import { Signup } from "./auth/signup";
import { AccountDistribute } from "./features/account-distribute/account-distribute";
import { CommLog } from "./features/comm-log/comm-log";
import { DeviceConnection } from "./features/device-connection/device-connection";
import { Operators } from "./features/operators/operators";
import { ResultReport } from "./features/result-report/result-report";
import { AUTH_SCREENS, PREVIEW_SCREENS, type Screen } from "./screens";

interface NavEntry {
  id: Screen;
  icon: IconName;
  label: string;
}

// 사이드바(로그인 후 앱 화면).
const NAV: NavEntry[] = [
  { id: "devices", icon: "globe", label: "기기 연결" },
  { id: "distribute", icon: "users", label: "계정 분배" },
  { id: "report", icon: "chart", label: "결과 보고" },
  { id: "operators", icon: "settings", label: "운영자 관리" },
  { id: "change-pw", icon: "eye", label: "비밀번호 변경" },
  { id: "comm-log", icon: "fileText", label: "통신 로그" },
];

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
    </UnstyledButton>
  );
}

// 미리보기 화면 선택기 — 실제 앱엔 없고, 인증 없이 아무 화면이나 띄워보기 위한 것.
function PreviewSwitcher({
  screen,
  setScreen,
}: {
  screen: Screen;
  setScreen: (s: Screen) => void;
}) {
  const data = [
    {
      group: "인증(로그인 전)",
      items: PREVIEW_SCREENS.filter((s) => s.group === "인증(로그인 전)").map(
        (s) => ({ value: s.value, label: s.label }),
      ),
    },
    {
      group: "앱(로그인 후)",
      items: PREVIEW_SCREENS.filter((s) => s.group === "앱(로그인 후)").map(
        (s) => ({ value: s.value, label: s.label }),
      ),
    },
  ];
  return (
    <Group gap="xs" wrap="nowrap">
      <Badge color="orange" variant="light" radius="sm" size="sm">
        미리보기
      </Badge>
      <Select
        size="xs"
        w={200}
        value={screen}
        onChange={(v) => v && setScreen(v as Screen)}
        data={data}
        comboboxProps={{ withinPortal: true }}
        aria-label="미리보기 화면 선택"
      />
    </Group>
  );
}

function AppScreen({
  screen,
  go,
}: {
  screen: Screen;
  go: (s: Screen) => void;
}) {
  switch (screen) {
    case "devices":
      return <DeviceConnection />;
    case "distribute":
      return <AccountDistribute />;
    case "report":
      return <ResultReport />;
    case "operators":
      return <Operators />;
    case "change-pw":
      // 변경 후 재로그인(§5) — 실제 네비게이션 핸들러를 넘겨 로그인 화면으로 이동시킨다.
      return <ChangePassword forced={false} go={go} />;
    case "comm-log":
      return <CommLog />;
    default:
      return null;
  }
}

export function AdminApp() {
  const [screen, setScreen] = useState<Screen>("login");
  // 첫 SuperAdmin은 기본 비번이라 변경 전까지 로그인 시 강제 변경 화면으로 보낸다(§5).
  // (미리보기 데모용 상태. 실제로는 서버가 "비번 변경 필요" 플래그로 판단.)
  const [mustChangePw, setMustChangePw] = useState(true);
  const go = (s: Screen) => setScreen(s);

  // 로그인: 서버 인증 시도 → 성공 시 mustChangePassword면 강제 변경, 아니면 앱(기기 연결).
  // 서버 미연결(오프라인 미리보기)이면 기존 데모 동작으로 폴백(화면 무손상).
  const demoBranch = () => {
    if (mustChangePw) {
      notifications.show({
        message: "기본 비밀번호입니다 — 변경이 필요합니다",
        color: "orange",
      });
      go("force-pw");
    } else {
      notifications.show({ message: "로그인되었습니다", color: "blue" });
      go("devices");
    }
  };
  const handleLogin = async (loginId: string, pw: string) => {
    try {
      const r = await api.auth.login(loginId, pw);
      if (r.mustChangePassword) {
        notifications.show({
          message: "기본 비밀번호입니다 — 변경이 필요합니다",
          color: "orange",
        });
        go("force-pw");
      } else {
        notifications.show({ message: "로그인되었습니다", color: "blue" });
        go("devices");
      }
    } catch (e) {
      if (isOffline(e)) {
        demoBranch(); // 서버 없는 미리보기: 데모 분기
      } else {
        notifications.show({
          message: e instanceof Error ? e.message : "로그인 실패",
          color: "red",
        });
      }
    }
  };

  // 로그인 전(인증) 화면: 사이드바 없이 전체화면 + 우상단에 미리보기 선택기만.
  if (AUTH_SCREENS.includes(screen)) {
    return (
      <Box style={{ position: "relative" }}>
        <Box style={{ position: "fixed", top: 12, right: 16, zIndex: 200 }}>
          <PreviewSwitcher screen={screen} setScreen={setScreen} />
        </Box>
        {screen === "login" && <Login go={go} onLogin={handleLogin} />}
        {screen === "signup" && <Signup go={go} />}
        {screen === "force-pw" && (
          <ChangePassword
            forced
            go={go}
            onDone={() => setMustChangePw(false)}
          />
        )}
      </Box>
    );
  }

  // 로그인 후(앱) 화면: 사이드바 셸.
  return (
    <AppShell
      header={{ height: 62 }}
      navbar={{ width: 248, breakpoint: "sm" }}
      padding={0}
    >
      <AppShell.Header>
        <Group h="100%" px="md" gap="sm" justify="space-between" wrap="nowrap">
          <Text fw={700} c="dimmed" size="sm">
            PLTMacro Admin
          </Text>
          <PreviewSwitcher screen={screen} setScreen={setScreen} />
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
              Admin
            </Text>
            <Text size="xs" c="dimmed" fw={600} mt={2}>
              원격제어 관리
            </Text>
          </Box>
        </Group>

        <Box
          mt="xs"
          style={{ display: "flex", flexDirection: "column", gap: 2 }}
        >
          {NAV.map((n) => (
            <NavButton
              key={n.id}
              entry={n}
              active={screen === n.id}
              onClick={() => setScreen(n.id)}
            />
          ))}
        </Box>
      </AppShell.Navbar>

      <AppShell.Main
        h="100dvh"
        style={{ display: "flex", flexDirection: "column" }}
      >
        <Box style={{ flex: 1, minHeight: 0, overflowY: "auto" }}>
          <AppScreen screen={screen} go={go} />
        </Box>
      </AppShell.Main>
    </AppShell>
  );
}
