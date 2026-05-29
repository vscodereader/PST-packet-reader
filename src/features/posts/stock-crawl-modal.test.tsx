import { MantineProvider } from "@mantine/core";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, it, expect, vi } from "vitest";

import { StockCrawlModal } from "./stock-crawl-modal";

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
  it("shows the crawl source banner while open", async () => {
    renderModal();
    expect(await screen.findByText(/finance\.naver\.com/)).toBeInTheDocument();
  });

  it("confirms the preselected stocks", async () => {
    const { onConfirm } = renderModal();
    await userEvent.click(await screen.findByRole("button", { name: /적용/ }));
    expect(onConfirm).toHaveBeenCalledTimes(1);
    expect(onConfirm.mock.calls[0]![0]).toEqual([
      expect.objectContaining({ code: "005930", name: "삼성전자" }),
    ]);
  });

  it("filters the stock list by query once the crawl finishes", async () => {
    renderModal();
    const search = await screen.findByPlaceholderText(
      "종목명 또는 코드 검색",
      undefined,
      { timeout: 2500 },
    );
    await userEvent.type(search, "카카오");
    expect(screen.getByText("카카오")).toBeInTheDocument();
    expect(screen.queryByText("삼성전자")).not.toBeInTheDocument();
  });

  it("returns to the crawling state on 다시 크롤링", async () => {
    renderModal();
    await userEvent.click(screen.getByRole("button", { name: /다시 크롤링/ }));
    expect(screen.getByText(/수집하는 중/)).toBeInTheDocument();
  });
});
