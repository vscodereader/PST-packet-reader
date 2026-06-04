import { MantineProvider } from "@mantine/core";
import { render, screen, act } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, it, expect, vi } from "vitest";

import { ipc } from "@/shared/ipc";

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
  it("logs an activity when crawl completes", async () => {
    vi.useFakeTimers();
    const spy = vi.spyOn(ipc.activity, "append");
    render(
      <MantineProvider>
        <StockCrawlModal
          open
          preselected={[]}
          onClose={vi.fn()}
          onConfirm={vi.fn()}
        />
      </MantineProvider>,
    );
    // Advance past the 1400ms crawl timeout.
    await act(async () => {
      vi.advanceTimersByTime(1500);
    });
    expect(spy).toHaveBeenCalledWith("info", expect.stringContaining("종목"));
    spy.mockRestore();
    vi.useRealTimers();
  });

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

  it("deselects a stock when clicked twice", async () => {
    renderModal({ preselected: [] });
    const search = await screen.findByPlaceholderText(
      "종목명 또는 코드 검색",
      undefined,
      { timeout: 2500 },
    );
    await userEvent.type(search, "카카오");
    const row = screen.getByText("카카오");
    await userEvent.click(row); // select
    await userEvent.click(row); // deselect
    expect(screen.getByRole("button", { name: /적용/ })).toBeDisabled();
  });

  it("toggles a stock from the list and confirms it", async () => {
    const { onConfirm } = renderModal({ preselected: [] });
    const search = await screen.findByPlaceholderText(
      "종목명 또는 코드 검색",
      undefined,
      { timeout: 2500 },
    );
    await userEvent.type(search, "카카오");
    await userEvent.click(screen.getByText("카카오"));
    await userEvent.click(screen.getByRole("button", { name: /적용/ }));
    expect(onConfirm.mock.calls[0]![0]).toEqual([
      expect.objectContaining({ name: "카카오" }),
    ]);
  });
});
