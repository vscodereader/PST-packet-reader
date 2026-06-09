import { MantineProvider } from "@mantine/core";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { StockCrawlModal } from "./stock-crawl-modal";

const listMock = vi.fn();
const searchMock = vi.fn();
vi.mock("@/shared/ipc", () => ({
  ipc: {
    forumStocks: {
      list: (...a: unknown[]) => listMock(...a),
      search: (...a: unknown[]) => searchMock(...a),
    },
  },
}));

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
  over: Partial<Parameters<typeof StockCrawlModal>[0]> = {},
) {
  const onConfirm = vi.fn();
  render(
    <MantineProvider>
      <StockCrawlModal
        open
        preselected={[]}
        onClose={vi.fn()}
        onConfirm={onConfirm}
        {...over}
      />
    </MantineProvider>,
  );
  return { onConfirm };
}

describe("StockCrawlModal", () => {
  beforeEach(() => {
    listMock.mockReset();
    searchMock.mockReset();
    listMock.mockResolvedValue(page([SK]));
    searchMock.mockResolvedValue(page([]));
  });

  it("returns null while closed", () => {
    const { container } = render(
      <MantineProvider>
        <StockCrawlModal
          open={false}
          preselected={[]}
          onClose={vi.fn()}
          onConfirm={vi.fn()}
        />
      </MantineProvider>,
    );
    expect(container).not.toHaveTextContent("종목 선택");
  });

  it("기본 진입 시 거래대금 카테고리를 KRX로 로드한다", async () => {
    renderModal();
    await waitFor(() =>
      expect(listMock).toHaveBeenCalledWith("tradingValue", "krx", 1),
    );
    expect(await screen.findByText("SK하이닉스")).toBeInTheDocument();
  });

  it("카테고리 탭(상승)을 누르면 해당 category로 재호출한다", async () => {
    renderModal();
    await waitFor(() => expect(listMock).toHaveBeenCalled());
    fireEvent.click(screen.getByRole("button", { name: "상승" }));
    await waitFor(() =>
      expect(listMock).toHaveBeenCalledWith("rising", "krx", 1),
    );
  });

  it("거래소 버튼 → 모달에서 NXT 선택 시 nxt로 재호출한다", async () => {
    renderModal();
    await waitFor(() =>
      expect(listMock).toHaveBeenCalledWith("tradingValue", "krx", 1),
    );
    fireEvent.click(screen.getByRole("button", { name: /KRX/ }));
    fireEvent.click(await screen.findByRole("button", { name: "NXT" }));
    await waitFor(() =>
      expect(listMock).toHaveBeenCalledWith("tradingValue", "nxt", 1),
    );
  });

  it("검색어 입력 시 search로 전환하고 결과를 렌더한다", async () => {
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

  it("더보기 클릭 시 다음 페이지를 append한다", async () => {
    listMock.mockReset();
    listMock.mockResolvedValueOnce(page([SK], true)).mockResolvedValueOnce(
      page([
        {
          code: "005930",
          name: "삼성전자",
          exchange: "KOSPI",
          price: "295,500",
          changeRate: "1.18",
          changeType: "rising",
          isHotDiscussion: false,
        },
      ]),
    );
    renderModal();
    expect(await screen.findByText("SK하이닉스")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "더보기" }));
    await waitFor(() =>
      expect(listMock).toHaveBeenCalledWith("tradingValue", "krx", 2),
    );
    expect(await screen.findByText("삼성전자")).toBeInTheDocument();
    expect(screen.getByText("SK하이닉스")).toBeInTheDocument();
  });

  it("행을 클릭해 선택한 뒤 적용하면 코드·이름·링크를 반환한다", async () => {
    const { onConfirm } = renderModal();
    fireEvent.click(await screen.findByText("SK하이닉스"));
    await waitFor(() =>
      expect(screen.getByRole("button", { name: /적용 \(1\)/ })).toBeEnabled(),
    );
    fireEvent.click(screen.getByRole("button", { name: /적용/ }));
    expect(onConfirm).toHaveBeenCalledTimes(1);
    expect(onConfirm.mock.calls[0]![0]).toEqual([
      expect.objectContaining({
        code: "000660",
        name: "SK하이닉스",
        link: expect.stringContaining("000660"),
      }),
    ]);
  });

  it("같은 행을 두 번 클릭하면 선택이 해제된다", async () => {
    renderModal();
    const row = await screen.findByText("SK하이닉스");
    fireEvent.click(row); // select
    await waitFor(() =>
      expect(screen.getByRole("button", { name: /적용/ })).toBeEnabled(),
    );
    fireEvent.click(row); // deselect
    expect(screen.getByRole("button", { name: /적용/ })).toBeDisabled();
  });

  it("preselected 종목은 적용 시 행에서 이름을 찾아 반환한다", async () => {
    const { onConfirm } = renderModal({ preselected: ["000660"] });
    await screen.findByText("SK하이닉스");
    fireEvent.click(await screen.findByRole("button", { name: /적용 \(1\)/ }));
    expect(onConfirm.mock.calls[0]![0]).toEqual([
      expect.objectContaining({ code: "000660", name: "SK하이닉스" }),
    ]);
  });

  it("상승/보합 등락은 각각 색상이 다르게 렌더된다", async () => {
    listMock.mockReset();
    listMock.mockResolvedValue(
      page([
        {
          code: "111111",
          name: "상승주",
          exchange: "KOSPI",
          price: "1,000",
          changeRate: "5.00",
          changeType: "rising",
          isHotDiscussion: false,
        },
        {
          code: "222222",
          name: "보합주",
          exchange: "KOSPI",
          price: "2,000",
          changeRate: "0.00",
          changeType: "even",
          isHotDiscussion: false,
        },
      ]),
    );
    renderModal();
    const rising = await screen.findByText("5.00");
    const even = await screen.findByText("0.00");
    expect(rising).toHaveStyle({ color: "var(--mantine-color-red-6)" });
    expect(even).toHaveStyle({ color: "var(--mantine-color-gray-6)" });
  });

  it("거래소 모달은 Escape로 닫을 수 있다", async () => {
    renderModal();
    await screen.findByText("SK하이닉스");
    fireEvent.click(screen.getByRole("button", { name: /KRX/ }));
    expect(
      await screen.findByRole("button", { name: "NXT" }),
    ).toBeInTheDocument();
    fireEvent.keyDown(document.body, { key: "Escape" });
    await waitFor(() =>
      expect(screen.queryByText("거래소 선택")).not.toBeInTheDocument(),
    );
  });

  it("검색 모드에서도 더보기로 다음 검색 페이지를 append한다", async () => {
    searchMock
      .mockResolvedValueOnce(
        page(
          [
            {
              code: "069500",
              name: "KODEX 200",
              exchange: "코스피",
              price: "",
              changeRate: "",
              changeType: "even",
              isHotDiscussion: false,
            },
          ],
          true,
        ),
      )
      .mockResolvedValueOnce(
        page([
          {
            code: "229200",
            name: "KODEX 코스닥150",
            exchange: "코스닥",
            price: "",
            changeRate: "",
            changeType: "even",
            isHotDiscussion: false,
          },
        ]),
      );
    renderModal();
    fireEvent.change(screen.getByPlaceholderText("종목명 또는 코드 검색"), {
      target: { value: "ko" },
    });
    expect(await screen.findByText("KODEX 200")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "더보기" }));
    await waitFor(() => expect(searchMock).toHaveBeenCalledWith("ko", 2));
    expect(await screen.findByText("KODEX 코스닥150")).toBeInTheDocument();
  });

  it("취소를 누르면 onClose가 호출된다", async () => {
    const onClose = vi.fn();
    render(
      <MantineProvider>
        <StockCrawlModal
          open
          preselected={[]}
          onClose={onClose}
          onConfirm={vi.fn()}
        />
      </MantineProvider>,
    );
    await screen.findByText("SK하이닉스");
    await userEvent.click(screen.getByRole("button", { name: "취소" }));
    expect(onClose).toHaveBeenCalled();
  });
});
