import { MantineProvider } from "@mantine/core";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, it, expect, vi } from "vitest";

import { Dashboard } from "./dashboard";

function renderDash(go = vi.fn()) {
  render(
    <MantineProvider>
      <Dashboard go={go} />
    </MantineProvider>,
  );
  return go;
}

describe("Dashboard", () => {
  it("renders the heading and stat labels", async () => {
    renderDash();
    expect(screen.getByText("오늘은 무엇을 써볼까요?")).toBeInTheDocument();
    // Stat tiles arrive from the async `listStats` IPC call.
    expect(await screen.findByText("운영 계정")).toBeInTheDocument();
    expect(screen.getByText("게시 성공률")).toBeInTheDocument();
  });

  it("navigates to posts when '새 글 작성' is clicked", async () => {
    const go = renderDash();
    await userEvent.click(screen.getByRole("button", { name: /새 글 작성/ }));
    expect(go).toHaveBeenCalledWith("posts");
  });

  it("navigates to accounts from a platform card", async () => {
    const go = renderDash();
    await userEvent.click(screen.getByText("종합토론방"));
    expect(go).toHaveBeenCalledWith("accounts");
  });

  it("navigates from a stat card to its target view", async () => {
    const go = renderDash();
    await userEvent.click(await screen.findByText("운영 계정"));
    expect(go).toHaveBeenCalledWith("accounts");
  });

  it("navigates to the queue from '큐 전체' and a scheduled row", async () => {
    const go = renderDash();
    await userEvent.click(screen.getByRole("button", { name: /큐 전체/ }));
    expect(go).toHaveBeenCalledWith("queue");
  });

  it("navigates to accounts from the '계정 관리' shortcut", async () => {
    const go = renderDash();
    await userEvent.click(screen.getByRole("button", { name: /^계정 관리/ }));
    expect(go).toHaveBeenCalledWith("accounts");
  });

  it("opens the queue from a scheduled row", async () => {
    const go = renderDash();
    await userEvent.click(
      await screen.findByText("삼성전자 4분기 실적 기대 — 매수 관점 정리"),
    );
    expect(go).toHaveBeenCalledWith("queue");
  });
});
