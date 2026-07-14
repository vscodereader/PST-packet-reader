import { MantineProvider } from "@mantine/core";
import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { resetIpc } from "@/test/ipc";

import { ReportModal } from "./report-modal";

vi.mock("@tauri-apps/api/core", async () => ({
  invoke: (await import("@/test/ipc")).invoke,
}));

// report-finished 이벤트 핸들러를 붙잡아, 백엔드 완료 이벤트 도착을 테스트에서 흉내낸다.
const listeners: Record<string, (e: { payload: unknown }) => void> = {};
vi.mock("@tauri-apps/api/event", () => ({
  listen: (name: string, cb: (e: { payload: unknown }) => void) => {
    listeners[name] = cb;
    return Promise.resolve(() => {});
  },
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
    expect(
      screen.getByRole("radio", { name: "음란물입니다" }),
    ).toBeInTheDocument();
    expect(screen.getAllByRole("radio")).toHaveLength(7);
    // 로그인된 종목토론방 계정이 체크박스로 뜨고, 실패(error) 계정은 제외.
    expect(await screen.findByText("invest_king7")).toBeInTheDocument();
    expect(screen.queryByText("day_trader_x")).not.toBeInTheDocument();
    // IP 회전 체크박스.
    expect(
      screen.getByRole("checkbox", { name: /IP 회전/ }),
    ).toBeInTheDocument();
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
    // 비차단: 시작 안내 토스트(파랑). 모달은 열린 채 "신고 중…"으로 진행 상태를 보이고 닫지 않는다
    // (완료 이벤트가 오면 결과 패널을 이 모달 안에 띄운다).
    await waitFor(() =>
      expect(showMock).toHaveBeenCalledWith(
        expect.objectContaining({ color: "blue" }),
      ),
    );
    expect(
      await screen.findByRole("button", { name: "신고 중…" }),
    ).toBeInTheDocument();
    expect(onClose).not.toHaveBeenCalled();
  });

  it("shows a per-account×link result panel with raw failure reason on report-finished", async () => {
    renderModal();
    await screen.findByText("invest_king7");
    // 백엔드 완료 이벤트를 흉내낸다(계정×링크별 성공/실패 + 실패 사유 원문).
    const rawReason =
      '신고 실패(status=400): {"success":false,"message":"거부됨"}';
    act(() => {
      listeners["report-finished"]?.({
        payload: {
          total: 2,
          succeeded: 1,
          outcomes: [
            {
              accountId: "invest_king7",
              link: LINK1,
              success: true,
              message: "신고 완료",
            },
            {
              accountId: "invest_king7",
              link: LINK2,
              success: false,
              message: rawReason,
            },
          ],
        },
      });
    });
    // 결과 요약 + 실패 사유 원문이 패널에 보인다.
    expect(await screen.findByText("총 2건 중 1건 성공")).toBeInTheDocument();
    expect(screen.getByText(rawReason)).toBeInTheDocument();
    // 일부만 성공 → 노랑 완료 토스트.
    expect(showMock).toHaveBeenCalledWith(
      expect.objectContaining({ color: "yellow" }),
    );
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
