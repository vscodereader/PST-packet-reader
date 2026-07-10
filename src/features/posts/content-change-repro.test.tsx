import { MantineProvider } from "@mantine/core";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, it, expect, vi } from "vitest";

import type { LibraryPost } from "@/shared/data/types";

import { PublishModal } from "./publish-modal";

vi.mock("@tauri-apps/api/core", async () => ({
  invoke: (await import("@/test/ipc")).invoke,
}));
const { notifShow } = vi.hoisted(() => ({ notifShow: vi.fn() }));
vi.mock("@mantine/notifications", () => ({
  notifications: { show: notifShow },
}));

const postDoc: LibraryPost = {
  id: "l1",
  title: "제목입니다",
  kind: "post",
  updated: "방금 전",
  words: 100,
  status: "ready",
  excerpt: "요약",
  body: "<p>본문</p>",
};

describe("내용변경 제목 클릭 재현", () => {
  it("게시 후 내용 변경 체크 → 제목 입력이 크래시하지 않는다", async () => {
    render(
      <MantineProvider>
        <PublishModal open doc={postDoc} onClose={vi.fn()} go={vi.fn()} />
      </MantineProvider>,
    );
    // 게시 후 내용 변경 체크
    await userEvent.click(screen.getByLabelText("게시 후 내용 변경"));
    // 내용변경 제목 입력 클릭 + 타이핑
    const title = await screen.findByPlaceholderText("변경할 새 제목");
    await userEvent.click(title);
    await userEvent.type(title, "새 제목");
    expect((title as HTMLInputElement).value).toBe("새 제목");
  });
});
