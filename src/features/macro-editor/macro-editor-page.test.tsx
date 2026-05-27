import { MantineProvider } from "@mantine/core";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { MacroEditorPage } from "./macro-editor-page";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

function renderMacroEditor() {
  render(
    <MantineProvider>
      <MacroEditorPage />
    </MantineProvider>,
  );
}

describe("MacroEditorPage", () => {
  beforeEach(() => {
    localStorage.clear();
  });

  it("saves, selects, edits, and deletes titles", async () => {
    const user = userEvent.setup();
    renderMacroEditor();

    await user.type(screen.getByLabelText("제목 작성"), "A");
    await user.click(screen.getAllByRole("button", { name: "저장" })[0]!);
    await user.type(screen.getByLabelText("제목 작성"), "C");
    await user.click(screen.getAllByRole("button", { name: "저장" })[0]!);

    await user.click(screen.getByLabelText("제목을 선택하세요 열기"));
    await user.click(screen.getByRole("button", { name: "A" }));
    await user.click(screen.getByRole("button", { name: "C" }));

    expect(screen.getByLabelText("제목을 선택하세요 열기")).toBeChecked();
    expect(screen.getByLabelText("A")).toBeInTheDocument();
    expect(screen.getByLabelText("C")).toBeInTheDocument();

    await user.click(screen.getByLabelText("A"));
    await user.click(screen.getAllByRole("button", { name: "편집" })[0]!);
    await user.clear(screen.getByLabelText("선택된 제목 편집"));
    await user.type(screen.getByLabelText("선택된 제목 편집"), "A edited");
    await user.click(screen.getAllByRole("button", { name: "저장" })[2]!);

    expect(screen.getByLabelText("A edited")).toBeInTheDocument();

    await user.click(screen.getAllByRole("button", { name: "삭제" })[0]!);

    expect(screen.queryByLabelText("A edited")).not.toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "A edited" }),
    ).not.toBeInTheDocument();
  });

  it("saves and edits content separately from titles", async () => {
    const user = userEvent.setup();
    renderMacroEditor();

    await user.type(screen.getByLabelText("내용 작성"), "내용 1");
    await user.click(screen.getAllByRole("button", { name: "저장" })[1]!);
    await user.click(screen.getByLabelText("내용을 선택하세요 열기"));
    await user.click(screen.getByRole("button", { name: "내용 1" }));
    await user.click(screen.getByLabelText("내용 1"));
    await user.click(screen.getAllByRole("button", { name: "편집" })[1]!);
    await user.type(screen.getByLabelText("선택된 내용 편집"), " 추가");
    await user.click(screen.getAllByRole("button", { name: "저장" })[3]!);

    expect(screen.getByLabelText("내용 1 추가")).toBeInTheDocument();
  });
});
