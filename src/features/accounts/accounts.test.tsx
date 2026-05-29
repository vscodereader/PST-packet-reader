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
});
