import { MantineProvider } from "@mantine/core";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { resetIpc } from "@/test/ipc";

import { ViewCountModal } from "./view-count-modal";

vi.mock("@tauri-apps/api/core", async () => ({
  invoke: (await import("@/test/ipc")).invoke,
}));

// Mantine notifications를 목킹해 완료 토스트 호출을 검증한다.
const showMock = vi.fn();
vi.mock("@mantine/notifications", () => ({
  notifications: { show: (...a: unknown[]) => showMock(...a) },
}));

function renderModal() {
  const onClose = vi.fn();
  render(
    <MantineProvider>
      <ViewCountModal open onClose={onClose} />
    </MantineProvider>,
  );
  return { onClose };
}

const LINK1 =
  "https://stock.naver.com/domestic/stock/005930/discussion/424274129";
const LINK2 =
  "https://stock.naver.com/domestic/stock/000660/discussion/424300000";

describe("ViewCountModal", () => {
  beforeEach(() => {
    resetIpc();
    showMock.mockClear();
  });

  it("renders the link input and a repeat-count number field", () => {
    renderModal();
    expect(
      screen.getByLabelText("조회수를 올릴 게시글 링크"),
    ).toBeInTheDocument();
    expect(screen.getByLabelText("반복 횟수")).toBeInTheDocument();
  });

  it("adds a link as a chip on Enter and clears the input", async () => {
    renderModal();
    const input = screen.getByLabelText("조회수를 올릴 게시글 링크");
    await userEvent.type(input, LINK1 + "{enter}");
    expect(await screen.findByText("글 #424274129")).toBeInTheDocument();
    expect(input).toHaveValue("");
  });

  it("disables 조회수 until at least one link and a positive count are set", async () => {
    renderModal();
    // 조회수 실행 버튼(정확한 이름 매칭 — 라벨이 '조회수').
    const runBtn = screen.getByRole("button", { name: "조회수" });
    // 링크가 없으면(기본 repeats=30이어도) 비활성.
    expect(runBtn).toBeDisabled();
    await userEvent.type(
      screen.getByLabelText("조회수를 올릴 게시글 링크"),
      LINK1 + "{enter}",
    );
    // 링크 1개 + repeats=30 → 활성.
    expect(runBtn).toBeEnabled();
  });

  it("disables 조회수 when the count is cleared to empty", async () => {
    renderModal();
    await userEvent.type(
      screen.getByLabelText("조회수를 올릴 게시글 링크"),
      LINK1 + "{enter}",
    );
    const runBtn = screen.getByRole("button", { name: "조회수" });
    expect(runBtn).toBeEnabled();
    // 숫자칸을 비우면 repeats가 유효치 못해 비활성이 되어야 한다.
    await userEvent.clear(screen.getByLabelText("반복 횟수"));
    expect(runBtn).toBeDisabled();
  });

  it("boosts every link with the chosen repeat count via boost_view_count", async () => {
    const { invoke } = await import("@/test/ipc");
    renderModal();
    const input = screen.getByLabelText("조회수를 올릴 게시글 링크");
    await userEvent.type(input, LINK1 + "{enter}");
    await userEvent.type(input, LINK2 + "{enter}");

    // 반복 횟수를 7로 지정.
    const countInput = screen.getByLabelText("반복 횟수");
    await userEvent.clear(countInput);
    await userEvent.type(countInput, "7");

    await userEvent.click(screen.getByRole("button", { name: "조회수" }));

    await waitFor(() =>
      expect(
        (invoke as unknown as { mock: { calls: unknown[][] } }).mock.calls.some(
          (c) => {
            if (c[0] !== "boost_view_count") return false;
            const a = c[1] as { links: string[]; repeats: number };
            return (
              a.links.includes(LINK1) &&
              a.links.includes(LINK2) &&
              a.repeats === 7
            );
          },
        ),
      ).toBe(true),
    );

    // 완료 토스트(2개 링크 모두 성공 → 초록).
    await waitFor(() =>
      expect(showMock).toHaveBeenCalledWith(
        expect.objectContaining({ color: "green" }),
      ),
    );
  });
});
