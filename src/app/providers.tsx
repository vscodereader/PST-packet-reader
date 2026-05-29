import "@fontsource/pretendard";
import "@mantine/core/styles.css";

import { MantineProvider } from "@mantine/core";
import type { ReactNode } from "react";

import { theme } from "./theme";

interface AppProvidersProps {
  children: ReactNode;
}

export function AppProviders({ children }: AppProvidersProps) {
  return (
    <MantineProvider theme={theme} defaultColorScheme="light">
      {children}
    </MantineProvider>
  );
}
