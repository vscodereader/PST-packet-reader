import { MantineProvider } from "@mantine/core";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import {
  StockSelectModalView,
  type StockSelectAdapter,
  type StockSelectPage,
  type StockSelectRow,
} from "./stock-select-modal-view";

// 표현 컴포넌트는 데이터 어댑터를 prop으로 받으므로, 스텁 어댑터를 직접 주입해 뷰 단독으로 검증한다
// (데스크톱=ipc, Admin=api 래퍼는 각자 테스트에서 어댑터 배선을 검증).

const row = (over: Partial<StockSelectRow> = {}): StockSelectRow => ({
  code: "000660",
  name: "SK하이닉스",
  isHotDiscussion: true,
  price: "1,911,000",
  changeType: "falling",
  changeRate: "-7.68",
  ...over,
});

const pageOf = (
  stocks: StockSelectRow[],
  hasNext = false,
): StockSelectPage => ({
  stocks,
  hasNext,
});

function makeAdapter(
  over: Partial<StockSelectAdapter> = {},
): StockSelectAdapter {
  return {
    list: vi.fn().mockResolvedValue(pageOf([row()])),
    search: vi.fn().mockResolvedValue(pageOf([])),
    recentPostedCodes: vi.fn().mockResolvedValue(new Set<string>()),
    ...over,
  };
}

function renderView(
  over: Partial<Parameters<typeof StockSelectModalView>[0]> = {},
) {
  const onConfirm = vi.fn();
  const adapter = over.adapter ?? makeAdapter();
  render(
    <MantineProvider>
      <StockSelectModalView
        open
        preselected={over.preselected ?? []}
        onClose={vi.fn()}
        onConfirm={onConfirm}
        adapter={adapter}
      />
    </MantineProvider>,
  );
  return { onConfirm, adapter };
}

describe("StockSelectModalView", () => {
  it("닫혀 있으면 아무것도 렌더하지 않는다", () => {
    const { container } = render(
      <MantineProvider>
        <StockSelectModalView
          open={false}
          preselected={[]}
          onClose={vi.fn()}
          onConfirm={vi.fn()}
          adapter={makeAdapter()}
        />
      </MantineProvider>,
    );
    expect(container).not.toHaveTextContent("종목 선택");
  });

  it("진입 시 어댑터 list를 기본(거래대금·krx·전체·1p)으로 호출하고 행을 렌더한다", async () => {
    const { adapter } = renderView();
    await waitFor(() =>
      expect(adapter.list).toHaveBeenCalledWith(
        "tradingValue",
        "krx",
        "all",
        1,
      ),
    );
    expect(await screen.findByText("SK하이닉스")).toBeInTheDocument();
  });

  it("검색어를 입력하면 list 대신 search로 전환한다", async () => {
    const adapter = makeAdapter({
      search: vi
        .fn()
        .mockResolvedValue(
          pageOf([row({ code: "069500", name: "KODEX 200" })]),
        ),
    });
    renderView({ adapter });
    await waitFor(() => expect(adapter.list).toHaveBeenCalled());
    fireEvent.change(screen.getByPlaceholderText("종목명 또는 코드 검색"), {
      target: { value: "ko" },
    });
    await waitFor(() => expect(adapter.search).toHaveBeenCalledWith("ko", 1));
    expect(await screen.findByText("KODEX 200")).toBeInTheDocument();
  });

  it("preselected는 선택 상태로 시드되어 적용 버튼이 활성화된다", async () => {
    renderView({ adapter: makeAdapter(), preselected: ["000660"] });
    await waitFor(() =>
      expect(screen.getByRole("button", { name: /적용 \(1\)/ })).toBeEnabled(),
    );
  });

  it("더보기를 누르면 다음 페이지를 이어붙인다", async () => {
    const adapter = makeAdapter({
      list: vi
        .fn()
        .mockResolvedValueOnce(pageOf([row()], true))
        .mockResolvedValueOnce(
          pageOf([row({ code: "005930", name: "삼성전자" })]),
        ),
    });
    renderView({ adapter });
    await screen.findByText("SK하이닉스");
    fireEvent.click(screen.getByRole("button", { name: "더보기" }));
    expect(await screen.findByText("삼성전자")).toBeInTheDocument();
    expect(screen.getByText("SK하이닉스")).toBeInTheDocument();
  });

  it("행을 선택해 적용하면 code·name만 반환한다", async () => {
    const { onConfirm } = renderView();
    fireEvent.click(await screen.findByText("SK하이닉스"));
    fireEvent.click(screen.getByRole("button", { name: /적용/ }));
    expect(onConfirm).toHaveBeenCalledWith([
      { code: "000660", name: "SK하이닉스" },
    ]);
  });

  it("최근 1시간 집합에 든 종목은 코드/이름 매칭으로 '1시간' 마커가 뜬다", async () => {
    const adapter = makeAdapter({
      recentPostedCodes: vi.fn().mockResolvedValue(new Set(["000660"])),
    });
    renderView({ adapter });
    await screen.findByText("SK하이닉스");
    await waitFor(() =>
      expect(screen.getByText("1시간 : 1개")).toBeInTheDocument(),
    );
    expect(screen.getByText("1시간", { exact: true })).toBeInTheDocument();
  });
});
