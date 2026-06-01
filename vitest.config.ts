import { fileURLToPath, URL } from "node:url";

import react from "@vitejs/plugin-react";
import { defineConfig } from "vitest/config";

export default defineConfig({
  plugins: [react()],
  resolve: {
    alias: {
      "@": fileURLToPath(new URL("./src", import.meta.url)),
    },
  },
  test: {
    environment: "jsdom",
    globals: true,
    setupFiles: ["./src/test/setup.ts"],
    coverage: {
      provider: "v8",
      reporter: ["text", "json-summary", "html"],
      include: ["src/**/*.{ts,tsx}"],
      exclude: [
        "src/test/**",
        "**/main.tsx",
        "**/vite-env.d.ts",
        "**/*.test.{ts,tsx}",
        "src/shared/bindings/**",
      ],
      thresholds: {
        lines: 93,
        statements: 93,
        functions: 90,
        branches: 80,
      },
    },
  },
});
