import { MantineProvider } from "@mantine/core";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { resetIpc } from "@/test/ipc";

import { LikeModal } from "./like-modal";

vi.mock("@tauri-apps/api/core", async () => ({
  invoke: (await import("@/test/ipc")).invoke,
}));

// Mantine notifications를 목킹해 완료 토스트 호출을 검증한다.
const showMock = vi.fn();
vi.mock("@mantine/notifications", () => ({
  notifications: { show: (...a: unknown[]) => showMock(...a) },
}));

function renderLike(over: Partial<Parameters<typeof LikeModal>[0]> = {}) {
  const onClose = vi.fn();
  render(
    <MantineProvider>
      <LikeModal open onClose={onClose} {...over} />
    </MantineProvider>,
  );
  return { onClose };
}

const LINK1 =
  "https://stock.naver.com/domestic/stock/005930/discussion/424274129";
const LINK2 =
  "https://stock.naver.com/domestic/stock/000660/discussion/424300000";

describe("LikeModal", () => {
  beforeEach(() => {
    resetIpc();
    showMock.mockClear();
  });

  it("renders the link input and forum login accounts as checkboxes", async () => {
    renderLike();
    expect(
      screen.getByLabelText("좋아요를 누를 게시글 링크"),
    ).toBeInTheDocument();
    expect(await screen.findByText("invest_king7")).toBeInTheDocument();
    // 로그인 실패(error) 계정은 제외.
    expect(screen.queryByText("day_trader_x")).not.toBeInTheDocument();
  });

  it("adds a link as a chip on Enter and clears the input", async () => {
    renderLike();
    const input = screen.getByLabelText("좋아요를 누를 게시글 링크");
    await userEvent.type(input, LINK1 + "{enter}");
    // 칩(글 #번호)이 뜨고 입력칸이 비워진다.
    expect(await screen.findByText("글 #424274129")).toBeInTheDocument();
    expect(input).toHaveValue("");
  });

  it("adds multiple links and can remove one", async () => {
    renderLike();
    const input = screen.getByLabelText("좋아요를 누를 게시글 링크");
    await userEvent.type(input, LINK1 + "{enter}");
    await userEvent.type(input, LINK2 + "{enter}");
    expect(screen.getByText("글 #424274129")).toBeInTheDocument();
    expect(screen.getByText("글 #424300000")).toBeInTheDocument();
    await userEvent.click(screen.getByLabelText(`${LINK1} 제거`));
    expect(screen.queryByText("글 #424274129")).not.toBeInTheDocument();
    expect(screen.getByText("글 #424300000")).toBeInTheDocument();
  });

  it("disables 좋아요 until at least one link and one account are chosen", async () => {
    renderLike();
    await screen.findByText("invest_king7");
    const likeBtn = screen.getByRole("button", { name: "좋아요" });
    expect(likeBtn).toBeDisabled();
    await userEvent.type(
      screen.getByLabelText("좋아요를 누를 게시글 링크"),
      LINK1 + "{enter}",
    );
    expect(likeBtn).toBeDisabled(); // 계정 미선택
    await userEvent.click(screen.getByText("invest_king7"));
    expect(likeBtn).toBeEnabled();
  });

  it("likes every (account × link) and passes all links to the backend", async () => {
    const { invoke } = await import("@/test/ipc");
    renderLike();
    await screen.findByText("invest_king7");
    const input = screen.getByLabelText("좋아요를 누를 게시글 링크");
    await userEvent.type(input, LINK1 + "{enter}");
    await userEvent.type(input, LINK2 + "{enter}");
    await userEvent.click(screen.getByText("invest_king7"));
    await userEvent.click(screen.getByRole("button", { name: "좋아요" }));

    await waitFor(() =>
      expect(
        (invoke as unknown as { mock: { calls: unknown[][] } }).mock.calls.some(
          (c) => {
            if (c[0] !== "like_discussion_post") return false;
            const a = c[1] as { postUrls: string[]; accountIds: string[] };
            return (
              a.postUrls.includes(LINK1) &&
              a.postUrls.includes(LINK2) &&
              a.accountIds.includes("invest_king7")
            );
          },
        ),
      ).toBe(true),
    );
    // 완료 토스트가 뜬다(2건 성공).
    await waitFor(() =>
      expect(showMock).toHaveBeenCalledWith(
        expect.objectContaining({ color: "green" }),
      ),
    );
  });

  it("supports 전체 선택 to select all shown accounts", async () => {
    renderLike();
    await screen.findByText("invest_king7");
    await userEvent.click(screen.getByRole("button", { name: "전체 선택" }));
    expect(
      screen.getByRole("button", { name: "전체 해제" }),
    ).toBeInTheDocument();
    const counter = await screen.findByText(/\/\d+개 선택됨/);
    expect(within(counter).queryByText(/^0\//)).not.toBeInTheDocument();
  });

  it("reaction='bad' — labels 싫어요 and calls dislike_discussion_post", async () => {
    const { invoke } = await import("@/test/ipc");
    renderLike({ reaction: "bad" });
    await screen.findByText("invest_king7");
    // 라벨·버튼·aria-label 이 전부 '싫어요'로 바뀐다.
    expect(screen.getByRole("button", { name: "싫어요" })).toBeInTheDocument();
    const input = screen.getByLabelText("싫어요를 누를 게시글 링크");
    await userEvent.type(input, LINK1 + "{enter}");
    await userEvent.click(screen.getByText("invest_king7"));
    await userEvent.click(screen.getByRole("button", { name: "싫어요" }));

    await waitFor(() =>
      expect(
        (invoke as unknown as { mock: { calls: unknown[][] } }).mock.calls.some(
          (c) => {
            // 좋아요가 아니라 싫어요 커맨드로 나가야 한다(패킷상 reactionType='bad').
            if (c[0] !== "dislike_discussion_post") return false;
            const a = c[1] as { postUrls: string[]; accountIds: string[] };
            return (
              a.postUrls.includes(LINK1) &&
              a.accountIds.includes("invest_king7")
            );
          },
        ),
      ).toBe(true),
    );
  });
});
