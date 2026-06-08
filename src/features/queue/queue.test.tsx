import { MantineProvider } from "@mantine/core";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, it, expect, vi } from "vitest";

import { resetIpc } from "@/test/ipc";

import { Queue } from "./queue";

vi.mock("@tauri-apps/api/core", async () => ({
  invoke: (await import("@/test/ipc")).invoke,
}));

async function renderQueue(go = vi.fn()) {
  render(
    <MantineProvider>
      <Queue go={go} />
    </MantineProvider>,
  );
  // now + scheduled lists load asynchronously over the IPC wrapper (mock here)
  await screen.findByText("삼성전자 4분기 실적 기대 — 매수 관점 정리");
  await screen.findByText("에코프로 조정 구간 대응 전략");
  return go;
}

describe("Queue", () => {
  beforeEach(() => {
    resetIpc();
  });

  it("renders the title and both queue sections", async () => {
    await renderQueue();
    expect(
      screen.getByRole("heading", { name: "게시 큐" }),
    ).toBeInTheDocument();
    expect(screen.getByText("즉시 처리 대기열")).toBeInTheDocument();
    expect(screen.getByText("예약 대기")).toBeInTheDocument();
  });

  it("opens the running batch in 알림 when its row is clicked", async () => {
    const go = await renderQueue();
    await userEvent.click(
      screen.getByText("삼성전자 4분기 실적 기대 — 매수 관점 정리"),
    );
    expect(go).toHaveBeenCalledWith("log", {
      logFilter: { batchId: "b0" },
    });
  });

  it("navigates to posts via '새 작업 추가'", async () => {
    const go = await renderQueue();
    await userEvent.click(screen.getByRole("button", { name: /새 작업 추가/ }));
    expect(go).toHaveBeenCalledWith("posts");
  });

  it("cancels a waiting item", async () => {
    await renderQueue();
    const title = "반도체 흐름 코멘트 10종";
    expect(screen.getByText(title)).toBeInTheDocument();
    await userEvent.click(screen.getAllByTitle("취소")[0]!);
    await waitFor(() =>
      expect(screen.queryByText(title)).not.toBeInTheDocument(),
    );
  });

  it("reorders waiting items with the move-down control", async () => {
    await renderQueue();
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
    await renderQueue();
    await userEvent.click(
      screen.getAllByRole("button", { name: /즉시 처리/ })[0]!,
    );
    await userEvent.click(screen.getAllByTitle("예약 취소")[0]!);
    expect(screen.getByText("예약 대기")).toBeInTheDocument();
  });

  it("shows 놓침 + 재예약 for a missed schedule", async () => {
    await renderQueue();
    // qs3(HBM)은 missed → "놓침" 뱃지와 "재예약" 버튼이 보인다.
    expect(screen.getByText("HBM 관련 기대 코멘트")).toBeInTheDocument();
    expect(screen.getByText("놓침")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "재예약" })).toBeInTheDocument();
  });

  it("reschedules a missed item, clearing 놓침", async () => {
    await renderQueue();
    expect(screen.getByText("놓침")).toBeInTheDocument();
    // 재예약 버튼(놓친 항목 전용)을 누르면 현재 시각으로 재예약돼 missed가 풀린다.
    await userEvent.click(screen.getByRole("button", { name: "재예약" }));
    await waitFor(() =>
      expect(screen.queryByText("놓침")).not.toBeInTheDocument(),
    );
  });

  it("reorders waiting items via drag and drop", async () => {
    await renderQueue();
    const q2 = "반도체 흐름 코멘트 10종";
    const q3 = "오늘의 특징주 정리 — 장 마감 요약";
    const q2row = screen.getByText(q2).closest("[draggable]")!;
    const q3row = screen.getByText(q3).closest("[draggable]")!;
    const dataTransfer = {
      effectAllowed: "",
      setData: () => {},
      getData: () => "",
    };
    fireEvent.dragStart(q2row, { dataTransfer });
    fireEvent.dragOver(q3row, { dataTransfer });
    fireEvent.dragEnd(q2row, { dataTransfer });
    const q2El = screen.getByText(q2);
    const q3El = screen.getByText(q3);
    expect(
      q3El.compareDocumentPosition(q2El) & Node.DOCUMENT_POSITION_FOLLOWING,
    ).toBeTruthy();
  });

  it("persists the new order so it survives a reload", async () => {
    const q2 = "반도체 흐름 코멘트 10종";
    const q3 = "오늘의 특징주 정리 — 장 마감 요약";
    const { unmount } = render(
      <MantineProvider>
        <Queue go={vi.fn()} />
      </MantineProvider>,
    );
    await screen.findByText(q2);

    // 첫 대기 항목(q2)을 한 칸 내린다 → 백엔드(reorder_queue_now)에 영속화돼야 한다.
    await userEvent.click(screen.getAllByTitle("우선순위 내리기")[0]!);
    unmount();

    // 재마운트 시 백엔드에서 다시 로드 → 로컬 상태가 아니라 영속화된 순서여야 한다.
    render(
      <MantineProvider>
        <Queue go={vi.fn()} />
      </MantineProvider>,
    );
    await screen.findByText(q2);
    const q2El = screen.getByText(q2);
    const q3El = screen.getByText(q3);
    expect(
      q3El.compareDocumentPosition(q2El) & Node.DOCUMENT_POSITION_FOLLOWING,
    ).toBeTruthy();
  });
});
