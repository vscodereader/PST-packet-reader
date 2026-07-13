import { MantineProvider } from "@mantine/core";
import { render, waitFor } from "@testing-library/react";
import { beforeEach, describe, it, expect, vi } from "vitest";

import { MacroApp } from "./app-shell";

// Capture the handlers app-shell registers via `listen(...)` so the test can
// fire a synthetic backend event and assert the resulting toast. This overrides
// the global no-op stub in src/test/setup.ts for this file only.
const handlers = vi.hoisted(
  () => new Map<string, (e: { payload: unknown }) => void>(),
);
vi.mock("@tauri-apps/api/event", () => ({
  listen: (name: string, cb: (e: { payload: unknown }) => void) => {
    handlers.set(name, cb);
    return Promise.resolve(() => {});
  },
  emit: () => Promise.resolve(),
}));

const showSpy = vi.hoisted(() => vi.fn());
vi.mock("@mantine/notifications", () => ({
  notifications: { show: showSpy },
}));

vi.mock("@tauri-apps/api/core", async () => ({
  invoke: (await import("@/test/ipc")).invoke,
}));

function renderApp() {
  render(
    <MantineProvider>
      <MacroApp />
    </MantineProvider>,
  );
}

describe("MacroApp content-edit toast listeners", () => {
  beforeEach(() => {
    localStorage.clear();
    showSpy.mockClear();
    handlers.clear();
  });

  it("shows a red 내용 변경 실패 toast on forum-content-edit-failed", async () => {
    renderApp();
    await waitFor(() =>
      expect(handlers.has("forum-content-edit-failed")).toBe(true),
    );
    handlers.get("forum-content-edit-failed")?.({
      payload: {
        loginId: "acc1",
        stock: "삼성전자",
        reason: "내용 변경 실패 — 세션 없음: NO_COOKIES",
      },
    });
    expect(showSpy).toHaveBeenCalledWith(
      expect.objectContaining({
        color: "red",
        title: "내용 변경 실패",
        message: expect.stringContaining("삼성전자"),
      }),
    );
  });
});
