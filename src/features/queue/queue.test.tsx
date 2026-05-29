import { MantineProvider } from "@mantine/core";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, it, expect, vi } from "vitest";

import { Queue } from "./queue";

function renderQueue(go = vi.fn()) {
  render(
    <MantineProvider>
      <Queue go={go} />
    </MantineProvider>,
  );
  return go;
}

describe("Queue", () => {
  it("renders the title and both queue sections", () => {
    renderQueue();
    expect(
      screen.getByRole("heading", { name: "게시 큐" }),
    ).toBeInTheDocument();
    expect(screen.getByText("즉시 처리 대기열")).toBeInTheDocument();
    expect(screen.getByText("예약 대기")).toBeInTheDocument();
  });

  it("opens the running batch in 알림 when its row is clicked", async () => {
    const go = renderQueue();
    await userEvent.click(
      screen.getByText("삼성전자 4분기 실적 기대 — 매수 관점 정리"),
    );
    expect(go).toHaveBeenCalledWith("log", {
      logFilter: { batchId: "b0" },
    });
  });

  it("navigates to posts via '새 작업 추가'", async () => {
    const go = renderQueue();
    await userEvent.click(screen.getByRole("button", { name: /새 작업 추가/ }));
    expect(go).toHaveBeenCalledWith("posts");
  });
});
