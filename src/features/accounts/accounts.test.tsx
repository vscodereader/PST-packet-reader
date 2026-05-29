import { MantineProvider } from "@mantine/core";
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, it, expect, vi } from "vitest";

import { Accounts } from "./accounts";

function renderAccounts(go = vi.fn()) {
  render(
    <MantineProvider>
      <Accounts go={go} />
    </MantineProvider>,
  );
  return go;
}

describe("Accounts", () => {
  it("renders the title and first page of accounts (10 rows)", () => {
    renderAccounts();
    expect(
      screen.getByRole("heading", { name: "계정 관리" }),
    ).toBeInTheDocument();
    // 15 accounts → first page shows 10 data rows + 1 header row
    const rows = screen.getAllByRole("row");
    expect(rows.length).toBe(11);
  });

  it("filters by platform via the segment chips", async () => {
    renderAccounts();
    // band has 2 accounts in the mock
    await userEvent.click(screen.getByRole("button", { name: /밴드/ }));
    const rows = screen.getAllByRole("row");
    expect(rows.length).toBe(3); // header + 2 band rows
  });

  it("navigates to 알림 with an account filter from 보러가기", async () => {
    const go = renderAccounts();
    const firstBody = screen.getAllByRole("row")[1];
    await userEvent.click(
      within(firstBody!).getByRole("button", { name: /보러가기/ }),
    );
    expect(go).toHaveBeenCalledWith(
      "log",
      expect.objectContaining({
        logFilter: expect.objectContaining({ platform: expect.any(String) }),
      }),
    );
  });

  it("edits a login id inline", async () => {
    renderAccounts();
    const row = screen.getAllByRole("row")[1]!;
    await userEvent.click(within(row).getByText("invest_king7"));
    const input = within(row).getByDisplayValue("invest_king7");
    await userEvent.clear(input);
    await userEvent.type(input, "renamed_id{Enter}");
    expect(await screen.findByText("renamed_id")).toBeInTheDocument();
  });

  it("reveals a masked password on demand", async () => {
    renderAccounts();
    const row = screen.getAllByRole("row")[1]!;
    expect(within(row).queryByText("ik7!naver22")).not.toBeInTheDocument();
    await userEvent.click(within(row).getByTitle("보기"));
    expect(within(row).getByText("ik7!naver22")).toBeInTheDocument();
  });

  it("cycles account status when the badge is clicked", async () => {
    renderAccounts();
    const row = screen.getAllByRole("row")[1]!;
    expect(within(row).getByTitle("클릭하여 상태 변경")).toHaveTextContent(
      "활성",
    );
    await userEvent.click(within(row).getByTitle("클릭하여 상태 변경"));
    expect(within(row).getByTitle("클릭하여 상태 변경")).toHaveTextContent(
      "에러",
    );
  });

  it("adds a new account row", async () => {
    renderAccounts();
    expect(screen.getByText(/총 15개 계정/)).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: /계정 추가/ }));
    expect(screen.getByText(/총 16개 계정/)).toBeInTheDocument();
  });

  it("bulk-deletes the selected page of accounts", async () => {
    renderAccounts();
    const header = screen.getAllByRole("row")[0]!;
    await userEvent.click(within(header).getByRole("checkbox"));
    await userEvent.click(screen.getByRole("button", { name: /10개 삭제/ }));
    // 15 − 10 = 5 remain → header + 5 rows
    expect(screen.getAllByRole("row").length).toBe(6);
  });

  it("deletes a single account via the row trash action", async () => {
    renderAccounts();
    const row = screen.getAllByRole("row")[1]!;
    await userEvent.click(within(row).getByTitle("삭제"));
    expect(screen.queryByText("invest_king7")).not.toBeInTheDocument();
  });

  it("filters accounts by the search box", async () => {
    renderAccounts();
    await userEvent.type(
      screen.getByPlaceholderText("계정·태그 검색"),
      "value_pick",
    );
    expect(screen.getAllByRole("row").length).toBe(2); // header + 1 match
  });
});
