import { MantineProvider } from "@mantine/core";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

// 블로그 새 글 발행(16-블로그새글) 구성 — 순수 헬퍼(대상 조립·발행 게이트) + 전송 배선 검증.
// 네트워크(api)·알림(notifications)·편집기(BlockEditor, Tauri 의존)는 mock.
const send = vi.hoisted(() =>
  vi.fn((_req: Record<string, unknown>) =>
    Promise.resolve({ ok: true, commandId: "c1" }),
  ),
);

vi.mock("../../api", () => ({
  isOffline: () => false,
  api: { blogWrite: { send } },
}));
vi.mock("@mantine/notifications", () => ({
  notifications: { show: vi.fn() },
}));
// BlockEditor는 Tauri IPC/파일다이얼로그에 의존하므로 테스트에선 stub으로 대체(본문 블록은 빈 배열).
vi.mock("@/features/blog/block-editor", () => ({
  BlockEditor: () => <div data-testid="block-editor-stub" />,
}));

import {
  BlogWriteConfig,
  buildBlogWriteTargets,
  canSendBlogWrite,
} from "./blog-write";

describe("buildBlogWriteTargets", () => {
  it("blogId 오버라이드를 쓰고, 비면 loginId를 블로그명으로 폴백한다", () => {
    const t = buildBlogWriteTargets(["acc_a", "acc_b"], {
      acc_a: "press02",
      acc_b: "  ",
    });
    expect(t).toEqual([
      { loginId: "acc_a", blogId: "press02" },
      { loginId: "acc_b", blogId: "acc_b" },
    ]);
  });

  it("오버라이드가 없으면 loginId 그대로 블로그명", () => {
    expect(buildBlogWriteTargets(["x"], {})).toEqual([
      { loginId: "x", blogId: "x" },
    ]);
  });
});

describe("canSendBlogWrite", () => {
  it("제목이나 블록이 있고 계정이 1개 이상이면 발행 가능", () => {
    expect(canSendBlogWrite("제목", [], ["a"])).toBe(true);
    expect(
      canSendBlogWrite(
        "",
        [{ id: "b1", type: "code", code: "x", align: "justify" }],
        ["a"],
      ),
    ).toBe(true);
  });

  it("내용이 비었거나 계정이 없으면 발행 불가", () => {
    expect(canSendBlogWrite("   ", [], ["a"])).toBe(false);
    expect(canSendBlogWrite("제목", [], [])).toBe(false);
  });
});

describe("BlogWriteConfig 전송 배선", () => {
  it("제목·계정·발행설정을 담아 api.blogWrite.send를 호출한다", async () => {
    const user = userEvent.setup();
    render(
      <MantineProvider>
        <BlogWriteConfig
          device={{ id: "d1", name: "하위-001" }}
          accounts={["acc_a", "acc_b"]}
        />
      </MantineProvider>,
    );

    await user.type(screen.getByLabelText("블로그 글 제목"), "새 글 제목");
    await user.click(screen.getByLabelText("acc_a"));

    const btn = screen.getByRole("button", {
      name: "블로그 새 글 발행 명령 전송",
    });
    expect(btn).toBeEnabled();
    await user.click(btn);

    await waitFor(() => expect(send).toHaveBeenCalledTimes(1));
    const arg = send.mock.calls[0]![0] as Record<string, unknown>;
    expect(arg.deviceId).toBe("d1");
    expect(arg.title).toBe("새 글 제목");
    expect(arg.targets).toEqual([{ loginId: "acc_a", blogId: "acc_a" }]);
    expect(arg.settings).toMatchObject({
      openType: 0,
      commentYn: true,
      searchYn: true,
    });
  });
});
