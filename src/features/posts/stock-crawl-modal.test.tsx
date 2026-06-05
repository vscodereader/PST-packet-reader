import { MantineProvider } from "@mantine/core";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, it, expect, vi } from "vitest";

import { StockCrawlModal } from "./stock-crawl-modal";

vi.mock("@tauri-apps/api/core", async () => ({
  invoke: (await import("@/test/ipc")).invoke,
}));

function renderModal(
  over: Partial<Parameters<typeof StockCrawlModal>[0]> = {},
) {
  const onConfirm = vi.fn();
  render(
    <MantineProvider>
      <StockCrawlModal
        open
        preselected={["005930"]}
        onClose={vi.fn()}
        onConfirm={onConfirm}
        {...over}
      />
    </MantineProvider>,
  );
  return { onConfirm };
}

describe("StockCrawlModal", () => {
  it("shows the search source banner while open", async () => {
    renderModal();
    expect(await screen.findByText(/finance\.naver\.com/)).toBeInTheDocument();
  });

  it("confirms the preselected stocks", async () => {
    const { onConfirm } = renderModal();
    // Wait for the initial (empty-query) search to populate so confirm can
    // resolve the preselected code's name from the live results.
    await screen.findByText("삼성전자", undefined, { timeout: 2500 });
    await userEvent.click(await screen.findByRole("button", { name: /적용/ }));
    expect(onConfirm).toHaveBeenCalledTimes(1);
    expect(onConfirm.mock.calls[0]![0]).toEqual([
      expect.objectContaining({ code: "005930", name: "삼성전자" }),
    ]);
  });

  it("filters results by a live search query", async () => {
    renderModal({ preselected: [] });
    const search = await screen.findByPlaceholderText("종목명 또는 코드 검색");
    await userEvent.type(search, "카카오");
    expect(await screen.findByText("카카오")).toBeInTheDocument();
    expect(screen.queryByText("삼성전자")).not.toBeInTheDocument();
  });

  it("deselects a stock when clicked twice", async () => {
    renderModal({ preselected: [] });
    const search = await screen.findByPlaceholderText("종목명 또는 코드 검색");
    await userEvent.type(search, "카카오");
    const row = await screen.findByText("카카오");
    await userEvent.click(row); // select
    await userEvent.click(row); // deselect
    expect(screen.getByRole("button", { name: /적용/ })).toBeDisabled();
  });

  it("toggles a stock from the search results and confirms it", async () => {
    const { onConfirm } = renderModal({ preselected: [] });
    const search = await screen.findByPlaceholderText("종목명 또는 코드 검색");
    await userEvent.type(search, "카카오");
    await userEvent.click(await screen.findByText("카카오"));
    await userEvent.click(screen.getByRole("button", { name: /적용/ }));
    expect(onConfirm.mock.calls[0]![0]).toEqual([
      expect.objectContaining({ code: "035720", name: "카카오" }),
    ]);
  });
});
