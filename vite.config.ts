import { fileURLToPath, URL } from "node:url";

import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";

// @ts-expect-error process is a nodejs global
const host = process.env.TAURI_DEV_HOST;

// https://vite.dev/config/
export default defineConfig(async () => ({
  plugins: [react()],

  resolve: {
    alias: {
      "@": fileURLToPath(new URL("./src", import.meta.url)),
    },
    // Force a single instance of each Mantine/React package. A stale duplicate
    // @mantine/core in the pnpm store let the dep-optimizer inline a second
    // MantineContext into @mantine/notifications, so <Notifications> couldn't
    // find the provider ("MantineProvider was not found in component tree").
    dedupe: [
      "react",
      "react-dom",
      "@mantine/core",
      "@mantine/hooks",
      "@mantine/notifications",
    ],
  },

  // Co-optimize the Mantine packages so esbuild bundles them against one
  // shared @mantine/core instead of duplicating it per chunk.
  optimizeDeps: {
    include: ["@mantine/core", "@mantine/hooks", "@mantine/notifications"],
  },

  // Vite options tailored for Tauri development and only applied in `tauri dev` or `tauri build`
  //
  // 1. prevent Vite from obscuring rust errors
  clearScreen: false,
  // 2. tauri expects a fixed port, fail if that port is not available
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host
      ? {
          protocol: "ws",
          host,
          port: 1421,
        }
      : undefined,
    watch: {
      // 3. tell Vite to ignore watching `src-tauri`
      ignored: ["**/src-tauri/**"],
    },
  },
}));
