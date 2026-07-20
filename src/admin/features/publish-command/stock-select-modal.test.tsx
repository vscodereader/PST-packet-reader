import { MantineProvider } from "@mantine/core";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const listMock = vi.hoisted(() => vi.fn());
const searchMock = vi.hoisted(() => vi.fn());
const postReportsMock = vi.hoisted(() => vi.fn());

vi.mock("../../api", () => ({
  api: {
    forumStocks: {
      list: (...a: unknown[]) => listMock(...a),
      search: (...a: unknown[]) => searchMock(...a),
    },
    postReports: { list: () => postReportsMock() },
  },
}));

import { StockSelectModal } from "./stock-select-modal";

interface Row {
  code: string;
  name: string;
  exchange: string;
  price: string;
  changeRate: string;
  changeType: string;
  isHotDiscussion: boolean;
}

const page = (stocks: Row[], hasNext = false) => ({
  stocks,
  totalCount: stocks.length,
  page: 1,
  hasNext,
});

const SK: Row = {
  code: "000660",
  name: "SK하이닉스",
  exchange: "KOSPI",
  price: "1,911,000",
  changeRate: "-7.68",
  changeType: "falling",
  isHotDiscussion: true,
};

function renderModal(
  over: Partial<Parameters<typeof StockSelectModal>[0]> = {},
) {
  const onConfirm = vi.fn();
  render(
    <MantineProvider>
      <StockSelectModal
        open
        deviceId="d1"
        preselected={[]}
        onClose={vi.fn()}
        onConfirm={onConfirm}
        {...over}
      />
    </MantineProvider>,
  );
  return { onConfirm };
}

describe("Admin StockSelectModal", () => {
  beforeEach(() => {
    listMock.mockReset();
    searchMock.mockReset();
    postReportsMock.mockReset();
    listMock.mockResolvedValue(page([SK]));
    searchMock.mockResolvedValue(page([]));
    postReportsMock.mockResolvedValue([]);
  });

  it("닫혀 있으면 아무것도 렌더하지 않는다", () => {
    const { container } = render(
      <MantineProvider>
        <StockSelectModal
          open={false}
          deviceId="d1"
          preselected={[]}
          onClose={vi.fn()}
          onConfirm={vi.fn()}
        />
      </MantineProvider>,
    );
    expect(container).not.toHaveTextContent("종목 선택");
  });

  it("기본 진입 시 거래대금 카테고리를 KRX로 api.forumStocks.list 호출한다", async () => {
    renderModal();
    await waitFor(() =>
      expect(listMock).toHaveBeenCalledWith({
        category: "tradingValue",
        exchange: "krx",
        market: "all",
        page: 1,
      }),
    );
    expect(await screen.findByText("SK하이닉스")).toBeInTheDocument();
  });

  it("검색어 입력 시 api.forumStocks.search로 전환한다", async () => {
    searchMock.mockResolvedValue(
      page([
        {
          code: "069500",
          name: "KODEX 200",
          exchange: "코스피",
          price: "",
          changeRate: "",
          changeType: "even",
          isHotDiscussion: false,
        },
      ]),
    );
    renderModal();
    await waitFor(() => expect(listMock).toHaveBeenCalled());
    fireEvent.change(screen.getByPlaceholderText("종목명 또는 코드 검색"), {
      target: { value: "ko" },
    });
    await waitFor(() => expect(searchMock).toHaveBeenCalledWith("ko", 1));
    expect(await screen.findByText("KODEX 200")).toBeInTheDocument();
  });

  it("행을 선택해 적용하면 code·name만 반환한다(link 없음)", async () => {
    const { onConfirm } = renderModal();
    fireEvent.click(await screen.findByText("SK하이닉스"));
    await waitFor(() =>
      expect(screen.getByRole("button", { name: /적용 \(1\)/ })).toBeEnabled(),
    );
    fireEvent.click(screen.getByRole("button", { name: /적용/ }));
    expect(onConfirm).toHaveBeenCalledTimes(1);
    expect(onConfirm.mock.calls[0]![0]).toEqual([
      { code: "000660", name: "SK하이닉스" },
    ]);
  });

  it("이 하위의 1시간 내 게시성공 종목명은 '1시간' 마커로 표시된다(deviceId 필터)", async () => {
    postReportsMock.mockResolvedValue([
      {
        deviceId: "d1",
        at: Date.now(),
        items: [
          { platform: "forum", status: "success", target: "SK하이닉스" },
          { platform: "forum", status: "success", target: "다른회사" },
        ],
      },
      {
        // 다른 하위 → 무시.
        deviceId: "d2",
        at: Date.now(),
        items: [{ platform: "forum", status: "success", target: "무시종목" }],
      },
    ]);
    renderModal();
    await screen.findByText("SK하이닉스");
    // 이 하위(d1)의 게시성공 2종목만 집계 → "1시간 : 2개".
    await waitFor(() =>
      expect(screen.getByText("1시간 : 2개")).toBeInTheDocument(),
    );
    // SK하이닉스 행에 "1시간" 마커가 뜬다(이름 매칭).
    expect(screen.getByText("1시간", { exact: true })).toBeInTheDocument();
  });
});
