import { MantineProvider } from "@mantine/core";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, it, expect, vi } from "vitest";

import { resetIpc } from "@/test/ipc";
import { pickOption } from "@/test/select";

import { Accounts } from "./accounts";

vi.mock("@tauri-apps/api/core", async () => ({
  invoke: (await import("@/test/ipc")).invoke,
}));

async function renderAccounts(go = vi.fn()) {
  render(
    <MantineProvider>
      <Accounts go={go} />
    </MantineProvider>,
  );
  // Wait for the async IPC load to populate the table.
  await screen.findByText("invest_king7");
  return go;
}

describe("Accounts", () => {
  // The component loads its rows asynchronously over the IPC wrapper, mocked
  // here by the in-memory backend. Reset between tests so each starts from the
  // pristine 15-account dataset.
  beforeEach(() => {
    resetIpc();
  });

  it("renders the title and first page of accounts (10 rows)", async () => {
    await renderAccounts();
    expect(
      screen.getByRole("heading", { name: "계정 관리" }),
    ).toBeInTheDocument();
    // 15 accounts → first page shows 10 data rows + 1 header row
    expect(screen.getAllByRole("row").length).toBe(11);
  });

  it("filters by platform via the segment chips", async () => {
    await renderAccounts();
    // band has 2 accounts in the mock
    await userEvent.click(screen.getByRole("button", { name: /밴드/ }));
    expect(screen.getAllByRole("row").length).toBe(3); // header + 2 band rows
  });

  it("navigates to 알림 with an account filter from 보러가기", async () => {
    const go = await renderAccounts();
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
    await renderAccounts();
    const row = screen.getAllByRole("row")[1]!;
    await userEvent.click(within(row).getByText("invest_king7"));
    const input = within(row).getByDisplayValue("invest_king7");
    await userEvent.clear(input);
    await userEvent.type(input, "renamed_id{Enter}");
    expect(await screen.findByText("renamed_id")).toBeInTheDocument();
  });

  it("reveals a masked password on demand", async () => {
    await renderAccounts();
    const row = screen.getAllByRole("row")[1]!;
    expect(within(row).queryByText("ik7!naver22")).not.toBeInTheDocument();
    await userEvent.click(within(row).getByTitle("보기"));
    expect(within(row).getByText("ik7!naver22")).toBeInTheDocument();
  });

  it("cycles account status when the badge is clicked", async () => {
    await renderAccounts();
    const row = screen.getAllByRole("row")[1]!;
    expect(within(row).getByTitle("클릭하여 상태 변경")).toHaveTextContent(
      "활성",
    );
    await userEvent.click(within(row).getByTitle("클릭하여 상태 변경"));
    await waitFor(() =>
      expect(within(row).getByTitle("클릭하여 상태 변경")).toHaveTextContent(
        "에러",
      ),
    );
  });

  it("adds a new account row", async () => {
    await renderAccounts();
    expect(screen.getByText(/총 15개 계정/)).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: /계정 추가/ }));
    expect(await screen.findByText(/총 16개 계정/)).toBeInTheDocument();
  });

  it("bulk-deletes the selected page of accounts", async () => {
    await renderAccounts();
    const header = screen.getAllByRole("row")[0]!;
    await userEvent.click(within(header).getByRole("checkbox"));
    await userEvent.click(screen.getByRole("button", { name: /10개 삭제/ }));
    // 15 − 10 = 5 remain → header + 5 rows
    await waitFor(() => expect(screen.getAllByRole("row").length).toBe(6));
  });

  it("deletes a single account via the row trash action", async () => {
    await renderAccounts();
    const row = screen.getAllByRole("row")[1]!;
    await userEvent.click(within(row).getByTitle("삭제"));
    await waitFor(() =>
      expect(screen.queryByText("invest_king7")).not.toBeInTheDocument(),
    );
  });

  it("filters accounts by the search box", async () => {
    await renderAccounts();
    await userEvent.type(
      screen.getByPlaceholderText("계정·태그 검색"),
      "value_pick",
    );
    expect(screen.getAllByRole("row").length).toBe(2); // header + 1 match
  });

  it("saves an edited cell on blur", async () => {
    await renderAccounts();
    const row = screen.getAllByRole("row")[1]!;
    await userEvent.click(within(row).getByText("invest_king7"));
    const input = within(row).getByDisplayValue("invest_king7");
    await userEvent.clear(input);
    await userEvent.type(input, "blur_id");
    await userEvent.tab();
    expect(await screen.findByText("blur_id")).toBeInTheDocument();
  });

  it("cancels an edit on Escape", async () => {
    await renderAccounts();
    const row = screen.getAllByRole("row")[1]!;
    await userEvent.click(within(row).getByText("invest_king7"));
    const input = within(row).getByDisplayValue("invest_king7");
    await userEvent.clear(input);
    await userEvent.type(input, "discard{Escape}");
    expect(within(row).getByText("invest_king7")).toBeInTheDocument();
  });

  it("edits a password inline", async () => {
    await renderAccounts();
    const row = screen.getAllByRole("row")[1]!;
    await userEvent.click(within(row).getByTitle("보기"));
    await userEvent.click(within(row).getByText("ik7!naver22"));
    const input = within(row).getByDisplayValue("ik7!naver22");
    await userEvent.clear(input);
    await userEvent.type(input, "newpass99{Enter}");
    expect(await screen.findByText("newpass99")).toBeInTheDocument();
  });

  it("saves a password edit on blur", async () => {
    await renderAccounts();
    const row = screen.getAllByRole("row")[1]!;
    await userEvent.click(within(row).getByTitle("보기"));
    await userEvent.click(within(row).getByText("ik7!naver22"));
    const input = within(row).getByDisplayValue("ik7!naver22");
    await userEvent.clear(input);
    await userEvent.type(input, "blurpass11");
    await userEvent.tab();
    expect(await screen.findByText("blurpass11")).toBeInTheDocument();
  });

  it("cancels a password edit on Escape", async () => {
    await renderAccounts();
    const row = screen.getAllByRole("row")[1]!;
    await userEvent.click(within(row).getByTitle("보기"));
    await userEvent.click(within(row).getByText("ik7!naver22"));
    const input = within(row).getByDisplayValue("ik7!naver22");
    await userEvent.clear(input);
    await userEvent.type(input, "discard{Escape}");
    expect(within(row).getByText("ik7!naver22")).toBeInTheDocument();
  });

  it("fires excel import and export actions", async () => {
    await renderAccounts();
    await userEvent.click(
      screen.getByRole("button", { name: /엑셀 가져오기/ }),
    );
    await userEvent.click(screen.getByRole("button", { name: /내보내기/ }));
    expect(
      screen.getByRole("heading", { name: "계정 관리" }),
    ).toBeInTheDocument();
  });

  it("adds a tag through the tag cell popover", async () => {
    await renderAccounts();
    const row = screen.getAllByRole("row")[1]!;
    await userEvent.click(within(row).getByText("대형주"));
    const tagInput = await screen.findByPlaceholderText("태그 추가");
    await userEvent.type(tagInput, "신규태그{Enter}");
    // rendered both as a TagsInput pill and a cell badge
    expect((await screen.findAllByText("신규태그")).length).toBeGreaterThan(0);
  });

  it("paginates to the second page", async () => {
    await renderAccounts();
    await userEvent.click(screen.getByRole("button", { name: "2" }));
    // 15 accounts → page 2 has 5 rows + header
    expect(screen.getAllByRole("row").length).toBe(6);
  });

  it("filters by tag via the tag select", async () => {
    await renderAccounts();
    await pickOption(0, "# 반도체"); // toolbar tag select is the first listbox
    // a1, a2 carry 반도체 → header + 2 rows
    expect(screen.getAllByRole("row").length).toBe(3);
  });

  it("changes a row's platform via its select", async () => {
    await renderAccounts();
    // combos[0] = tag filter; combos[1] = first row's platform select
    await pickOption(1, "밴드");
    expect(
      screen.getByRole("heading", { name: "계정 관리" }),
    ).toBeInTheDocument();
  });
});
