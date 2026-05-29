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
});
