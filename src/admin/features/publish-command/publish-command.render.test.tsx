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
const stocksList = vi.hoisted(() => vi.fn(() => Promise.resolve({ stocks: [] })));

vi.mock("../../api", () => ({
  isOffline: () => false,
  api: {
    publish: { send },
    scheduled: { create },
    forumStocks: { list: stocksList },
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
function ForumConfigHarness() {
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
        mode="post"
        cfg={cfg}
        onPatch={(patch) => setCfg((c) => ({ ...c, ...patch }))}
        postId="p1"
        postTitle="급등주 분석"
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
      expect.objectContaining({ mode: "comment", commentNicknameRandom: true }),
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
    await user.click(screen.getByRole("checkbox", { name: "게시 후 내용변경" }));
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
});
