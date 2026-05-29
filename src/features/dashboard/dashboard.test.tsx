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
  it("renders the heading and stat labels", () => {
    renderDash();
    expect(screen.getByText("오늘은 무엇을 써볼까요?")).toBeInTheDocument();
    expect(screen.getByText("운영 계정")).toBeInTheDocument();
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
});
