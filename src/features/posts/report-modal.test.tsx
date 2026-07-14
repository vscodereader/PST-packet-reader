import { MantineProvider } from "@mantine/core";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { resetIpc } from "@/test/ipc";

import { ReportModal } from "./report-modal";

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
      <ReportModal open onClose={onClose} />
    </MantineProvider>,
  );
  return { onClose };
}

const LINK1 =
  "https://stock.naver.com/domestic/stock/000660/discussion/425406371";
const LINK2 =
  "https://stock.naver.com/domestic/stock/005930/discussion/424274129";

describe("ReportModal", () => {
  beforeEach(() => {
    resetIpc();
    showMock.mockClear();
  });

  it("renders the link input, all 7 reason radios, and forum accounts", async () => {
    renderModal();
    expect(screen.getByLabelText("신고할 게시글 링크")).toBeInTheDocument();
    // 사유 7개(설계서 §2.4)가 라디오로 뜬다.
    expect(
      screen.getByRole("radio", { name: "스팸홍보/도배입니다" }),
    ).toBeInTheDocument();
    expect(screen.getByRole("radio", { name: "음란물입니다" })).toBeInTheDocument();
    expect(screen.getAllByRole("radio")).toHaveLength(7);
    // 로그인된 종목토론방 계정이 체크박스로 뜨고, 실패(error) 계정은 제외.
    expect(await screen.findByText("invest_king7")).toBeInTheDocument();
    expect(screen.queryByText("day_trader_x")).not.toBeInTheDocument();
    // IP 회전 체크박스.
    expect(screen.getByRole("checkbox", { name: /IP 회전/ })).toBeInTheDocument();
  });

  it("adds a link as a chip on Enter and clears the input", async () => {
    renderModal();
    const input = screen.getByLabelText("신고할 게시글 링크");
    await userEvent.type(input, LINK1 + "{enter}");
    expect(await screen.findByText("글 #425406371")).toBeInTheDocument();
    expect(input).toHaveValue("");
  });

  it("disables 신고하기 until a link and an account are chosen", async () => {
    renderModal();
    await screen.findByText("invest_king7");
    const btn = screen.getByRole("button", { name: "신고하기" });
    expect(btn).toBeDisabled();
    await userEvent.type(
      screen.getByLabelText("신고할 게시글 링크"),
      LINK1 + "{enter}",
    );
    expect(btn).toBeDisabled(); // 계정 미선택
    await userEvent.click(screen.getByText("invest_king7"));
    expect(btn).toBeEnabled();
  });

  it("submits links × accounts with the chosen reason and IP-rotation flag", async () => {
    const { invoke } = await import("@/test/ipc");
    const { onClose } = renderModal();
    await screen.findByText("invest_king7");
    const input = screen.getByLabelText("신고할 게시글 링크");
    await userEvent.type(input, LINK1 + "{enter}");
    await userEvent.type(input, LINK2 + "{enter}");
    await userEvent.click(screen.getByText("invest_king7"));
    // 사유를 스팸홍보/도배(AA29)로 바꾼다.
    await userEvent.click(
      screen.getByRole("radio", { name: "스팸홍보/도배입니다" }),
    );
    // IP 회전 켜기.
    await userEvent.click(screen.getByRole("checkbox", { name: /IP 회전/ }));
    await userEvent.click(screen.getByRole("button", { name: "신고하기" }));

    await waitFor(() =>
      expect(
        (invoke as unknown as { mock: { calls: unknown[][] } }).mock.calls.some(
          (c) => {
            if (c[0] !== "report_posts") return false;
            const a = c[1] as {
              links: string[];
              accountIds: string[];
              reasonCode: string;
              rotateIp: boolean;
            };
            return (
              a.links.includes(LINK1) &&
              a.links.includes(LINK2) &&
              a.accountIds.includes("invest_king7") &&
              a.reasonCode === "AA29" &&
              a.rotateIp === true
            );
          },
        ),
      ).toBe(true),
    );
    // 비차단: 시작 안내 토스트(파랑) + 모달 닫힘.
    await waitFor(() =>
      expect(showMock).toHaveBeenCalledWith(
        expect.objectContaining({ color: "blue" }),
      ),
    );
    await waitFor(() => expect(onClose).toHaveBeenCalled());
  });

  it("defaults to the first reason (AA01) when none is changed", async () => {
    const { invoke } = await import("@/test/ipc");
    renderModal();
    await screen.findByText("invest_king7");
    await userEvent.type(
      screen.getByLabelText("신고할 게시글 링크"),
      LINK1 + "{enter}",
    );
    await userEvent.click(screen.getByText("invest_king7"));
    await userEvent.click(screen.getByRole("button", { name: "신고하기" }));
    await waitFor(() =>
      expect(
        (invoke as unknown as { mock: { calls: unknown[][] } }).mock.calls.some(
          (c) =>
            c[0] === "report_posts" &&
            (c[1] as { reasonCode: string }).reasonCode === "AA01" &&
            (c[1] as { rotateIp: boolean }).rotateIp === false,
        ),
      ).toBe(true),
    );
  });
});
