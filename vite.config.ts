import { existsSync } from "node:fs";
import { fileURLToPath, URL } from "node:url";

import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";

const host = process.env.TAURI_DEV_HOST;

// Admin 웹 엔트리는 Admin 브랜치(#324)에만 존재한다(master는 pstmacro 전용).
// 파일이 있을 때만 멀티페이지 입력에 추가해, master(admin.html 없음)·#324(있음)
// 양쪽에서 `vite build`가 깨지지 않게 한다.
const adminHtml = fileURLToPath(new URL("./admin.html", import.meta.url));
const buildInput = {
  main: fileURLToPath(new URL("./index.html", import.meta.url)),
  ...(existsSync(adminHtml) ? { admin: adminHtml } : {}),
};

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

  // 멀티페이지 빌드 입력: 메인 앱(index.html) + (있으면) Admin 웹(admin.html).
  // admin.html은 #324에만 있으므로 buildInput에서 조건부로 포함한다(위 참고).
  build: {
    rollupOptions: {
      input: buildInput,
    },
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
