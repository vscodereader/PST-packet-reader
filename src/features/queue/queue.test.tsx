import { MantineProvider } from "@mantine/core";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, it, expect, vi } from "vitest";

import type { QueueNowItem } from "@/shared/data/types";
import { resetIpc, setCommandFailures, setQueueNow } from "@/test/ipc";

import { Queue } from "./queue";

vi.mock("@tauri-apps/api/core", async () => ({
  invoke: (await import("@/test/ipc")).invoke,
}));

// 진행 중 아이템 1건 — 대상별 라이브 상태(성공/실패/진행중/대기 4종)를 함께 담는다.
const RUNNING_ITEM: QueueNowItem = {
  id: "qr1",
  title: "진행 중 게시 작업",
  kind: "post",
  state: "running",
  progress: [2, 4],
  locs: [{ p: "naver", name: "개미투자 카페" }],
  items: [
    {
      platform: "naver",
      target: "ZZ카페하나",
      loginId: "user01",
      status: "success",
      msg: "글 게시 완료",
    },
    {
      platform: "forum",
      target: "ZZ종목둘",
      code: "086520",
      loginId: "user02",
      status: "fail",
      msg: "종목토론방 게시에 실패했습니다",
      trace: "FORUM_ERR\nstack",
    },
    {
      platform: "band",
      target: "ZZ밴드셋",
      loginId: "user03",
      status: "running",
      msg: "게시 중…",
    },
    {
      platform: "naver",
      target: "ZZ카페넷",
      loginId: "user04",
      status: "waiting",
      msg: "대기 중",
    },
  ],
};

async function renderQueue(go = vi.fn()) {
  render(
    <MantineProvider>
      <Queue go={go} />
    </MantineProvider>,
  );
  // now + scheduled lists load asynchronously over the IPC wrapper (mock here)
  await screen.findByText("반도체 흐름 코멘트 10종");
  await screen.findByText("에코프로 조정 구간 대응 전략");
  return go;
}

describe("Queue", () => {
  beforeEach(() => {
    resetIpc();
  });

  it("renders the title and both queue sections", async () => {
    await renderQueue();
    expect(
      screen.getByRole("heading", { name: "게시 큐" }),
    ).toBeInTheDocument();
    expect(screen.getByText("즉시 처리 대기열")).toBeInTheDocument();
    expect(screen.getByText("예약 대기")).toBeInTheDocument();
  });

  it("labels only a login-only item '로그인'; a publish item carrying plan.login shows its kind (#225)", async () => {
    // 게시 아이템도 이제 plan.login을 동봉(게시 직전 계정별 로그인)하므로, plan.login 유무만으로
    // "로그인"을 붙이면 게시 아이템이 오표시된다. 게시 타깃이 있으면 종류(글)로 표시해야 한다.
    const login = {
      accountId: "user01",
      platform: "naver" as const,
      headless: false,
      useAdb: true,
      force: true,
    };
    const base = {
      postId: "",
      kind: "post" as const,
      title: "",
      bodyText: "",
      comments: [],
      linkOverride: "",
      naver: [],
      forum: [],
      band: [],
    };
    const items: QueueNowItem[] = [
      {
        id: "qlogin",
        title: "계정 로그인 작업",
        kind: "post",
        state: "waiting",
        locs: [{ p: "naver", name: "user01" }],
        items: [],
        plan: { ...base, title: "계정 로그인 작업", login: [login] },
      },
      {
        id: "qpub",
        title: "카페 글 게시 작업",
        kind: "post",
        state: "waiting",
        locs: [{ p: "naver", name: "카페" }],
        items: [],
        plan: {
          ...base,
          title: "카페 글 게시 작업",
          naver: [
            {
              accountId: "user01",
              cafe: "111",
              cafeName: "카페",
              menuId: 1,
              boardType: "",
            },
          ],
          login: [login],
        },
      },
    ];
    setQueueNow(items);
    render(
      <MantineProvider>
        <Queue go={vi.fn()} />
      </MantineProvider>,
    );
    await screen.findByText("카페 글 게시 작업");
    // "로그인" 배지는 로그인 전용 아이템 1건에만 — 수정 전엔 게시 아이템에도 붙어 2건이었다.
    expect(screen.getAllByText("로그인")).toHaveLength(1);
  });

  it("navigates to posts via '새 작업 추가'", async () => {
    const go = await renderQueue();
    await userEvent.click(screen.getByRole("button", { name: /새 작업 추가/ }));
    expect(go).toHaveBeenCalledWith("posts");
  });

  it("cancels a waiting item", async () => {
    await renderQueue();
    const title = "반도체 흐름 코멘트 10종";
    expect(screen.getByText(title)).toBeInTheDocument();
    await userEvent.click(screen.getAllByTitle("취소")[0]!);
    await waitFor(() =>
      expect(screen.queryByText(title)).not.toBeInTheDocument(),
    );
  });

  it("reorders waiting items with the move-down control", async () => {
    await renderQueue();
    const q2 = "반도체 흐름 코멘트 10종";
    const q3 = "오늘의 특징주 정리 — 장 마감 요약";
    // boundary: moving the first waiting item up is a no-op
    await userEvent.click(screen.getAllByTitle("우선순위 올리기")[0]!);
    // move first waiting item down → q3 now precedes q2
    await userEvent.click(screen.getAllByTitle("우선순위 내리기")[0]!);
    const q2El = screen.getByText(q2);
    const q3El = screen.getByText(q3);
    expect(
      q3El.compareDocumentPosition(q2El) & Node.DOCUMENT_POSITION_FOLLOWING,
    ).toBeTruthy();
  });

  it("exposes 즉시 처리 and 예약 취소 on scheduled rows", async () => {
    await renderQueue();
    await userEvent.click(
      screen.getAllByRole("button", { name: /즉시 처리/ })[0]!,
    );
    await userEvent.click(screen.getAllByTitle("예약 취소")[0]!);
    expect(screen.getByText("예약 대기")).toBeInTheDocument();
  });

  it("shows 놓침 + 재예약 for a missed schedule", async () => {
    await renderQueue();
    // qs3(HBM)은 missed → "놓침" 뱃지와 "재예약" 버튼이 보인다.
    expect(screen.getByText("HBM 관련 기대 코멘트")).toBeInTheDocument();
    expect(screen.getByText("놓침")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "재예약" })).toBeInTheDocument();
  });

  it("reschedules a missed item, clearing 놓침", async () => {
    await renderQueue();
    expect(screen.getByText("놓침")).toBeInTheDocument();
    // 재예약 버튼(놓친 항목 전용)을 누르면 현재 시각으로 재예약돼 missed가 풀린다.
    await userEvent.click(screen.getByRole("button", { name: "재예약" }));
    await waitFor(() =>
      expect(screen.queryByText("놓침")).not.toBeInTheDocument(),
    );
  });

  it("reorders waiting items via drag and drop", async () => {
    await renderQueue();
    const q2 = "반도체 흐름 코멘트 10종";
    const q3 = "오늘의 특징주 정리 — 장 마감 요약";
    const q2row = screen.getByText(q2).closest("[draggable]")!;
    const q3row = screen.getByText(q3).closest("[draggable]")!;
    const dataTransfer = {
      effectAllowed: "",
      setData: () => {},
      getData: () => "",
    };
    fireEvent.dragStart(q2row, { dataTransfer });
    fireEvent.dragOver(q3row, { dataTransfer });
    fireEvent.dragEnd(q2row, { dataTransfer });
    const q2El = screen.getByText(q2);
    const q3El = screen.getByText(q3);
    expect(
      q3El.compareDocumentPosition(q2El) & Node.DOCUMENT_POSITION_FOLLOWING,
    ).toBeTruthy();
  });

  it("reverts to the backend order and warns when persisting a reorder fails", async () => {
    setCommandFailures(["reorder_queue_now"]);
    await renderQueue();
    const q2 = "반도체 흐름 코멘트 10종";
    const q3 = "오늘의 특징주 정리 — 장 마감 요약";
    // 낙관적으로 q2를 내렸다가, 영속화 실패(reorder_queue_now reject) → catch가 백엔드
    // 순서를 다시 불러와 원래 순서(q2가 q3보다 앞)로 되돌린다.
    await userEvent.click(screen.getAllByTitle("우선순위 내리기")[0]!);
    await waitFor(() => {
      const q2El = screen.getByText(q2);
      const q3El = screen.getByText(q3);
      expect(
        q2El.compareDocumentPosition(q3El) & Node.DOCUMENT_POSITION_FOLLOWING,
      ).toBeTruthy();
    });
  });

  it("keeps the 놓침 badge when a reschedule fails", async () => {
    setCommandFailures(["reschedule_queue_scheduled"]);
    const { invoke } = await import("@tauri-apps/api/core");
    await renderQueue();
    await userEvent.click(screen.getByRole("button", { name: "재예약" }));
    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith(
        "reschedule_queue_scheduled",
        expect.anything(),
      ),
    );
    // 실패 시 setSched를 부르지 않으므로 missed(놓침)가 그대로 남는다.
    expect(screen.getByText("놓침")).toBeInTheDocument();
  });

  it("진행 중 아이템을 클릭하면 대상별 상태가 인라인으로 펼쳐진다", async () => {
    setQueueNow([RUNNING_ITEM]);
    const go = vi.fn();
    render(
      <MantineProvider>
        <Queue go={go} />
      </MantineProvider>,
    );
    const title = await screen.findByText("진행 중 게시 작업");
    // 펼치기 전엔 대상 목록(ZZ종목둘은 items에만 있음)이 보이지 않는다.
    expect(screen.queryByText("ZZ종목둘")).not.toBeInTheDocument();

    await userEvent.click(title);

    // 4개 대상의 상태가 SubLog로 펼쳐진다.
    expect(await screen.findByText("ZZ종목둘")).toBeInTheDocument();
    expect(screen.getByText("ZZ밴드셋")).toBeInTheDocument();
    expect(screen.getByText("ZZ카페넷")).toBeInTheDocument();
    expect(
      screen.getByText("종목토론방 게시에 실패했습니다"),
    ).toBeInTheDocument();
    // 알림 화면으로 이동하지 않고 인라인으로 펼친다(#219 — 기존 네비게이션 대체).
    expect(go).not.toHaveBeenCalledWith("log", expect.anything());

    // 다시 클릭하면 접힌다.
    await userEvent.click(title);
    await waitFor(() =>
      expect(screen.queryByText("ZZ종목둘")).not.toBeInTheDocument(),
    );
  });

  it("항목이 아직 없는 진행 중 아이템은 펼치면 준비 안내를 보여준다", async () => {
    setQueueNow([{ ...RUNNING_ITEM, items: [] }]);
    render(
      <MantineProvider>
        <Queue go={vi.fn()} />
      </MantineProvider>,
    );
    const title = await screen.findByText("진행 중 게시 작업");
    await userEvent.click(title);
    expect(
      await screen.findByText("진행 상태를 준비하고 있어요…"),
    ).toBeInTheDocument();
  });

  it("persists the new order so it survives a reload", async () => {
    const q2 = "반도체 흐름 코멘트 10종";
    const q3 = "오늘의 특징주 정리 — 장 마감 요약";
    const { unmount } = render(
      <MantineProvider>
        <Queue go={vi.fn()} />
      </MantineProvider>,
    );
    await screen.findByText(q2);

    // 첫 대기 항목(q2)을 한 칸 내린다 → 백엔드(reorder_queue_now)에 영속화돼야 한다.
    await userEvent.click(screen.getAllByTitle("우선순위 내리기")[0]!);
    unmount();

    // 재마운트 시 백엔드에서 다시 로드 → 로컬 상태가 아니라 영속화된 순서여야 한다.
    render(
      <MantineProvider>
        <Queue go={vi.fn()} />
      </MantineProvider>,
    );
    await screen.findByText(q2);
    const q2El = screen.getByText(q2);
    const q3El = screen.getByText(q3);
    expect(
      q3El.compareDocumentPosition(q2El) & Node.DOCUMENT_POSITION_FOLLOWING,
    ).toBeTruthy();
  });
});
