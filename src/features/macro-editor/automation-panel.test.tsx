import { MantineProvider } from "@mantine/core";
import { invoke } from "@tauri-apps/api/core";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { AutomationPanel } from "./automation-panel";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

function renderAutomationPanel() {
  render(
    <MantineProvider>
      <AutomationPanel
        contentDraft="본문"
        selectedContents={[]}
        selectedTitles={[]}
        titleDraft="제목"
      />
    </MantineProvider>,
  );
}

describe("AutomationPanel", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset();
  });

  it("passes the draft title and body to the Rust automation command", async () => {
    vi.mocked(invoke).mockResolvedValue({
      current_url: "https://stock.naver.com/domestic/stock/000000/discussion",
      login_profile: {
        image_url: null,
        logged_in: true,
        message: "Success",
        nickname: "할수있다",
      },
      register_button_highlighted: true,
      selected: {
        category: "상승",
        item_text: "3 테스트 +1.2%",
        method: "view-all-button-fast",
        rank: "3",
      },
    });

    const user = userEvent.setup();
    renderAutomationPanel();

    await user.click(screen.getByRole("button", { name: "자동 입력 실행" }));

    expect(invoke).toHaveBeenCalledWith("run_naver_discussion", {
      body: "본문",
      host: "127.0.0.1",
      port: 9222,
      title: "제목",
    });
    expect(await screen.findByText(/선택 카테고리: 상승/)).toBeInTheDocument();
    expect(screen.getByText(/로그인 확인: 할수있다/)).toBeInTheDocument();
  });
});
