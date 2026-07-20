import { MantineProvider } from "@mantine/core";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { type ComponentProps, useState } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

// 게시명령 화면의 종토 옵션(닉네임 랜덤·게시 후 내용변경, 15-기타명령 §3·§4) 렌더/배선 검증.
// 네트워크(api)·알림(notifications)은 mock. 컴포넌트를 직접 렌더해 게이트/위치/payload를 확인한다.
const send = vi.hoisted(() =>
  vi.fn((_req: Record<string, unknown>) =>
    Promise.resolve({ ok: true, commandId: "c1" }),
  ),
);
const create = vi.hoisted(() =>
  vi.fn((_req: Record<string, unknown>) =>
    Promise.resolve({ ok: true, id: "s1" }),
  ),
);
const queryNick = vi.hoisted(() =>
  vi.fn(() => Promise.resolve({ ok: true, commandId: "c2" })),
);
const nickRemaining = vi.hoisted(() => vi.fn(() => Promise.resolve({})));
// 종목 프록시 목 — 실제 서버처럼 비어있지 않은 종목 목록을 준다(자동 pickStocks 산출·수동 모달 공용).
const STOCK_ROWS = vi.hoisted(() => [
  {
    code: "000660",
    name: "SK하이닉스",
    exchange: "KOSPI",
    price: "1,911,000",
    changeRate: "-7.68",
    changeType: "falling",
    isHotDiscussion: true,
  },
  {
    code: "035420",
    name: "NAVER",
    exchange: "KOSPI",
    price: "200,000",
    changeRate: "1.00",
    changeType: "rising",
    isHotDiscussion: false,
  },
  {
    code: "035720",
    name: "카카오",
    exchange: "KOSPI",
    price: "50,000",
    changeRate: "0.50",
    changeType: "rising",
    isHotDiscussion: false,
  },
]);
const stocksList = vi.hoisted(() =>
  vi.fn(() =>
    Promise.resolve({
      stocks: STOCK_ROWS,
      totalCount: 3,
      page: 1,
      hasNext: false,
    }),
  ),
);
const stocksSearch = vi.hoisted(() =>
  vi.fn(() =>
    Promise.resolve({
      stocks: STOCK_ROWS,
      totalCount: 3,
      page: 1,
      hasNext: false,
    }),
  ),
);
const postReportsList = vi.hoisted(() => vi.fn(() => Promise.resolve([])));

vi.mock("../../api", () => ({
  isOffline: () => false,
  api: {
    publish: { send },
    scheduled: { create },
    forumStocks: { list: stocksList, search: stocksSearch },
    postReports: { list: postReportsList },
    devices: {
      queryNicknameRemaining: queryNick,
      nicknameRemaining: nickRemaining,
    },
  },
}));
vi.mock("@mantine/notifications", () => ({
  notifications: { show: vi.fn() },
}));

import { ForumCommentConfig, ForumConfig } from "./publish-command";

const device = { id: "d1", name: "하위-001", ip: "1.2.3.4" };

function renderCmt(postCommentCount: number) {
  return render(
    <MantineProvider>
      <ForumCommentConfig
        device={device}
        postId="p1"
        postTitle="급등주 분석"
        postCommentCount={postCommentCount}
        accounts={["acc_a", "acc_b"]}
        onSchedule={() => {}}
      />
    </MantineProvider>,
  );
}

// ForumConfig는 cfg를 부모가 들고 onPatch로 갱신한다(체크박스 → 입력 노출). 그 왕복을 재현하는 래퍼.
function ForumConfigHarness({
  mode = "post",
  postCommentCount = 0,
}: {
  mode?: ComponentProps<typeof ForumConfig>["mode"];
  postCommentCount?: number;
}) {
  const [cfg, setCfg] = useState<ComponentProps<typeof ForumConfig>["cfg"]>({
    category: "tradingValue",
    market: "all",
    count: 3,
    accounts: ["acc_a"],
    contentChange: { enabled: false, title: "", body: "", delaySec: 0 },
  });
  return (
    <MantineProvider>
      <ForumConfig
        device={device}
        mode={mode}
        cfg={cfg}
        onPatch={(patch) => setCfg((c) => ({ ...c, ...patch }))}
        postId="p1"
        postTitle="급등주 분석"
        postCommentCount={postCommentCount}
        accounts={["acc_a"]}
        onSchedule={() => {}}
      />
    </MantineProvider>
  );
}

describe("게시명령 종토 옵션(닉네임 랜덤·게시 후 내용변경) 렌더", () => {
  beforeEach(() => {
    send.mockClear();
    create.mockClear();
    queryNick.mockClear();
    nickRemaining.mockClear();
  });

  describe("ForumCommentConfig 닉네임 랜덤(15-기타명령 §3)", () => {
    it("작성 댓글 수 < 2면 체크박스를 숨긴다(계정 여러 개여도)", () => {
      renderCmt(1);
      expect(screen.queryByText(/닉네임 랜덤/)).toBeNull();
    });

    it("작성 댓글 수 ≥ 2면 체크박스를 보인다", () => {
      renderCmt(2);
      expect(screen.getByText(/닉네임 랜덤/)).toBeInTheDocument();
    });

    it("체크하면 계정별 변경 가능횟수를 원격 조회하고, 켠 채 게시하면 payload에 실린다", async () => {
      const user = userEvent.setup();
      renderCmt(2);
      // URL 1개 + 계정 1개 선택 → 게시 활성.
      const urlInput = screen.getByPlaceholderText("종목토론방 글 URL 1");
      await user.type(urlInput, "https://post/1");
      // 두 계정(acc_a·acc_b)은 마스킹이 같아("ac•••") 첫 버튼을 고른다.
      const [firstAcct] = screen.getAllByRole("button", { name: /^ac•+$/ });
      await user.click(firstAcct!);
      // 닉네임 랜덤 체크 → 원격 조회 요청(§6-2 실시간).
      await user.click(screen.getByRole("checkbox", { name: /닉네임 랜덤/ }));
      await waitFor(() => expect(queryNick).toHaveBeenCalled());
      // "지금 게시" → commentNicknameRandom=true가 payload에 실린다.
      await user.click(screen.getByRole("button", { name: "지금 게시" }));
      await waitFor(() => expect(send).toHaveBeenCalled());
      expect(send).toHaveBeenCalledWith(
        expect.objectContaining({
          mode: "comment",
          commentNicknameRandom: true,
        }),
      );
    });
  });

  describe("ForumConfig 닉네임 랜덤(글+댓글, 15-기타명령 §3)", () => {
    it("글+댓글(both)이고 작성 댓글 수 ≥ 2면 체크박스를 보인다", () => {
      render(<ForumConfigHarness mode="both" postCommentCount={2} />);
      expect(screen.getByText(/닉네임 랜덤/)).toBeInTheDocument();
    });

    it("글+댓글(both)이라도 작성 댓글 수 < 2면 숨긴다", () => {
      render(<ForumConfigHarness mode="both" postCommentCount={1} />);
      expect(screen.queryByText(/닉네임 랜덤/)).toBeNull();
    });

    it("순수 글(post) 모드는 작성 댓글 수 ≥ 2여도 숨긴다", () => {
      render(<ForumConfigHarness mode="post" postCommentCount={5} />);
      expect(screen.queryByText(/닉네임 랜덤/)).toBeNull();
    });

    it("켠 채 게시하면 commentNicknameRandom=true가 payload에 실린다", async () => {
      const user = userEvent.setup();
      render(<ForumConfigHarness mode="both" postCommentCount={2} />);
      await user.click(screen.getByRole("checkbox", { name: /닉네임 랜덤/ }));
      await waitFor(() => expect(queryNick).toHaveBeenCalled());
      await user.click(screen.getByRole("button", { name: "지금 게시" }));
      await waitFor(() => expect(send).toHaveBeenCalled());
      expect(send).toHaveBeenCalledWith(
        expect.objectContaining({ mode: "both", commentNicknameRandom: true }),
      );
    });
  });

  describe("ForumConfig 게시 후 내용변경(15-기타명령 §4)", () => {
    it("체크박스가 종목 수와 계정 사이에 렌더된다(위치)", () => {
      render(<ForumConfigHarness />);
      const stockCount = screen.getByText("종목 수");
      const contentChange = screen.getByText("게시 후 내용변경");
      const accounts = screen.getByText(/^계정 \(/);
      // DOM 순서: 종목 수 → 게시 후 내용변경 → 계정.
      expect(
        stockCount.compareDocumentPosition(contentChange) &
          Node.DOCUMENT_POSITION_FOLLOWING,
      ).toBeTruthy();
      expect(
        contentChange.compareDocumentPosition(accounts) &
          Node.DOCUMENT_POSITION_FOLLOWING,
      ).toBeTruthy();
    });

    it("체크 후 제목/내용/지연을 입력하고 게시하면 contentChange가 payload에 실린다", async () => {
      const user = userEvent.setup();
      render(<ForumConfigHarness />);
      // 기본은 접혀 있다(입력 없음).
      expect(screen.queryByPlaceholderText("변경할 새 제목")).toBeNull();
      await user.click(
        screen.getByRole("checkbox", { name: "게시 후 내용변경" }),
      );
      await user.type(screen.getByPlaceholderText("변경할 새 제목"), "새 제목");
      await user.type(screen.getByPlaceholderText("변경할 새 내용"), "새 본문");
      await user.click(screen.getByRole("button", { name: "지금 게시" }));
      await waitFor(() => expect(send).toHaveBeenCalled());
      expect(send).toHaveBeenCalledWith(
        expect.objectContaining({
          mode: "post",
          contentChange: { title: "새 제목", body: "새 본문", delaySec: 0 },
        }),
      );
    });

    it("체크 안 하면 contentChange를 payload에 싣지 않는다", async () => {
      const user = userEvent.setup();
      render(<ForumConfigHarness />);
      await user.click(screen.getByRole("button", { name: "지금 게시" }));
      await waitFor(() => expect(send).toHaveBeenCalled());
      expect(send).toHaveBeenCalledWith(
        expect.not.objectContaining({ contentChange: expect.anything() }),
      );
    });
  });

  describe("ForumConfig 종목 자동/수동 선택 토글(§18)", () => {
    it("기본은 자동선택 — 카테고리/시장/종목 수 블록이 보인다", () => {
      render(<ForumConfigHarness />);
      expect(screen.getByText("카테고리")).toBeInTheDocument();
      expect(screen.getByText(/^시장/)).toBeInTheDocument();
      expect(screen.getByText("종목 수")).toBeInTheDocument();
      // 자동 토글이 눌린 상태(수동 선택 버튼은 아직 없음).
      expect(
        screen.getByRole("button", { name: "종목 자동선택" }),
      ).toBeInTheDocument();
      expect(
        screen.queryByRole("button", { name: "종목 선택" }),
      ).not.toBeInTheDocument();
    });

    it("수동선택으로 전환하면 카테고리/시장/종목 수를 숨기고 '종목 선택' 버튼을 보인다", async () => {
      const user = userEvent.setup();
      render(<ForumConfigHarness />);
      await user.click(screen.getByRole("button", { name: "종목 수동선택" }));
      expect(screen.queryByText("카테고리")).not.toBeInTheDocument();
      expect(screen.queryByText("종목 수")).not.toBeInTheDocument();
      expect(
        screen.getByRole("button", { name: "종목 선택" }),
      ).toBeInTheDocument();
    });

    it("수동으로 종목을 고르면 배지가 뜨고, 게시 시 assignments에 그대로 흐른다", async () => {
      const user = userEvent.setup();
      render(<ForumConfigHarness />);
      await user.click(screen.getByRole("button", { name: "종목 수동선택" }));
      // 종목 미선택 → 게시 비활성.
      expect(screen.getByRole("button", { name: "지금 게시" })).toBeDisabled();
      // 모달 열기 → 종목 선택 → 적용.
      await user.click(screen.getByRole("button", { name: "종목 선택" }));
      await user.click(await screen.findByText("SK하이닉스"));
      await user.click(
        await screen.findByRole("button", { name: /적용 \(1\)/ }),
      );
      // 배지로 선택 종목이 표시된다(모달 닫힘 후 본문에 남는다).
      await waitFor(() =>
        expect(
          screen.queryByRole("button", { name: /적용/ }),
        ).not.toBeInTheDocument(),
      );
      expect(screen.getByText("SK하이닉스")).toBeInTheDocument();
      // 게시 활성화 → 전송 payload의 assignments에 고른 종목이 실린다.
      await user.click(screen.getByRole("button", { name: "지금 게시" }));
      await waitFor(() => expect(send).toHaveBeenCalled());
      expect(send).toHaveBeenCalledWith(
        expect.objectContaining({
          assignments: [
            {
              loginId: "acc_a",
              stocks: [{ code: "000660", name: "SK하이닉스" }],
            },
          ],
        }),
      );
    });
  });
});
