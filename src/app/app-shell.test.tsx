import { MantineProvider } from "@mantine/core";
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, it, expect, vi } from "vitest";

import { MacroApp } from "./app-shell";

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

// Nav buttons live in the navbar landmark; scope queries there so dashboard
// CTAs that share a label (e.g. 계정 관리) don't cause ambiguous matches.
function navButton(name: RegExp | string) {
  return within(screen.getByRole("navigation")).getByRole("button", { name });
}

describe("MacroApp", () => {
  beforeEach(() => {
    localStorage.clear();
  });

  it("renders the dashboard view by default", async () => {
    renderApp();
    expect(navButton("대시보드")).toBeInTheDocument();
    // dashboard-only content (a stat card label) — loaded via async IPC.
    expect(await screen.findByText("운영 계정")).toBeInTheDocument();
  });

  it("navigates to 글 관리 and persists the view", async () => {
    renderApp();
    await userEvent.click(navButton(/글 관리/));
    expect(screen.getByPlaceholderText("제목 검색")).toBeInTheDocument();
    expect(localStorage.getItem("mc-view")).toBe("posts");
  });

  it("navigates to 게시 큐", async () => {
    renderApp();
    await userEvent.click(navButton(/게시 큐/));
    expect(screen.getByText("예약 대기")).toBeInTheDocument();
    expect(localStorage.getItem("mc-view")).toBe("queue");
  });

  it("navigates to 계정 관리", async () => {
    renderApp();
    await userEvent.click(navButton(/계정 관리/));
    expect(screen.getByText("엑셀 가져오기")).toBeInTheDocument();
    expect(localStorage.getItem("mc-view")).toBe("accounts");
  });

  it("opens the 알림 (log) view from the nav item", async () => {
    renderApp();
    await userEvent.click(navButton("알림"));
    expect(
      screen.getByPlaceholderText("내용·종목·계정 검색"),
    ).toBeInTheDocument();
    expect(localStorage.getItem("mc-view")).toBe("log");
  });

  it("restores the persisted view on mount", () => {
    localStorage.setItem("mc-view", "accounts");
    renderApp();
    expect(screen.getByText("엑셀 가져오기")).toBeInTheDocument();
  });

  it("opens the log view from the header bell", async () => {
    const { container } = render(
      <MantineProvider>
        <MacroApp />
      </MantineProvider>,
    );
    const bell = container.querySelector(".tabler-icon-bell");
    expect(bell).toBeTruthy();
    await userEvent.click(bell as Element);
    expect(localStorage.getItem("mc-view")).toBe("log");
    expect(
      screen.getByPlaceholderText("내용·종목·계정 검색"),
    ).toBeInTheDocument();
  });

  it("toggles the desktop sidebar via the burger (collapse + expand)", async () => {
    renderApp();
    const burger = screen.getByLabelText("사이드바 접기");
    await userEvent.click(burger); // collapse
    await userEvent.click(burger); // expand
    // Nav stays mounted and the burger remains usable after toggling.
    expect(navButton("대시보드")).toBeInTheDocument();
  });

  it("exposes a mobile menu burger", () => {
    renderApp();
    expect(screen.getByLabelText("메뉴 열기")).toBeInTheDocument();
  });
});
