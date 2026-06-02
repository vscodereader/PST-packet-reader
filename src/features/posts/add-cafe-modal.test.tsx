import { MantineProvider } from "@mantine/core";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

import type { Account } from "@/shared/data/types";
import { resetIpc } from "@/test/ipc";

import { AddCafeModal } from "./add-cafe-modal";

vi.mock("@tauri-apps/api/core", async () => ({
  invoke: (await import("@/test/ipc")).invoke,
}));

const naverAccount: Account = {
  id: "n1",
  platform: "naver",
  loginId: "naver_user",
  pw: "pw",
  status: "active",
  last: "방금 전",
  tags: [],
};

function renderModal(over: Partial<Parameters<typeof AddCafeModal>[0]> = {}) {
  const onAdded = vi.fn();
  const onClose = vi.fn();
  render(
    <MantineProvider>
      <AddCafeModal
        open
        accounts={[naverAccount]}
        onClose={onClose}
        onAdded={onAdded}
        {...over}
      />
    </MantineProvider>,
  );
  return { onAdded, onClose };
}

describe("AddCafeModal", () => {
  beforeEach(() => resetIpc());

  it("resolves a cafe reference and previews its boards", async () => {
    renderModal();
    await userEvent.type(
      screen.getByPlaceholderText(/cafe\.naver\.com/),
      "cafe.naver.com/x",
    );
    await userEvent.click(screen.getByRole("button", { name: "조회" }));
    expect(
      await screen.findByText("해석된 카페 (cafe.naver.com/x)"),
    ).toBeInTheDocument();
    expect(screen.getByText("글쓰기 가능 게시판 2개")).toBeInTheDocument();
  });

  it("saves the resolved cafe and reports it upward", async () => {
    const { onAdded, onClose } = renderModal();
    await userEvent.type(
      screen.getByPlaceholderText(/cafe\.naver\.com/),
      "cafe.naver.com/x",
    );
    await userEvent.click(screen.getByRole("button", { name: "조회" }));
    await screen.findByText("해석된 카페 (cafe.naver.com/x)");
    await userEvent.click(screen.getByRole("button", { name: "저장" }));
    expect(onAdded).toHaveBeenCalledWith(
      expect.objectContaining({
        cafeId: 31732304,
        cafeRef: "cafe.naver.com/x",
      }),
    );
    expect(onClose).toHaveBeenCalled();
  });

  it("warns when there is no naver account to resolve with", () => {
    renderModal({ accounts: [] });
    expect(screen.getByText(/네이버 계정을 먼저/)).toBeInTheDocument();
  });
});
