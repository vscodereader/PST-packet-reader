import { MantineProvider } from "@mantine/core";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, it, expect, vi } from "vitest";

import { WriterModal } from "./writer-modal";

function renderWriter(over: Partial<Parameters<typeof WriterModal>[0]> = {}) {
  const onSave = vi.fn();
  render(
    <MantineProvider>
      <WriterModal
        open
        doc={null}
        drafts={[]}
        onClose={vi.fn()}
        onSave={onSave}
        onSaveDraft={vi.fn()}
        onDeleteDraft={vi.fn()}
        {...over}
      />
    </MantineProvider>,
  );
  return { onSave };
}

describe("WriterModal", () => {
  it("renders the mode pills and a title field", async () => {
    renderWriter();
    expect(
      await screen.findByPlaceholderText("제목을 입력하세요"),
    ).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "글 작성" })).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "댓글 작성" }),
    ).toBeInTheDocument();
  });

  it("saves with the entered title via 저장", async () => {
    const { onSave } = renderWriter();
    await userEvent.type(
      await screen.findByPlaceholderText("제목을 입력하세요"),
      "테스트 글",
    );
    await userEvent.click(screen.getByRole("button", { name: "저장" }));
    expect(onSave).toHaveBeenCalledTimes(1);
    expect(onSave.mock.calls[0]![0]).toMatchObject({
      title: "테스트 글",
      kind: "post",
      status: "ready",
    });
  });

  it("shows the comment composer in 댓글 작성 mode", async () => {
    renderWriter();
    await userEvent.click(screen.getByRole("button", { name: "댓글 작성" }));
    expect(await screen.findByText("댓글 대상")).toBeInTheDocument();
    expect(screen.getByText("댓글 내용")).toBeInTheDocument();
  });
});
