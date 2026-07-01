import { MantineProvider } from "@mantine/core";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { resetIpc } from "@/test/ipc";

import { LikeModal } from "./like-modal";

vi.mock("@tauri-apps/api/core", async () => ({
  invoke: (await import("@/test/ipc")).invoke,
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

describe("LikeModal", () => {
  beforeEach(() => {
    resetIpc();
  });

  it("renders the post-link input and forum login accounts as checkboxes", async () => {
    renderLike();
    expect(
      screen.getByLabelText("좋아요를 누를 게시글 링크"),
    ).toBeInTheDocument();
    // 종목토론방(forum) 게시 가능 계정은 보인다.
    expect(await screen.findByText("invest_king7")).toBeInTheDocument();
    // 로그인 실패(error) 계정은 좋아요 대상에서 제외된다.
    expect(screen.queryByText("day_trader_x")).not.toBeInTheDocument();
  });

  it("disables 좋아요 until a link and at least one account are chosen", async () => {
    renderLike();
    await screen.findByText("invest_king7");
    const likeBtn = screen.getByRole("button", { name: "좋아요" });
    expect(likeBtn).toBeDisabled();

    await userEvent.type(
      screen.getByLabelText("좋아요를 누를 게시글 링크"),
      "https://stock.naver.com/domestic/stock/005930/discussion/424274129",
    );
    // 링크만으로는 아직 계정 미선택이라 비활성.
    expect(likeBtn).toBeDisabled();

    await userEvent.click(screen.getByText("invest_king7"));
    expect(likeBtn).toBeEnabled();
  });

  it("likes the post for the selected account and shows the result", async () => {
    renderLike();
    await screen.findByText("invest_king7");

    await userEvent.type(
      screen.getByLabelText("좋아요를 누를 게시글 링크"),
      "https://stock.naver.com/domestic/stock/005930/discussion/424274129",
    );
    await userEvent.click(screen.getByText("invest_king7"));
    await userEvent.click(screen.getByRole("button", { name: "좋아요" }));

    await waitFor(() =>
      expect(screen.getByText("1개 중 1개 성공")).toBeInTheDocument(),
    );
    expect(screen.getByText("좋아요 완료")).toBeInTheDocument();
  });

  it("passes the selected account loginId(s) to the backend command", async () => {
    const { invoke } = await import("@/test/ipc");
    renderLike();
    await screen.findByText("invest_king7");

    const link =
      "https://stock.naver.com/domestic/stock/005930/discussion/424274129";
    await userEvent.type(
      screen.getByLabelText("좋아요를 누를 게시글 링크"),
      link,
    );
    await userEvent.click(screen.getByText("invest_king7"));
    await userEvent.click(screen.getByRole("button", { name: "좋아요" }));

    await waitFor(() =>
      expect(
        (invoke as unknown as { mock: { calls: unknown[][] } }).mock.calls.some(
          (c) =>
            c[0] === "like_discussion_post" &&
            (c[1] as { postUrl: string; accountIds: string[] }).postUrl ===
              link &&
            (
              c[1] as { postUrl: string; accountIds: string[] }
            ).accountIds.includes("invest_king7"),
        ),
      ).toBe(true),
    );
  });

  it("supports 전체 선택 to select all shown accounts", async () => {
    renderLike();
    await screen.findByText("invest_king7");
    const selectAll = screen.getByRole("button", { name: "전체 선택" });
    await userEvent.click(selectAll);
    // 전체 선택 후에는 '전체 해제'로 바뀐다.
    expect(
      screen.getByRole("button", { name: "전체 해제" }),
    ).toBeInTheDocument();
    // 선택 카운트가 0보다 크다.
    const counter = await screen.findByText(/\/\d+개 선택됨/);
    expect(within(counter).queryByText(/^0\//)).not.toBeInTheDocument();
  });
});
