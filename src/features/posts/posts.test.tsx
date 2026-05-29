import { MantineProvider } from "@mantine/core";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, it, expect, vi } from "vitest";

import { Posts } from "./posts";

function renderPosts(go = vi.fn()) {
  render(
    <MantineProvider>
      <Posts go={go} />
    </MantineProvider>,
  );
  return go;
}

describe("Posts", () => {
  it("renders the title and the 글쓰기 action", () => {
    renderPosts();
    expect(
      screen.getByRole("heading", { name: "글 관리" }),
    ).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /글쓰기/ })).toBeInTheDocument();
  });

  it("opens the writer modal from 글쓰기", async () => {
    renderPosts();
    await userEvent.click(screen.getByRole("button", { name: /글쓰기/ }));
    expect(
      await screen.findByPlaceholderText("제목을 입력하세요"),
    ).toBeInTheDocument();
  });

  it("opens the publish modal from a row's 게시하기", async () => {
    renderPosts();
    const [firstPublish] = screen.getAllByRole("button", { name: /게시하기/ });
    await userEvent.click(firstPublish!);
    expect(await screen.findByRole("dialog")).toHaveTextContent("게시 설정");
  });

  it("filters posts by kind", async () => {
    renderPosts();
    await userEvent.click(screen.getByRole("button", { name: /^댓글/ }));
    // a known comment-kind post is visible, a known post-kind one is not
    expect(
      screen.getByText("반도체 흐름 코멘트 모음 (10종)"),
    ).toBeInTheDocument();
    expect(
      screen.queryByText("이번 주 시장 브리핑 정리"),
    ).not.toBeInTheDocument();
  });

  it("filters posts by the search box", async () => {
    renderPosts();
    await userEvent.type(screen.getByPlaceholderText("제목 검색"), "카카오");
    expect(screen.getByText("카카오 반등 시그널 분석")).toBeInTheDocument();
    expect(
      screen.queryByText("이번 주 시장 브리핑 정리"),
    ).not.toBeInTheDocument();
  });

  it("opens the writer to edit when a card is clicked", async () => {
    renderPosts();
    await userEvent.click(screen.getByText("이번 주 시장 브리핑 정리"));
    expect(
      await screen.findByDisplayValue("이번 주 시장 브리핑 정리"),
    ).toBeInTheDocument();
  });

  it("duplicates a post from the row menu", async () => {
    renderPosts();
    const dots = screen
      .getAllByRole("button")
      .find((b) => b.textContent === "")!;
    await userEvent.click(dots);
    await userEvent.click(await screen.findByText("복제"));
    expect(await screen.findByText(/복사본/)).toBeInTheDocument();
  });

  it("deletes a post from the row menu", async () => {
    renderPosts();
    const firstTitle = "#{종목명} 4분기 실적 기대 — 매수 관점 정리";
    expect(screen.getByText(firstTitle)).toBeInTheDocument();
    const dots = screen
      .getAllByRole("button")
      .find((b) => b.textContent === "")!;
    await userEvent.click(dots);
    await userEvent.click(await screen.findByText("삭제"));
    expect(screen.queryByText(firstTitle)).not.toBeInTheDocument();
  });
});
