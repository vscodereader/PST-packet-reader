import { MantineProvider } from "@mantine/core";
import { invoke } from "@tauri-apps/api/core";
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
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "search_stocks") {
        return Promise.resolve([
          {
            code: "005930",
            link: "https://stock.naver.com/domestic/stock/005930/discussion?chip=all",
            name: "삼성전자",
          },
        ]);
      }

      if (command === "parse_template_csv") {
        return Promise.resolve({
          bodies: ["내용1"],
          comments: ["댓글1"],
          titles: ["제목1", "제목2"],
        });
      }

      return Promise.resolve({ completed: 1, reports: [] });
    });
  });

  it("renders the batch UI without the legacy picker panels", () => {
    renderMacroEditor();

    expect(screen.getByText("패킷 기반 종목/글/댓글 실행 설정")).toBeInTheDocument();
    expect(screen.queryByText("토론방 자동 입력")).not.toBeInTheDocument();
    expect(screen.queryByText("제목을 선택하세요")).not.toBeInTheDocument();
  });

  it("imports CSV values into editable textareas", async () => {
    const user = userEvent.setup();
    renderMacroEditor();

    const file = new File(["제목,내용,댓글내용\n제목1,내용1,댓글1"], "template.csv", {
      type: "text/csv",
    });
    const fileInput = document.querySelector<HTMLInputElement>('input[type="file"]');

    expect(fileInput).not.toBeNull();

    await user.upload(fileInput!, file);

    expect(
      await screen.findByText("template.csv에서 제목 2개, 내용 1개, 댓글내용 1개를 가져왔습니다."),
    ).toBeInTheDocument();
    expect(screen.getByText("template.csv")).toBeInTheDocument();
  });
});
