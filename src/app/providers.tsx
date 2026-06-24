import "@fontsource/pretendard";
import "@mantine/core/styles.css";
import "@mantine/notifications/styles.css";
import "./global.css";

import { Box, Button, MantineProvider } from "@mantine/core";
import {
  Notifications,
  cleanNotifications,
  cleanNotificationsQueue,
  useNotifications,
} from "@mantine/notifications";
import type { ReactNode } from "react";

import { theme } from "./theme";

interface AppProvidersProps {
  children: ReactNode;
}

/**
 * 알림(토스트)이 하나라도 떠 있으면 "알림 모두 닫기" 버튼을 보여준다(#267-10). 클릭하면
 * 화면에 보이는 알림은 물론 표시 한도(limit)를 넘어 대기 중인 알림까지 한 번에 모두 닫는다.
 * 알림이 0개가 되면 버튼도 자동으로 사라진다(X를 하나하나 누르지 않아도 됨).
 */
function CloseAllNotificationsButton() {
  const store = useNotifications();
  const total = store.notifications.length + store.queue.length;
  if (total === 0) return null;
  return (
    <Box
      style={{
        position: "fixed",
        bottom: 12,
        left: "50%",
        transform: "translateX(-50%)",
        // Mantine 알림 컨테이너보다 위에 떠 항상 누를 수 있게 한다.
        zIndex: 9999,
      }}
    >
      <Button
        size="xs"
        radius="xl"
        color="dark"
        onClick={() => {
          // 대기열(표시 한도 너머)까지 먼저 비우고, 보이는 알림도 모두 닫는다.
          cleanNotificationsQueue();
          cleanNotifications();
        }}
      >
        알림 모두 닫기 ({total})
      </Button>
    </Box>
  );
}

export function AppProviders({ children }: AppProvidersProps) {
  return (
    <MantineProvider theme={theme} defaultColorScheme="light">
      <Notifications position="bottom-center" />
      <CloseAllNotificationsButton />
      {children}
    </MantineProvider>
  );
}
