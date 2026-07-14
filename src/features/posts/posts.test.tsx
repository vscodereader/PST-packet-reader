import { MantineProvider } from "@mantine/core";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, it, expect, vi } from "vitest";

import { ipc } from "@/shared/ipc";
import { invoke as ipcBackend, resetIpc } from "@/test/ipc";

import { Posts } from "./posts";

vi.mock("@tauri-apps/api/core", async () => ({
  invoke: (await import("@/test/ipc")).invoke,
}));

vi.mock("@tauri-apps/plugin-dialog", () => ({
  open: vi.fn().mockResolvedValue(null),
}));

const { notifShow } = vi.hoisted(() => ({ notifShow: vi.fn() }));
vi.mock("@mantine/notifications", () => ({
  notifications: { show: notifShow },
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
    notifShow.mockClear();
    vi.spyOn(ipc.activity, "append").mockResolvedValue(undefined);
  });

  it("renders the title and the 글쓰기 action", async () => {
    await renderPosts();
    expect(
      screen.getByRole("heading", { name: "글 관리" }),
    ).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /글쓰기/ })).toBeInTheDocument();
  });

  it("renders 신고하기 between 엑셀 가져오기 and 조회수", async () => {
    await renderPosts();
    const excel = screen.getByRole("button", { name: /엑셀 가져오기/ });
    const report = screen.getByRole("button", { name: /신고하기/ });
    const viewCount = screen.getByRole("button", { name: "조회수" });
    // DOM 순서: 엑셀 가져오기 → 신고하기 → 조회수 (툴바 삽입 위치 검증).
    expect(
      excel.compareDocumentPosition(report) & Node.DOCUMENT_POSITION_FOLLOWING,
    ).toBeTruthy();
    expect(
      report.compareDocumentPosition(viewCount) &
        Node.DOCUMENT_POSITION_FOLLOWING,
    ).toBeTruthy();
  });

  it("opens the report modal from 신고하기", async () => {
    await renderPosts();
    await userEvent.click(screen.getByRole("button", { name: /신고하기/ }));
    expect(await screen.findByRole("dialog")).toHaveTextContent("신고 사유");
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

  it("opens the writer to edit from the row menu", async () => {
    await renderPosts();
    const dots = screen
      .getAllByRole("button")
      .find((b) => b.textContent === "")!;
    await userEvent.click(dots);
    await userEvent.click(await screen.findByText("편집"));
    expect(
      await screen.findByPlaceholderText("제목을 입력하세요"),
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

  it("fires excel import — opens open dialog and calls importPosts", async () => {
    const { open } = await import("@tauri-apps/plugin-dialog");
    vi.mocked(open).mockResolvedValueOnce("/tmp/게시글.xlsx");
    vi.mocked(ipcBackend).mockClear();
    await renderPosts();
    await userEvent.click(
      screen.getByRole("button", { name: /엑셀 가져오기/ }),
    );
    expect(vi.mocked(open)).toHaveBeenCalledWith(
      expect.objectContaining({ multiple: false }),
    );
    await waitFor(() =>
      expect(
        vi
          .mocked(ipcBackend)
          .mock.calls.some((c) => c[0] === "import_posts_xlsx"),
      ).toBe(true),
    );
    await waitFor(() =>
      expect(notifShow).toHaveBeenCalledWith(
        expect.objectContaining({
          color: "green",
          message: expect.stringContaining("가져옴"),
        }),
      ),
    );
  });

  it("does not invoke import when open dialog is cancelled", async () => {
    const { open } = await import("@tauri-apps/plugin-dialog");
    vi.mocked(open).mockResolvedValueOnce(null);
    vi.mocked(ipcBackend).mockClear();
    await renderPosts();
    await userEvent.click(
      screen.getByRole("button", { name: /엑셀 가져오기/ }),
    );
    expect(
      vi
        .mocked(ipcBackend)
        .mock.calls.some((c) => c[0] === "import_posts_xlsx"),
    ).toBe(false);
  });

  it("logs to activity feed when import IPC command rejects", async () => {
    const { open } = await import("@tauri-apps/plugin-dialog");
    vi.mocked(open).mockResolvedValueOnce("/tmp/게시글.xlsx");
    const realImpl = vi.mocked(ipcBackend).getMockImplementation()! as (
      cmd: string,
      args?: Record<string, unknown>,
    ) => Promise<unknown>;
    vi.mocked(ipcBackend).mockImplementation((cmd, args) =>
      cmd === "import_posts_xlsx"
        ? Promise.reject(new Error("parse error"))
        : realImpl(cmd, args),
    );
    try {
      await renderPosts();
      await userEvent.click(
        screen.getByRole("button", { name: /엑셀 가져오기/ }),
      );
      await waitFor(() =>
        expect(ipc.activity.append).toHaveBeenCalledWith(
          "error",
          expect.stringContaining("가져오기"),
        ),
      );
    } finally {
      vi.mocked(ipcBackend).mockImplementation(realImpl);
    }
  });
});
