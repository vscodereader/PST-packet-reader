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

  it("cancels a waiting item", async () => {
    renderQueue();
    const title = "반도체 흐름 코멘트 10종";
    expect(screen.getByText(title)).toBeInTheDocument();
    await userEvent.click(screen.getAllByTitle("취소")[0]!);
    expect(screen.queryByText(title)).not.toBeInTheDocument();
  });

  it("reorders waiting items with the move-down control", async () => {
    renderQueue();
    const q2 = "반도체 흐름 코멘트 10종";
    const q3 = "오늘의 특징주 정리 — 장 마감 요약";
    // boundary: moving the first waiting item up is a no-op
    await userEvent.click(screen.getAllByTitle("우선순위 올리기")[0]!);
    // move first waiting item down → q3 now precedes q2
    await userEvent.click(screen.getAllByTitle("우선순위 내리기")[0]!);
    const q2El = screen.getByText(q2);
    const q3El = screen.getByText(q3);
    expect(
      q3El.compareDocumentPosition(q2El) & Node.DOCUMENT_POSITION_FOLLOWING,
    ).toBeTruthy();
  });

  it("exposes 즉시 처리 and 예약 취소 on scheduled rows", async () => {
    renderQueue();
    await userEvent.click(
      screen.getAllByRole("button", { name: /즉시 처리/ })[0]!,
    );
    await userEvent.click(screen.getAllByTitle("예약 취소")[0]!);
    expect(screen.getByText("예약 대기")).toBeInTheDocument();
  });
});
