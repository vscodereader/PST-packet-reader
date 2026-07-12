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
    // 내용변경 체크박스는 forum(종목토론방) 컨텍스트에서만 뜬다. 계정 자동선택이 비동기라
    // forum 카드("종목 선택" 버튼)가 뜰 때까지 기다린 뒤 체크한다.
    await screen.findByRole("button", { name: /종목 선택/ });
    await userEvent.click(await screen.findByLabelText("게시 후 내용 변경"));
    // 내용변경 제목 입력 클릭 + 타이핑
    const title = await screen.findByPlaceholderText("변경할 새 제목");
    await userEvent.click(title);
    await userEvent.type(title, "새 제목");
    expect((title as HTMLInputElement).value).toBe("새 제목");
  });
});
