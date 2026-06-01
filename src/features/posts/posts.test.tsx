import { MantineProvider } from "@mantine/core";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, it, expect, vi } from "vitest";

import { resetIpc } from "@/test/ipc";

import { Posts } from "./posts";

vi.mock("@tauri-apps/api/core", async () => ({
  invoke: (await import("@/test/ipc")).invoke,
}));

async function renderPosts(go = vi.fn()) {
  render(
    <MantineProvider>
      <Posts go={go} />
    </MantineProvider>,
  );
  // The list loads asynchronously over the IPC wrapper (mock backend here).
  // Use a non-draft title — drafts are excluded from this library list.
  await screen.findByText("카카오 반등 시그널 분석");
  return go;
}

describe("Posts", () => {
  beforeEach(() => {
    resetIpc();
  });

  it("renders the title and the 글쓰기 action", async () => {
    await renderPosts();
    expect(
      screen.getByRole("heading", { name: "글 관리" }),
    ).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /글쓰기/ })).toBeInTheDocument();
  });

  it("opens the writer modal from 글쓰기", async () => {
    await renderPosts();
    await userEvent.click(screen.getByRole("button", { name: /글쓰기/ }));
    expect(
      await screen.findByPlaceholderText("제목을 입력하세요"),
    ).toBeInTheDocument();
  });

  it("opens the publish modal from a row's 게시하기", async () => {
    await renderPosts();
    const [firstPublish] = screen.getAllByRole("button", { name: /게시하기/ });
    await userEvent.click(firstPublish!);
    expect(await screen.findByRole("dialog")).toHaveTextContent("게시 설정");
  });

  it("filters posts by kind", async () => {
    await renderPosts();
    await userEvent.click(screen.getByRole("button", { name: /^댓글/ }));
    // a known comment-kind post is visible, a known post-kind one is not
    expect(
      screen.getByText("반도체 흐름 코멘트 모음 (10종)"),
    ).toBeInTheDocument();
    expect(
      screen.queryByText("카카오 반등 시그널 분석"),
    ).not.toBeInTheDocument();
  });

  it("filters posts by the search box", async () => {
    await renderPosts();
    await userEvent.type(screen.getByPlaceholderText("제목 검색"), "카카오");
    expect(screen.getByText("카카오 반등 시그널 분석")).toBeInTheDocument();
    expect(
      screen.queryByText("이번 주 시장 브리핑 정리"),
    ).not.toBeInTheDocument();
  });

  it("opens the writer to edit when a card is clicked", async () => {
    await renderPosts();
    await userEvent.click(screen.getByText("카카오 반등 시그널 분석"));
    expect(
      await screen.findByDisplayValue("카카오 반등 시그널 분석"),
    ).toBeInTheDocument();
  });

  it("duplicates a post from the row menu", async () => {
    await renderPosts();
    const dots = screen
      .getAllByRole("button")
      .find((b) => b.textContent === "")!;
    await userEvent.click(dots);
    await userEvent.click(await screen.findByText("복제"));
    expect(await screen.findByText(/복사본/)).toBeInTheDocument();
  });

  it("deletes a post from the row menu", async () => {
    await renderPosts();
    const firstTitle = "#{종목명} 4분기 실적 기대 — 매수 관점 정리";
    expect(screen.getByText(firstTitle)).toBeInTheDocument();
    const dots = screen
      .getAllByRole("button")
      .find((b) => b.textContent === "")!;
    await userEvent.click(dots);
    await userEvent.click(await screen.findByText("삭제"));
    await waitFor(() =>
      expect(screen.queryByText(firstTitle)).not.toBeInTheDocument(),
    );
  });

  it("updates an existing post when edited and saved", async () => {
    await renderPosts();
    await userEvent.click(screen.getByText("카카오 반등 시그널 분석"));
    const title = await screen.findByDisplayValue("카카오 반등 시그널 분석");
    await userEvent.clear(title);
    await userEvent.type(title, "카카오 수정본");
    await userEvent.click(screen.getByRole("button", { name: "저장" }));
    expect(await screen.findByText("카카오 수정본")).toBeInTheDocument();
  });

  it("creates a new post from the writer", async () => {
    await renderPosts();
    await userEvent.click(screen.getByRole("button", { name: /글쓰기/ }));
    await userEvent.type(
      await screen.findByPlaceholderText("제목을 입력하세요"),
      "새로 쓴 글",
    );
    await userEvent.click(screen.getByRole("button", { name: "저장" }));
    expect(await screen.findByText("새로 쓴 글")).toBeInTheDocument();
  });

  it("saves a draft from the writer", async () => {
    await renderPosts();
    await userEvent.click(screen.getByRole("button", { name: /글쓰기/ }));
    await userEvent.type(
      await screen.findByPlaceholderText("제목을 입력하세요"),
      "초안 글",
    );
    await userEvent.click(screen.getByRole("button", { name: "임시저장" }));
    await userEvent.click(await screen.findByText("임시저장하기"));
    expect(screen.getByText(/임시저장 목록/)).toBeInTheDocument();
  });

  it("deletes a draft from the writer list", async () => {
    await renderPosts();
    await userEvent.click(screen.getByRole("button", { name: /글쓰기/ }));
    await userEvent.click(
      await screen.findByRole("button", { name: "임시저장" }),
    );
    const trashes = await screen.findAllByTitle("삭제");
    await userEvent.click(trashes[0]!);
    expect(screen.getByText(/임시저장 목록/)).toBeInTheDocument();
  });
});
