import { MantineProvider } from "@mantine/core";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, it, expect, vi } from "vitest";

import type { LibraryPost, PublishPlan } from "@/shared/data/types";
import { invoke as ipcBackend, resetIpc, setAccounts } from "@/test/ipc";
import { pickOption } from "@/test/select";

import { PublishModal } from "./publish-modal";

vi.mock("@tauri-apps/api/core", async () => ({
  invoke: (await import("@/test/ipc")).invoke,
}));

// 테스트는 <Notifications/> 없이 렌더하므로 토스트가 DOM에 뜨지 않는다.
// notifications.show를 스파이로 대체해 red 토스트를 단언한다.
const { notifShow } = vi.hoisted(() => ({ notifShow: vi.fn() }));
vi.mock("@mantine/notifications", () => ({
  notifications: { show: notifShow },
}));

const postDoc: LibraryPost = {
  id: "l1",
  title: "#{종목명} 4분기 실적 기대",
  kind: "post",
  updated: "방금 전",
  words: 100,
  status: "ready",
  excerpt: "요약",
  body: "<p>#{종목명} 본문 #{링크}</p>",
};

function renderPublish(over: Partial<Parameters<typeof PublishModal>[0]> = {}) {
  const go = vi.fn();
  render(
    <MantineProvider>
      <PublishModal open doc={postDoc} onClose={vi.fn()} go={go} {...over} />
    </MantineProvider>,
  );
  return { go };
}

/** 새 흐름(밴드 미러): 게시판 링크를 붙여넣어 카페 대상을 추가하고 선택한다. cafeId+menuId만
 *  링크에서 파싱되고, boardType은 게시 시점 백엔드가 해결한다(모달은 쿠키를 안 쓴다). */
async function addNaverCafe(cafeId: number, menuId = 1) {
  const input = await screen.findByPlaceholderText(
    "https://cafe.naver.com/f-e/cafes/31732304/menus/1",
  );
  await userEvent.clear(input);
  await userEvent.type(
    input,
    `https://cafe.naver.com/f-e/cafes/${cafeId}/menus/${menuId}`,
  );
  await userEvent.click(screen.getByRole("button", { name: "추가" }));
  await pickOption(0, `카페 ${cafeId} · 게시판 ${menuId}`);
}

// 기본 종목 선택이 사라져(#267-11) forum 계정으로 게시하려면 종목을 직접 골라야 한다. 종목 선택은
// 계정 토글로 비워지므로(#267-2) 계정 선택을 모두 끝낸 뒤 호출한다 — 기본 forum 계정(a1)을 게시
// 가능하게 만들어, 이전 기본 종목 005930에 의존하던 테스트 상태를 그대로 복원한다.
async function pickStock(query = "005930", name = "삼성전자") {
  await userEvent.click(
    await screen.findByRole("button", { name: /종목 선택/ }),
  );
  const search = await screen.findByPlaceholderText("종목명 또는 코드 검색");
  await userEvent.clear(search);
  await userEvent.type(search, query);
  await userEvent.click(await screen.findByText(name));
  await userEvent.click(await screen.findByRole("button", { name: /적용/ }));
}

/**
 * 즉시 게시("지금 바로")는 백엔드 게시를 직접 호출하지 않고, 게시 큐의 즉시 처리
 * 대기열에 아이템 하나를 적재한다(add_queue_now, #198). 그 아이템에 동결된 plan을
 * 꺼내 단언에 쓴다 — 실제 게시(글/댓글/밴드/종토방)와 그 결과 매핑은 워커가 맡으므로
 * queue_runner의 백엔드 테스트가 검증한다.
 */
function enqueuedPlan(): PublishPlan {
  const call = ipcBackend.mock.calls.find((c) => c[0] === "add_queue_now");
  expect(call).toBeDefined();
  return (call![1] as { item: { plan: PublishPlan } }).item.plan;
}

describe("PublishModal", () => {
  // The shared in-memory IPC mock is module-level; reset its fixtures and clear
  // recorded calls between tests so `mock.calls.find(...)` never matches a stale
  // call from an earlier test (e.g. the latest/popular comment-job assertions).
  beforeEach(() => {
    resetIpc();
    ipcBackend.mockClear();
    notifShow.mockClear();
  });

  it("renders 게시 설정 with the document title", async () => {
    renderPublish();
    const dialog = await screen.findByRole("dialog");
    expect(dialog).toHaveTextContent("게시 설정");
    expect(dialog).toHaveTextContent("#{종목명} 4분기 실적 기대");
  });

  it("shows the template-variable substitution card for tokenized docs", async () => {
    renderPublish();
    expect(await screen.findByText("변수 자동 치환")).toBeInTheDocument();
  });

  it("opens the preview from 미리보기", async () => {
    renderPublish();
    await userEvent.click(
      await screen.findByRole("button", { name: "미리보기" }),
    );
    expect(await screen.findByText(/각 게시판 서식으로/)).toBeInTheDocument();
  });

  it("reveals the schedule button when 예약 게시 is chosen", async () => {
    renderPublish();
    await userEvent.click(await screen.findByText("예약 게시"));
    expect(
      await screen.findByRole("button", { name: /예약 \(\d+\)/ }),
    ).toBeInTheDocument();
  });

  it("opens the stock crawl modal from 종목 선택", async () => {
    renderPublish();
    await userEvent.click(
      await screen.findByRole("button", { name: /종목 선택/ }),
    );
    // 재디자인된 종목 선택 모달은 검색창 + 카테고리 탭을 띄운다.
    expect(
      await screen.findByPlaceholderText("종목명 또는 코드 검색"),
    ).toBeInTheDocument();
    expect(
      await screen.findByRole("button", { name: "거래대금" }),
    ).toBeInTheDocument();
  });

  it("enqueues the publish and shows the queued confirmation", async () => {
    renderPublish();
    await pickStock();
    await userEvent.click(
      await screen.findByRole("button", { name: /^게시 \(\d+\)/ }),
    );
    // 즉시 게시는 큐에 적재하고(add_queue_now) 결과 패널에 "대기열에 추가됨"을 보여준다.
    // 실제 진행률은 모달이 아니라 게시 큐에서 보인다(#198).
    expect(
      await screen.findByText(/대기열에 추가됨/, undefined, { timeout: 3000 }),
    ).toBeInTheDocument();
    expect(ipcBackend).toHaveBeenCalledWith("add_queue_now", expect.anything());
  });

  it("routes to the publish queue after an immediate publish", async () => {
    const { go } = renderPublish();
    await pickStock();
    await userEvent.click(
      await screen.findByRole("button", { name: /^게시 \(\d+\)/ }),
    );
    // 큐에 적재되면 결과 패널이 뜬다.
    expect(
      await screen.findByText("계속 작성", undefined, { timeout: 3000 }),
    ).toBeInTheDocument();
    // 즉시 게시도 큐를 타므로 후속 버튼은 "게시큐 보기" → 큐 화면으로 이동한다(#198).
    await userEvent.click(screen.getByRole("button", { name: "게시큐 보기" }));
    expect(go).toHaveBeenCalledWith("queue");
  });

  it("게시(숫자) 후 '계속작성'은 모달을 유지하고 방금 게시한 계정이 목록에서 빠진다(#4/#5)", async () => {
    // 기본 선택 계정(a1, invest_king7)으로 즉시 게시한다.
    const onClose = vi.fn();
    renderPublish({ onClose });
    // 게시 전에는 계정 체크박스 목록에 invest_king7이 보인다.
    expect(await screen.findByText("invest_king7")).toBeInTheDocument();
    await pickStock();
    await userEvent.click(
      await screen.findByRole("button", { name: /^게시 \(\d+\)/ }),
    );
    // 결과 패널(계속 작성/게시큐 보기)이 뜬다.
    const keep = await screen.findByRole(
      "button",
      { name: "계속 작성" },
      { timeout: 3000 },
    );
    // '계속작성'은 게시설정창을 닫지 않는다(#5) — onClose는 호출되지 않는다.
    await userEvent.click(keep);
    expect(onClose).not.toHaveBeenCalled();
    // 게시설정창은 그대로 유지된다.
    expect(await screen.findByText("게시 설정")).toBeInTheDocument();
    // 그리고 방금 게시한 계정(invest_king7)은 새로고침된 계정 목록에서 빠진다(#4).
    await waitFor(() =>
      expect(screen.queryByText("invest_king7")).not.toBeInTheDocument(),
    );
    // 선택 개수도 0개가 된다(방금 쓴 계정만 선택돼 있었으므로).
    expect(await screen.findByText("0개")).toBeInTheDocument();
  });

  it("게시(숫자) 직후 결과 패널이 떠 있는 동안에도 선택은 게시 계정에서 풀린다(#4)", async () => {
    renderPublish();
    expect(await screen.findByText("1개")).toBeInTheDocument();
    await pickStock();
    await userEvent.click(
      await screen.findByRole("button", { name: /^게시 \(\d+\)/ }),
    );
    // 결과 패널이 뜨면 큐 적재가 끝난 것 — 그 즉시 선택이 비워진다(0개).
    expect(
      await screen.findByText("계속 작성", undefined, { timeout: 3000 }),
    ).toBeInTheDocument();
    await waitFor(() => expect(screen.getByText("0개")).toBeInTheDocument());
  });

  it("offers a retry control when enqueuing fails", async () => {
    // 큐 적재(add_queue_now)가 거부되면 결과 행을 실패로 두고 재시도 버튼을 보여준다.
    const real = ipcBackend.getMockImplementation()!;
    ipcBackend.mockImplementation(
      (cmd: string, args?: Record<string, unknown>) =>
        cmd === "add_queue_now"
          ? Promise.reject(new Error("큐 적재 실패"))
          : real(cmd, args),
    );
    try {
      renderPublish();
      await pickStock();
      await userEvent.click(
        await screen.findByRole("button", { name: /^게시 \(\d+\)/ }),
      );
      expect(
        await screen.findByRole(
          "button",
          { name: "재시도" },
          { timeout: 3000 },
        ),
      ).toBeInTheDocument();
    } finally {
      ipcBackend.mockImplementation(real);
    }
  });

  it("enqueues naver jobs to the immediate-processing queue", async () => {
    renderPublish();
    // Swap the preselected forum account for a naver one. Selecting it loads
    // that account's joined cafes; picking a cafe resolves its boards (first
    // board auto-selected), which drives a real backend publish.
    await userEvent.click(await screen.findByText("invest_king7"));
    await userEvent.click(screen.getByText("money_lab"));
    await addNaverCafe(11111111);
    // boards resolve and the first board is auto-selected → the naver job
    // becomes valid, so the publish button's count ticks up to 1.
    await userEvent.click(
      await screen.findByRole(
        "button",
        { name: /^게시 \(1\)/ },
        { timeout: 3000 },
      ),
    );
    // The result row renders "{loginId} · {msg}" in one node, so match loosely.
    expect(
      await screen.findByText(/대기열에 추가됨/, undefined, { timeout: 3000 }),
    ).toBeInTheDocument();
    // 즉시 게시는 백엔드 게시를 직접 부르지 않고 즉시 처리 대기열에 적재한다(add_queue_now,
    // #198). 적재 아이템의 plan.naver에 동결된 글 정보가 실린다 — 실제 게시는 워커가 한다.
    expect(ipcBackend).toHaveBeenCalledWith(
      "add_queue_now",
      expect.objectContaining({
        item: expect.objectContaining({
          state: "waiting",
          plan: expect.objectContaining({
            naver: [
              expect.objectContaining({
                // 백엔드는 쿠키 파일 키(loginId)로 계정을 찾는다 — UI 고유 id("a5")가 아니다.
                accountId: "money_lab",
                cafe: "11111111",
                menuId: 1,
                boardType: "",
              }),
            ],
          }),
        }),
      }),
    );
  });

  it("attaches plan.login and saves credentials for the target account (#225)", async () => {
    // 선택 로그인을 없앤 대신, 게시가 대상 계정마다 plan.login(force+useAdb)을 동봉하고
    // 자격증명을 accounts.json에 저장한다 — 워커가 게시 직전 [회전→로그인→게시]를 하도록.
    renderPublish();
    await userEvent.click(await screen.findByText("invest_king7")); // drop forum
    await userEvent.click(screen.getByText("money_lab"));
    await addNaverCafe(11111111);
    await userEvent.click(
      await screen.findByRole(
        "button",
        { name: /^게시 \(1\)/ },
        { timeout: 3000 },
      ),
    );
    await screen.findByText(/대기열에 추가됨/, undefined, { timeout: 3000 });
    // 게시 plan에 그 계정 로그인 스펙이 동봉된다(force=재로그인, useAdb=IP 회전).
    expect(enqueuedPlan().login).toEqual([
      {
        accountId: "money_lab",
        platform: "naver",
        headless: false,
        useAdb: true,
        force: true,
      },
    ]);
    // 자격증명이 저장돼야 백엔드가 게시 직전에 로그인할 수 있다.
    expect(ipcBackend).toHaveBeenCalledWith("save_accounts", {
      accounts: [
        { id: "money_lab", password: "mlab2024!!", label: "money_lab" },
      ],
    });
  });

  it("adds a cafe target from a board link — no joined-cafe lookup (cookie-free)", async () => {
    // 카페는 게시판 링크로 추가한다(시드 로그인 없이). 게시판 목록은 쿠키 필수라
    // 모달은 list_joined_cafes를 절대 부르지 않는다(회귀 가드).
    renderPublish();
    await userEvent.click(await screen.findByText("money_lab"));
    await addNaverCafe(11111111);
    // 링크에서 파싱된 카페·게시판이 칩으로 떠야 한다(제거 버튼 aria-label로 단언 —
    // 같은 라벨이 열린 Select 옵션에도 있어 텍스트로는 중복됨).
    expect(
      await screen.findByLabelText("카페 11111111 · 게시판 1 제거"),
    ).toBeInTheDocument();
    expect(ipcBackend).not.toHaveBeenCalledWith(
      "list_joined_cafes",
      expect.anything(),
    );
  });

  it("deselects an account when its row is clicked again", async () => {
    renderPublish();
    expect(await screen.findByText("1개")).toBeInTheDocument();
    // the first usable account (a1) is preselected; click its row to deselect
    await userEvent.click(screen.getByText("invest_king7"));
    expect(await screen.findByText("0개")).toBeInTheDocument();
  });

  it("toggles selection when the checkbox itself is clicked (exactly once)", async () => {
    renderPublish();
    // invest_king7 (a1) is preselected → its checkbox is the only checked one.
    expect(await screen.findByText("1개")).toBeInTheDocument();
    const checkedBoxes = (await screen.findAllByRole("checkbox")).filter(
      (b) => (b as HTMLInputElement).checked,
    );
    expect(checkedBoxes).toHaveLength(1);
    // Clicking the checkbox itself must register a single toggle → deselected.
    // (A double-toggle from the checkbox + the row bubbling would leave it at 1개.)
    await userEvent.click(checkedBoxes[0]!);
    expect(await screen.findByText("0개")).toBeInTheDocument();
    // And clicking it again re-selects — the control is not stuck.
    const box = (await screen.findAllByRole("checkbox"))[0]!;
    await userEvent.click(box);
    expect(await screen.findByText("1개")).toBeInTheDocument();
  });

  it("selects every visible account and expands the destinations", async () => {
    renderPublish();
    await userEvent.click(
      await screen.findByRole("button", { name: /보이는 계정 전체/ }),
    );
    // 15 accounts − 1 errored = 14 selectable
    expect(await screen.findByText("14개")).toBeInTheDocument();
    // forum + naver + band → 3 platform marks in the footer pill row
    expect(
      screen.getByRole("button", { name: /종목 선택/ }),
    ).toBeInTheDocument();
  });

  it("edits the link override when the doc has a link token", async () => {
    renderPublish();
    const input = await screen.findByPlaceholderText(
      "비우면 종목별 시세 링크 자동 삽입",
    );
    await userEvent.type(input, "https://x.test");
    expect(input).toHaveValue("https://x.test");
  });

  it("선택한 종목(시드 밖)도 이름으로 칩에 표시된다", async () => {
    renderPublish();
    // forum 계정이 기본 선택돼 있어 종목 칩 영역이 보인다.
    await userEvent.click(
      await screen.findByRole("button", { name: /종목 선택/ }),
    );
    // list_stocks 밖의 ETF(0193T0)를 검색해 고른다.
    const search = await screen.findByPlaceholderText("종목명 또는 코드 검색");
    await userEvent.type(search, "0193T0");
    await userEvent.click(
      await screen.findByText("KODEX SK하이닉스단일종목레버리지"),
    );
    await userEvent.click(await screen.findByRole("button", { name: /적용/ }));
    // onConfirm으로 받은 이름이 칩에 그대로 표시된다(코드가 아니라 이름). 기본 종목이 없어
    // (#267-11) 이 종목이 유일하면 변수 미리보기 예시에도 같은 이름이 떠 2곳에서 매칭되므로
    // findAllByText로 "한 곳 이상"을 확인한다.
    expect(
      (await screen.findAllByText("KODEX SK하이닉스단일종목레버리지")).length,
    ).toBeGreaterThan(0);
  });

  it("removes a selected stock chip", async () => {
    renderPublish();
    await pickStock();
    // "삼성전자" shows in the chip and in the variable-preview example;
    // the chip (first in DOM) carries the remove button.
    const chip = (await screen.findAllByText("삼성전자"))[0]!.closest("div")!;
    await userEvent.click(within(chip).getByRole("button"));
    expect(screen.queryByText("삼성전자")).not.toBeInTheDocument();
  });

  it("shows the custom date/time picker once 예약 게시 is chosen", async () => {
    renderPublish();
    await userEvent.click(await screen.findByText("예약 게시"));
    // Native date/time inputs are replaced by the DateTimePicker trigger, which
    // renders the scheduled moment (defaulting to now) as a friendly label.
    expect(
      await screen.findByRole("button", { name: /\d+월 \d+일.*\d\d:\d\d/ }),
    ).toBeInTheDocument();
  });

  it("adds the post to the scheduled queue when 예약 is confirmed", async () => {
    renderPublish();
    await pickStock();
    await userEvent.click(await screen.findByText("예약 게시"));
    await userEvent.click(
      await screen.findByRole("button", { name: /^예약 \(\d+\)/ }),
    );
    const scheduled = (await ipcBackend("list_queue_scheduled")) as {
      title: string;
    }[];
    expect(scheduled.some((q) => q.title === postDoc.title)).toBe(true);
  });

  it("freezes a post-mode plan (flattened body, naver target) when 예약 is confirmed", async () => {
    renderPublish();
    // forum(a1)을 빼고 naver(money_lab) 선택 → 카페/게시판이 정해지면 naver job 1건.
    await userEvent.click(await screen.findByText("invest_king7"));
    await userEvent.click(screen.getByText("money_lab"));
    await addNaverCafe(11111111);
    // 카페·게시판이 정해지면 naver job 1건 → 버튼 카운트가 1로 오른다(아직 now 모드).
    await screen.findByRole(
      "button",
      { name: /^게시 \(1\)/ },
      { timeout: 3000 },
    );
    await userEvent.click(await screen.findByText("예약 게시"));
    await userEvent.click(
      await screen.findByRole("button", { name: /^예약 \(1\)/ }),
    );
    const call = ipcBackend.mock.calls.find(
      (c) => c[0] === "add_queue_scheduled",
    );
    expect(call).toBeDefined();
    const item = (call?.[1] as { item: { plan?: unknown } }).item;
    expect(item.plan).toEqual(
      expect.objectContaining({
        postId: postDoc.id,
        kind: "post",
        title: postDoc.title,
        // 본문은 예약 시점에 평문화돼 동결된다(HTML 태그 제거).
        bodyText: "#{종목명} 본문 #{링크}",
        comments: [],
        naver: [
          expect.objectContaining({
            accountId: "money_lab",
            cafe: "11111111",
            menuId: 1,
            boardType: "",
          }),
        ],
        forum: [],
      }),
    );
    // post 모드 naver 대상엔 댓글 스펙이 없어야 한다.
    const plan = item.plan as { naver: { commentTarget?: unknown }[] };
    expect(plan.naver[0]?.commentTarget).toBeUndefined();
  });

  it("includes a url comment spec in a comment-mode scheduled plan", async () => {
    const commentDoc: LibraryPost = {
      id: "lcs",
      title: "URL 댓글 예약",
      kind: "comment",
      updated: "방금 전",
      words: 30,
      status: "ready",
      excerpt: "요약",
      commentTarget: "url",
      commentUrl: "https://cafe.naver.com/ca-fe/cafes/31732304/articles/9",
      comments: ["댓글1", "댓글2"],
    };
    renderPublish({ doc: commentDoc });
    await userEvent.click(await screen.findByText("invest_king7"));
    await userEvent.click(screen.getByText("money_lab"));
    await userEvent.click(await screen.findByText("예약 게시"));
    await userEvent.click(
      await screen.findByRole("button", { name: /^예약 \(1\)/ }),
    );
    const call = ipcBackend.mock.calls.find(
      (c) => c[0] === "add_queue_scheduled",
    );
    expect(call).toBeDefined();
    const plan = (call?.[1] as { item: { plan: { naver: unknown[] } } }).item
      .plan;
    expect(plan.naver).toEqual([
      expect.objectContaining({
        accountId: "money_lab",
        commentTarget: {
          mode: "url",
          cafeId: 31732304,
          articleId: 9,
        },
      }),
    ]);
  });

  it("forum '특정 게시글' 댓글: 종목 선택 없이 게시 버튼이 활성화되고 plan.forum에 글 URL이 동결된다", async () => {
    // 종목토론방 글 URL은 code(035720)가 URL 안에 있어 종목 선택이 필요 없다. 기본 stockCodes는
    // 005930이지만, url 댓글 잡은 그 선택을 무시하고 URL의 035720을 대상으로 삼아야 한다 —
    // code가 005930이 아니라 035720으로 동결되는 것으로 "URL이 곧 대상"임을 증명한다.
    const forumUrl =
      "https://stock.naver.com/domestic/stock/035720/discussion/421063210?chip=all";
    const commentDoc: LibraryPost = {
      id: "lfu",
      title: "종토방 URL 댓글",
      kind: "comment",
      updated: "방금 전",
      words: 20,
      status: "ready",
      excerpt: "요약",
      commentTarget: "url",
      commentUrl: forumUrl,
      comments: ["댓글1"],
    };
    renderPublish({ doc: commentDoc });
    // 기본 forum 계정이 선택돼 있다. 종목을 따로 고르지 않아도 게시 버튼이 활성화된다(잡 1건).
    // (이전엔 카페 URL 파서로만 대상을 풀어, 종토방 URL은 대상 미해석 → 버튼 비활성이었다.)
    const publishBtn = await screen.findByRole(
      "button",
      { name: /^게시 \(1\)/ },
      { timeout: 3000 },
    );
    expect(publishBtn).toBeEnabled();

    await userEvent.click(await screen.findByText("예약 게시"));
    await userEvent.click(
      await screen.findByRole("button", { name: /^예약 \(1\)/ }),
    );
    const call = ipcBackend.mock.calls.find(
      (c) => c[0] === "add_queue_scheduled",
    );
    expect(call).toBeDefined();
    const plan = (
      call?.[1] as { item: { plan: { forum: unknown[]; naver: unknown[] } } }
    ).item.plan;
    expect(plan.naver).toEqual([]);
    expect(plan.forum).toEqual([
      expect.objectContaining({
        accountId: "invest_king7",
        code: "035720",
        commentUrl: forumUrl,
      }),
    ]);
  });

  it("forum 특정글 '나눠서 게시'(#403): 댓글수==계정수면 활성, 클릭 시 forumCommentDistribute=true로 적재", async () => {
    const forumUrl =
      "https://stock.naver.com/domestic/stock/035720/discussion/421063210?chip=all";
    const commentDoc: LibraryPost = {
      id: "lfd1",
      title: "종토 나눠서",
      kind: "comment",
      updated: "방금 전",
      words: 20,
      status: "ready",
      excerpt: "요약",
      commentTarget: "url",
      commentUrl: forumUrl,
      comments: ["댓글1"], // 댓글 1개 == 기본 forum 계정 1개
    };
    renderPublish({ doc: commentDoc });
    const splitBtn = await screen.findByRole(
      "button",
      { name: "나눠서 게시" },
      { timeout: 3000 },
    );
    expect(splitBtn).toBeEnabled();
    await userEvent.click(splitBtn);
    const call = ipcBackend.mock.calls.find((c) => c[0] === "add_queue_now");
    expect(call).toBeDefined();
    const plan = (
      call?.[1] as { item: { plan: { forumCommentDistribute?: boolean } } }
    ).item.plan;
    expect(plan.forumCommentDistribute).toBe(true);
  });

  it("forum 특정글 '나눠서 게시'(설계서 §3): 댓글수 > 계정수면 활성(>= 조건으로 완화)", async () => {
    const forumUrl =
      "https://stock.naver.com/domestic/stock/035720/discussion/421063210?chip=all";
    const commentDoc: LibraryPost = {
      id: "lfd2",
      title: "종토 나눠서 댓글 더 많음",
      kind: "comment",
      updated: "방금 전",
      words: 20,
      status: "ready",
      excerpt: "요약",
      commentTarget: "url",
      commentUrl: forumUrl,
      comments: ["댓글1", "댓글2"], // 댓글 2개 >= 기본 forum 계정 1개 → 활성(설계서 §3)
    };
    renderPublish({ doc: commentDoc });
    const splitBtn = await screen.findByRole(
      "button",
      { name: "나눠서 게시" },
      { timeout: 3000 },
    );
    expect(splitBtn).toBeEnabled();
    await userEvent.click(splitBtn);
    const call = ipcBackend.mock.calls.find((c) => c[0] === "add_queue_now");
    expect(call).toBeDefined();
    const plan = (
      call?.[1] as { item: { plan: { forumCommentDistribute?: boolean } } }
    ).item.plan;
    expect(plan.forumCommentDistribute).toBe(true);
  });

  it("forum '특정 게시글' 댓글: 여러 글 URL을 넣으면 각 글마다 댓글 잡이 만들어진다(다중 url)", async () => {
    // 종토방 글 URL 2개 → plan.forum에 글마다 잡 1개(총 2개), 각자 자신의 URL·종목코드로 동결돼
    // 두 글 모두 댓글이 달려야 한다(카페 다중 url과 대칭, #특정게시글 다중링크).
    const url1 =
      "https://stock.naver.com/domestic/stock/035720/discussion/421063210?chip=all";
    const url2 =
      "https://stock.naver.com/domestic/stock/005930/discussion/421099999?chip=all";
    const commentDoc: LibraryPost = {
      id: "lfm",
      title: "종토방 다중 URL 댓글",
      kind: "comment",
      updated: "방금 전",
      words: 20,
      status: "ready",
      excerpt: "요약",
      commentTarget: "url",
      commentUrls: [url1, url2],
      comments: ["댓글1"],
    };
    renderPublish({ doc: commentDoc });
    // 기본 forum 계정(invest_king7)이 선택돼 있다. 두 URL이 곧 대상이라 종목 선택 없이 잡 2개.
    const publishBtn = await screen.findByRole(
      "button",
      { name: /^게시 \(2\)/ },
      { timeout: 3000 },
    );
    expect(publishBtn).toBeEnabled();

    await userEvent.click(await screen.findByText("예약 게시"));
    await userEvent.click(
      await screen.findByRole("button", { name: /^예약 \(2\)/ }),
    );
    const call = ipcBackend.mock.calls.find(
      (c) => c[0] === "add_queue_scheduled",
    );
    expect(call).toBeDefined();
    const plan = (
      call?.[1] as {
        item: { plan: { forum: { commentUrl?: string; code?: string }[] } };
      }
    ).item.plan;
    expect(plan.forum).toHaveLength(2);
    // 링크별로 자신의 URL·종목코드가 동결돼야 한다(둘 다 댓글이 달리도록).
    expect(plan.forum.map((t) => t.commentUrl)).toEqual([url1, url2]);
    expect(plan.forum.map((t) => t.code)).toEqual(["035720", "005930"]);
  });

  it("band '특정 게시글' 댓글: 밴드 선택 없이 게시 버튼이 활성화되고 plan.band에 글 URL+url 스펙이 동결된다", async () => {
    const bandUrl = "https://www.band.us/band/103043410/post/57";
    const commentDoc: LibraryPost = {
      id: "lbu",
      title: "밴드 URL 댓글",
      kind: "comment",
      updated: "방금 전",
      words: 20,
      status: "ready",
      excerpt: "요약",
      commentTarget: "url",
      commentUrl: bandUrl,
      comments: ["댓글1"],
    };
    renderPublish({ doc: commentDoc });
    await userEvent.click(await screen.findByText("invest_king7")); // 기본 forum 해제
    await userEvent.click(screen.getByText("value_invest")); // band 계정 선택
    // '밴드 선택'(멤버십 추가) 없이도 URL이 곧 대상이라 게시 버튼이 활성화된다.
    const publishBtn = await screen.findByRole(
      "button",
      { name: /^게시 \(1\)/ },
      { timeout: 3000 },
    );
    expect(publishBtn).toBeEnabled();

    await userEvent.click(await screen.findByText("예약 게시"));
    await userEvent.click(
      await screen.findByRole("button", { name: /^예약 \(1\)/ }),
    );
    const call = ipcBackend.mock.calls.find(
      (c) => c[0] === "add_queue_scheduled",
    );
    expect(call).toBeDefined();
    const plan = (
      call?.[1] as { item: { plan: { band: unknown[]; forum: unknown[] } } }
    ).item.plan;
    expect(plan.forum).toEqual([]);
    expect(plan.band).toEqual([
      expect.objectContaining({
        accountId: "value_invest",
        link: bandUrl,
        commentTarget: { mode: "url" },
      }),
    ]);
  });

  it("여러 링크(commentUrls)를 넣으면 각 링크의 글마다 댓글 대상이 만들어진다(카페 다중 url)", async () => {
    // 특정 게시글 댓글에 카페 글 링크 2개 → plan.naver에 글마다 대상 1개(총 2개), 각자
    // 자신의 articleId로 동결된다(각 링크의 글에 모두 댓글이 달리도록).
    const commentDoc: LibraryPost = {
      id: "lmu",
      title: "다중 링크 댓글",
      kind: "comment",
      updated: "방금 전",
      words: 20,
      status: "ready",
      excerpt: "요약",
      commentTarget: "url",
      commentUrls: [
        "https://cafe.naver.com/ca-fe/cafes/31732304/articles/9",
        "https://cafe.naver.com/ca-fe/cafes/31732304/articles/15",
      ],
      comments: ["댓글1"],
    };
    renderPublish({ doc: commentDoc });
    await userEvent.click(await screen.findByText("invest_king7")); // 기본 forum 해제
    await userEvent.click(screen.getByText("money_lab")); // naver(cafe) 계정
    // 두 링크가 곧 대상이라 게시판 선택 없이 게시 버튼이 활성화된다(잡 2개).
    const publishBtn = await screen.findByRole(
      "button",
      { name: /^게시 \(2\)/ },
      { timeout: 3000 },
    );
    expect(publishBtn).toBeEnabled();

    await userEvent.click(await screen.findByText("예약 게시"));
    await userEvent.click(
      await screen.findByRole("button", { name: /^예약 \(2\)/ }),
    );
    const call = ipcBackend.mock.calls.find(
      (c) => c[0] === "add_queue_scheduled",
    );
    expect(call).toBeDefined();
    const plan = (call?.[1] as { item: { plan: { naver: unknown[] } } }).item
      .plan;
    expect(plan.naver).toHaveLength(2);
    // 링크별로 다른 articleId(9, 15)가 각 대상에 동결돼야 한다.
    const specs = (
      plan.naver as { commentTarget?: { articleId?: number } }[]
    ).map((t) => t.commentTarget?.articleId);
    expect(specs).toEqual(expect.arrayContaining([9, 15]));
    expect(plan.naver).toEqual([
      expect.objectContaining({
        accountId: "money_lab",
        commentTarget: { mode: "url", cafeId: 31732304, articleId: 9 },
      }),
      expect.objectContaining({
        accountId: "money_lab",
        commentTarget: { mode: "url", cafeId: 31732304, articleId: 15 },
      }),
    ]);
  });

  it("includes a latest comment spec (cafeId + count) in a both-mode scheduled plan", async () => {
    const bothDoc: LibraryPost = {
      id: "lbs",
      title: "글+댓글 예약",
      kind: "both",
      updated: "방금 전",
      words: 100,
      status: "ready",
      excerpt: "요약",
      body: "<p>본문</p>",
      commentTarget: "latest",
      commentCount: 3,
      comments: ["좋네요"],
    };
    renderPublish({ doc: bothDoc });
    await userEvent.click(await screen.findByText("invest_king7"));
    await userEvent.click(screen.getByText("money_lab"));
    await addNaverCafe(11111111);
    await screen.findByRole(
      "button",
      { name: /^게시 \(1\)/ },
      { timeout: 3000 },
    );
    await userEvent.click(await screen.findByText("예약 게시"));
    await userEvent.click(
      await screen.findByRole("button", { name: /^예약 \(1\)/ }),
    );
    const call = ipcBackend.mock.calls.find(
      (c) => c[0] === "add_queue_scheduled",
    );
    expect(call).toBeDefined();
    const plan = (call?.[1] as { item: { plan: { naver: unknown[] } } }).item
      .plan;
    expect(plan.naver).toEqual([
      expect.objectContaining({
        accountId: "money_lab",
        commentTarget: { mode: "latest", count: 3, cafeId: 11111111 },
      }),
    ]);
  });

  it("includes naver/forum/band targets in a scheduled plan", async () => {
    renderPublish();
    // 기본 forum(a1) 유지 + naver(money_lab) + band(value_invest) 선택.
    await userEvent.click(await screen.findByText("money_lab"));
    await userEvent.click(screen.getByText("value_invest"));
    await addNaverCafe(11111111);

    // 밴드 링크를 저장하고 조회된 밴드(데일밴드)를 골라 게시 대상으로 추가한다. naver 카페
    // Select가 콤보박스 0이므로 밴드 Select는 1이다.
    const linkInput = screen.getByLabelText("밴드 링크");
    const saveBtn = screen.getByRole("button", { name: "저장" });
    await userEvent.type(linkInput, "https://band.us/band/103043410");
    await waitFor(() => expect(saveBtn).toBeEnabled());
    await userEvent.click(saveBtn);
    await waitFor(() => expect(linkInput).toHaveValue(""));
    await screen.findByPlaceholderText("게시할 밴드 선택");
    await pickOption(1, "데일밴드");
    await screen.findByLabelText("데일밴드 제거"); // 칩 등장 확인

    await pickStock(); // 기본 forum(a1)에 종목 부여 → forum 잡 1건 복원(#267-11)
    // forum + naver + band = 3곳.
    await screen.findByRole(
      "button",
      { name: /^게시 \(3\)/ },
      { timeout: 3000 },
    );
    await userEvent.click(await screen.findByText("예약 게시"));
    await userEvent.click(
      await screen.findByRole("button", { name: /^예약 \(3\)/ }),
    );
    const call = ipcBackend.mock.calls.find(
      (c) => c[0] === "add_queue_scheduled",
    );
    expect(call).toBeDefined();
    const plan = (
      call?.[1] as {
        item: {
          plan: { naver: unknown[]; forum: unknown[]; band: unknown[] };
        };
      }
    ).item.plan;
    expect(plan.naver).toHaveLength(1);
    expect(plan.forum).toEqual([
      expect.objectContaining({ accountId: "invest_king7", code: "005930" }),
    ]);
    // 밴드도 이제 plan에 실린다 — 즉시 게시와 동일하게 밴드명으로 가입 링크를 동결한다.
    expect(plan.band).toEqual([
      expect.objectContaining({
        accountId: "value_invest",
        name: "데일밴드",
        link: "https://band.us/band/103043410",
      }),
    ]);
  });

  it("freezes a band comment target (popular + count) in a comment-mode scheduled plan", async () => {
    const commentDoc: LibraryPost = {
      id: "lbcs",
      title: "밴드 댓글 예약",
      kind: "comment",
      updated: "방금 전",
      words: 20,
      status: "ready",
      excerpt: "요약",
      commentTarget: "popular",
      commentCount: 5,
      comments: ["좋아요"],
    };
    renderPublish({ doc: commentDoc });
    // 밴드 계정 선택 + 링크 저장 + 게시할 밴드 선택.
    await userEvent.click(await screen.findByText("value_invest"));
    await pickStock(); // 기본 forum(a1)에 종목 부여(#267-11)
    const linkInput = screen.getByLabelText("밴드 링크");
    const saveBtn = screen.getByRole("button", { name: "저장" });
    await userEvent.type(linkInput, "https://band.us/band/103043410");
    await waitFor(() => expect(saveBtn).toBeEnabled());
    await userEvent.click(saveBtn);
    await waitFor(() => expect(linkInput).toHaveValue(""));
    await screen.findByPlaceholderText("게시할 밴드 선택");
    await pickOption(0, "데일밴드");
    await screen.findByLabelText("데일밴드 제거");

    await userEvent.click(await screen.findByText("예약 게시"));
    await userEvent.click(
      await screen.findByRole("button", { name: /^예약 \(\d+\)/ }),
    );
    const call = ipcBackend.mock.calls.find(
      (c) => c[0] === "add_queue_scheduled",
    );
    expect(call).toBeDefined();
    const plan = (
      call?.[1] as { item: { plan: { band: { commentTarget?: unknown }[] } } }
    ).item.plan;
    // 댓글 전용 예약: 밴드 대상에 최신/인기 spec이 동결돼, 워커가 band_comment로 간다
    // (새 글을 쓰는 band_publish가 아니라 → 리더 승인제 밴드 1003 회피). cafeId는 없다.
    expect(plan.band[0]?.commentTarget).toEqual({ mode: "popular", count: 5 });
  });

  it("drops band targets from a url-mode comment plan (band has no url support)", async () => {
    const commentDoc: LibraryPost = {
      id: "lcsu",
      title: "URL 댓글 + 밴드 예약",
      kind: "comment",
      updated: "방금 전",
      words: 30,
      status: "ready",
      excerpt: "요약",
      commentTarget: "url",
      commentUrl: "https://cafe.naver.com/ca-fe/cafes/31732304/articles/9",
      comments: ["댓글1"],
    };
    renderPublish({ doc: commentDoc });
    // 카페 url 대상(money_lab) + 밴드(value_invest)를 함께 선택한다.
    await userEvent.click(await screen.findByText("money_lab"));
    await userEvent.click(await screen.findByText("value_invest"));
    await pickStock(); // 기본 forum(a1)에 종목 부여(#267-11)
    const linkInput = screen.getByLabelText("밴드 링크");
    const saveBtn = screen.getByRole("button", { name: "저장" });
    await userEvent.type(linkInput, "https://band.us/band/103043410");
    await waitFor(() => expect(saveBtn).toBeEnabled());
    await userEvent.click(saveBtn);
    await waitFor(() => expect(linkInput).toHaveValue(""));
    await screen.findByPlaceholderText("게시할 밴드 선택");
    await pickOption(0, "데일밴드");
    await screen.findByLabelText("데일밴드 제거");

    await userEvent.click(await screen.findByText("예약 게시"));
    await userEvent.click(
      await screen.findByRole("button", { name: /^예약 \(\d+\)/ }),
    );
    const call = ipcBackend.mock.calls.find(
      (c) => c[0] === "add_queue_scheduled",
    );
    expect(call).toBeDefined();
    const plan = (
      call?.[1] as { item: { plan: { naver: unknown[]; band: unknown[] } } }
    ).item.plan;
    // 카페 url 대상은 실리고, 밴드는 url 미지원이라 제외된다(runNow가 실패로 막는 것과
    // 일관 — latest로 둔갑해 엉뚱한 최신글에 댓글이 달리는 것을 방지).
    expect(plan.naver.length).toBeGreaterThan(0);
    expect(plan.band).toEqual([]);
  });

  it("blocks scheduling when a band is selected but none is picked", async () => {
    renderPublish();
    // 밴드 계정만 선택하고 예약 모드로 전환 — 게시할 밴드는 미선택.
    await userEvent.click(await screen.findByText("value_invest")); // band a7
    await userEvent.click(await screen.findByText("예약 게시"));
    // bandReady 통일: 예약이어도 밴드 미선택이면 예약 버튼이 비활성이어야 한다.
    expect(
      await screen.findByRole("button", { name: /^예약 \(\d+\)/ }),
    ).toBeDisabled();
  });

  it("enqueues a 'both' job with the full comment pool frozen", async () => {
    const bothDoc: LibraryPost = {
      id: "lb",
      title: "실적 점검 + 댓글",
      kind: "both",
      updated: "방금 전",
      words: 100,
      status: "ready",
      excerpt: "요약",
      body: "<p>본문</p>",
      comments: ["좋네요", "굿"],
    };
    renderPublish({ doc: bothDoc });
    await userEvent.click(await screen.findByText("invest_king7")); // drop forum
    await userEvent.click(screen.getByText("money_lab")); // a5 naver
    await addNaverCafe(11111111);
    await userEvent.click(
      await screen.findByRole(
        "button",
        { name: /^게시 \(1\)/ },
        { timeout: 3000 },
      ),
    );
    // 즉시 게시는 동결된 plan을 큐에 적재한다(both): plan.kind=both, 댓글 풀 전체를 싣고,
    // 방금 쓴 글에 self-comment를 다는 일은 워커가 맡는다(#198).
    const plan = enqueuedPlan();
    expect(plan.kind).toBe("both");
    expect(plan.naver).toHaveLength(1);
    expect(plan.naver[0]).toEqual(
      expect.objectContaining({ accountId: "money_lab", cafe: "11111111" }),
    );
    expect(plan.comments).toEqual(["좋네요", "굿"]);
    // 큐에 적재되면 결과 행은 "추가됨"으로 뜬다(실제 댓글 게시·집계는 워커가 한다).
    expect(
      await screen.findByText(/대기열에 추가됨/, undefined, { timeout: 3000 }),
    ).toBeInTheDocument();
  });

  it("글+댓글(both) 모드에서 닉네임 랜덤 체크박스가 보인다(설계서 §2)", async () => {
    // 댓글 전용뿐 아니라 글+댓글 모드에서도 닉네임 랜덤을 켤 수 있어야 한다(게이트 완화).
    renderPublish({
      doc: { ...postDoc, kind: "both", comments: ["댓글1"] },
    });
    expect(await screen.findByText("닉네임 랜덤")).toBeInTheDocument();
    expect(
      await screen.findByRole("checkbox", { name: "랜덤" }),
    ).toBeInTheDocument();
  });

  it("닉네임 랜덤을 체크하면 계정별 변경 잔여 횟수가 뜬다(forum_nickname_remaining 모킹)", async () => {
    // 체크 전에는 조회하지 않고(불필요 API 방지), 체크하면 forum 계정별 잔여 횟수를 보여준다.
    // 기본 forum 계정(invest_king7) 하나만 선택돼 있어 접두사 없이 "변경 기회 N회 남음"으로 뜬다.
    renderPublish({
      doc: { ...postDoc, kind: "both", comments: ["댓글1"] },
    });
    const checkbox = await screen.findByRole("checkbox", { name: "랜덤" });
    expect(screen.queryByText(/변경 기회/)).not.toBeInTheDocument();
    await userEvent.click(checkbox);
    // 모킹된 forum_nickname_remaining=3 → "변경 기회 3회 남음".
    expect(
      await screen.findByText("변경 기회 3회 남음"),
    ).toBeInTheDocument();
  });

  it("enqueues a 'both' job with an empty comment pool", async () => {
    // both 문서이지만 댓글 풀이 비면 plan.comments는 빈 배열로 적재된다(워커가 글만 게시).
    // 결과 행은 "추가됨"으로 떠야 한다 — 빈 댓글 때문에 실패처럼 접히면 안 된다.
    const bothNoComments: LibraryPost = {
      id: "lbnc",
      title: "글만 있는 글+댓글",
      kind: "both",
      updated: "방금 전",
      words: 100,
      status: "ready",
      excerpt: "요약",
      body: "<p>본문</p>",
      comments: [],
    };
    renderPublish({ doc: bothNoComments });
    await userEvent.click(await screen.findByText("invest_king7")); // drop forum
    await userEvent.click(screen.getByText("money_lab")); // a5 naver
    await addNaverCafe(11111111);
    await userEvent.click(
      await screen.findByRole(
        "button",
        { name: /^게시 \(1\)/ },
        { timeout: 3000 },
      ),
    );
    // 빈 댓글 풀로 적재되고, 결과는 "추가됨"으로 떠야 한다(실패처럼 접히면 안 된다).
    const plan = enqueuedPlan();
    expect(plan.kind).toBe("both");
    expect(plan.comments).toEqual([]);
    expect(
      await screen.findByText(/대기열에 추가됨/, undefined, {
        timeout: 3000,
      }),
    ).toBeInTheDocument();
    expect(screen.queryByText(/댓글 없음/)).not.toBeInTheDocument();
  });

  it("comments on a pasted article URL in 'comment' mode", async () => {
    const commentDoc: LibraryPost = {
      id: "lc",
      title: "URL 댓글 세트",
      kind: "comment",
      updated: "방금 전",
      words: 30,
      status: "ready",
      excerpt: "요약",
      commentTarget: "url",
      commentUrl: "https://cafe.naver.com/ca-fe/cafes/31732304/articles/9",
      comments: ["댓글1", "댓글2"],
    };
    renderPublish({ doc: commentDoc });
    await userEvent.click(await screen.findByText("invest_king7")); // drop forum
    await userEvent.click(screen.getByText("money_lab")); // a5 naver
    // No board pick needed in comment mode — the job is the account itself.
    await userEvent.click(
      await screen.findByRole(
        "button",
        { name: /^게시 \(1\)/ },
        { timeout: 3000 },
      ),
    );
    // url 댓글 대상은 plan.naver의 commentTarget(url)에 박제돼 큐에 적재된다. 댓글 분배
    // (어느 계정이 어떤 댓글)는 워커가 RNG로 정하므로 여기선 단언하지 않는다(#98).
    const plan = enqueuedPlan();
    expect(plan.naver).toHaveLength(1);
    expect(plan.naver[0]?.accountId).toBe("money_lab");
    expect(plan.naver[0]?.commentTarget).toEqual(
      expect.objectContaining({ mode: "url", cafeId: 31732304, articleId: 9 }),
    );
    expect(plan.comments).toEqual(["댓글1", "댓글2"]);
  });

  it("freezes a 'latest' commentTarget spec (worker expands at run time)", async () => {
    const latestDoc: LibraryPost = {
      id: "ll",
      title: "최신글 댓글 세트",
      kind: "comment",
      updated: "방금 전",
      words: 30,
      status: "ready",
      excerpt: "요약",
      commentTarget: "latest",
      commentCount: 3,
      comments: ["댓글1", "댓글2"],
    };
    renderPublish({ doc: latestDoc });
    await userEvent.click(await screen.findByText("invest_king7")); // drop forum
    await userEvent.click(screen.getByText("money_lab")); // a5 naver
    // 주식투자연구소 카페 (cafeId 11111111) has 10 latest articles in the mock.
    // 개수(3)는 템플릿(doc.commentCount)에서 동결 — 게시 모달엔 개수 UI가 없다.
    await addNaverCafe(11111111);
    await userEvent.click(
      await screen.findByRole(
        "button",
        { name: /^게시 \(1\)/ },
        { timeout: 3000 },
      ),
    );
    // 최신글 대상은 프론트가 펼치지 않고 동결 스펙(mode:latest, count, cafeId)으로 적재한다.
    // 실행 시점에 상위 N개를 조회·분배하는 일은 워커가 맡는다(예약 게시와 동일 경로, #98/#198).
    const plan = enqueuedPlan();
    expect(plan.naver).toHaveLength(1);
    expect(plan.naver[0]?.accountId).toBe("money_lab");
    expect(plan.naver[0]?.commentTarget).toEqual(
      expect.objectContaining({ mode: "latest", count: 3, cafeId: 11111111 }),
    );
    expect(plan.comments).toEqual(["댓글1", "댓글2"]);
  });

  it("freezes a 'popular' commentTarget spec instead of querying upfront", async () => {
    const popularDoc: LibraryPost = {
      id: "lp",
      title: "인기글 댓글 세트",
      kind: "comment",
      updated: "방금 전",
      words: 30,
      status: "ready",
      excerpt: "요약",
      commentTarget: "popular",
      commentCount: 1,
      comments: ["좋아요"],
    };
    renderPublish({ doc: popularDoc });
    await userEvent.click(await screen.findByText("invest_king7"));
    await userEvent.click(screen.getByText("money_lab"));
    await addNaverCafe(11111111);
    await userEvent.click(
      await screen.findByRole(
        "button",
        { name: /^게시 \(1\)/ },
        { timeout: 3000 },
      ),
    );
    // popular 대상도 프론트가 펼치지 않고 동결 스펙(mode:popular)으로 적재한다 — 인기글
    // 조회·정렬은 워커가 실행 시점에 한다. 프론트는 더 이상 list_cafe_articles를 부르지 않는다.
    expect(ipcBackend).not.toHaveBeenCalledWith(
      "list_cafe_articles",
      expect.anything(),
    );
    const plan = enqueuedPlan();
    expect(plan.naver).toHaveLength(1);
    expect(plan.naver[0]?.commentTarget).toEqual(
      expect.objectContaining({ mode: "popular", count: 1, cafeId: 11111111 }),
    );
  });

  it("freezes a per-account latest spec for each selected naver account", async () => {
    // 다계정 댓글 전용: 각 계정이 고른 카페가 동결 스펙(mode:latest, count, cafeId)으로
    // plan.naver에 실린다. 프론트는 더 이상 글목록을 미리 조회하지 않으므로(상위 N개 펼침은
    // 워커가 실행 시점에) list_cafe_articles는 불리지 않는다.
    const latestDoc: LibraryPost = {
      id: "lmulti",
      title: "최신글 댓글 — 다계정",
      kind: "comment",
      updated: "방금 전",
      words: 30,
      status: "ready",
      excerpt: "요약",
      commentTarget: "latest",
      commentCount: 1,
      comments: ["댓글"],
    };
    renderPublish({ doc: latestDoc });
    await userEvent.click(await screen.findByText("invest_king7")); // drop forum
    await userEvent.click(screen.getByText("money_lab")); // a5 naver
    await userEvent.click(screen.getByText("insight_note")); // a10 naver
    // 카페 대상은 전역(밴드 미러)이라 게시판 1개를 추가하면 두 계정 모두에 적용된다.
    await addNaverCafe(11111111);
    await userEvent.click(
      await screen.findByRole(
        "button",
        { name: /^게시 \(2\)/ },
        { timeout: 3000 },
      ),
    );
    expect(ipcBackend).not.toHaveBeenCalledWith(
      "list_cafe_articles",
      expect.anything(),
    );
    const plan = enqueuedPlan();
    expect(plan.naver).toHaveLength(2);
    expect(plan.naver).toEqual(
      expect.arrayContaining([
        expect.objectContaining({
          accountId: "money_lab",
          commentTarget: expect.objectContaining({
            mode: "latest",
            count: 1,
            cafeId: 11111111,
          }),
        }),
        expect.objectContaining({
          accountId: "insight_note",
          commentTarget: expect.objectContaining({
            mode: "latest",
            count: 1,
            cafeId: 11111111,
          }),
        }),
      ]),
    );
  });

  it("freezes the template's commentCount (5) into the commentTarget spec", async () => {
    // 개수는 댓글 템플릿(doc.commentCount)에서 동결된 값을 쓴다 — 게시 모달엔
    // 더 이상 개수 선택 UI가 없다(중복 제거).
    const latestDoc: LibraryPost = {
      id: "l5",
      title: "최신글 5건",
      kind: "comment",
      updated: "방금 전",
      words: 30,
      status: "ready",
      excerpt: "요약",
      commentTarget: "latest",
      commentCount: 5,
      comments: ["댓글"],
    };
    renderPublish({ doc: latestDoc });
    await userEvent.click(await screen.findByText("invest_king7")); // drop forum
    await userEvent.click(screen.getByText("money_lab")); // a5 naver
    // 주식투자연구소 카페 (cafeId 11111111) has 10 latest articles in the mock.
    await addNaverCafe(11111111);
    await userEvent.click(
      await screen.findByRole(
        "button",
        { name: /^게시 \(1\)/ },
        { timeout: 3000 },
      ),
    );
    const plan = enqueuedPlan();
    // 개수(5)는 commentTarget.count로 동결돼 적재된다 — 상위 5개 펼침은 워커가 실행 시점에.
    expect(plan.naver).toHaveLength(1);
    expect(plan.naver[0]?.commentTarget).toEqual(
      expect.objectContaining({ mode: "latest", count: 5, cafeId: 11111111 }),
    );
  });

  it("blocks publish while a selected naver account hasn't picked a cafe", async () => {
    const latestDoc: LibraryPost = {
      id: "lguard",
      title: "최신글 — 카페 미선택 가드",
      kind: "comment",
      updated: "방금 전",
      words: 30,
      status: "ready",
      excerpt: "요약",
      commentTarget: "latest",
      commentCount: 1,
      comments: ["댓글"],
    };
    renderPublish({ doc: latestDoc });
    await userEvent.click(await screen.findByText("invest_king7")); // drop forum
    await userEvent.click(screen.getByText("money_lab")); // a5 naver, no cafe yet
    // No cafe picked → no comment job is built, so the publish button is (0) and
    // disabled (listTargetReady guard).
    const publish = await screen.findByRole("button", { name: /^게시 \(0\)/ });
    expect(publish).toBeDisabled();
    // Picking a cafe satisfies the guard and re-enables it.
    await addNaverCafe(11111111);
    await waitFor(() =>
      expect(screen.getByRole("button", { name: /^게시 \(1\)/ })).toBeEnabled(),
    );
  });

  it("adds a cafe target with a band account selected too", async () => {
    renderPublish();
    // Add a naver and a band account alongside the default forum one.
    await userEvent.click(await screen.findByText("money_lab")); // a5 naver
    await userEvent.click(screen.getByText("value_invest")); // a7 band
    // 카페는 게시판 링크로 추가한다(naver Select=콤보박스 0).
    await addNaverCafe(11111111);
    await pickStock(); // 기본 forum(a1)에 종목 부여 → forum 잡 1건(#267-11)
    // forum (a1) = 1 job; the naver job lands once the cafe is added → 2.
    // (밴드 계정은 선택됐지만 게시할 밴드 미선택 → 밴드 잡 0건.)
    await screen.findByRole(
      "button",
      { name: /^게시 \(2\)/ },
      { timeout: 3000 },
    );
    // 추가한 카페가 칩으로 뜨고, 밴드 링크 입력란도 함께 보인다.
    expect(
      screen.getByLabelText("카페 11111111 · 게시판 1 제거"),
    ).toBeInTheDocument();
    // 밴드 링크 입력란이 있고, 시드 밴드명(단타클럽 BAND 등)은 더 이상 없다.
    expect(screen.getByLabelText("밴드 링크")).toBeInTheDocument();
    expect(screen.queryByText("단타클럽 BAND")).not.toBeInTheDocument();
  });

  it("requires a selected band before publishing (button disabled until then)", async () => {
    renderPublish();
    await userEvent.click(await screen.findByText("value_invest")); // band a7
    // 밴드 미선택: 안내 문구 + 게시 버튼 비활성.
    expect(screen.getByText(/게시할 밴드를 선택하세요/)).toBeInTheDocument();
    expect(
      await screen.findByRole("button", { name: /^게시 \(\d+\)/ }),
    ).toBeDisabled();
  });

  it("accumulates bands from links, multi-selects, and freezes each into the plan", async () => {
    renderPublish();
    await userEvent.click(await screen.findByText("value_invest")); // band a7

    const linkInput = screen.getByLabelText("밴드 링크");
    const saveBtn = screen.getByRole("button", { name: "저장" });

    // 링크 저장 → resolveName 조회 완료(옵션 등장) 후 드롭다운에서 선택 → 칩.
    const addBand = async (link: string, name: string) => {
      await userEvent.type(linkInput, link);
      await waitFor(() => expect(saveBtn).toBeEnabled());
      await userEvent.click(saveBtn);
      await waitFor(() => expect(linkInput).toHaveValue(""));
      // 조회 완료 시 드롭다운(Select)이 활성화된다(placeholder 변경으로 확인).
      await screen.findByPlaceholderText("게시할 밴드 선택");
      await pickOption(0, name); // 드롭다운(유일 listbox)에서 밴드 선택 → 칩
    };

    await addBand("https://band.us/band/103043410", "데일밴드"); // 목: 103043410→데일밴드
    expect(await screen.findByLabelText("데일밴드 제거")).toBeInTheDocument(); // 칩
    await addBand("https://band.us/band/999", "밴드 999");
    expect(await screen.findByLabelText("밴드 999 제거")).toBeInTheDocument(); // 칩

    // 게시 → 선택한 각 밴드가 동결돼 plan.band에 적재된다(워커가 band_publish로 게시하고
    // 결과를 알림에 기록한다 — 즉시 게시도 큐를 타므로 프론트는 직접 호출하지 않는다, #198).
    await pickStock(); // 기본 forum(a1)에 종목 부여 → 게시 가능(#267-11). 계정 토글 모두 끝낸 뒤 호출.
    const publishBtn = await screen.findByRole("button", {
      name: /^게시 \(\d+\)/,
    });
    await waitFor(() => expect(publishBtn).toBeEnabled());
    await userEvent.click(publishBtn);
    const plan = enqueuedPlan();
    expect(plan.band.map((b) => ({ name: b.name, link: b.link }))).toEqual(
      expect.arrayContaining([
        { name: "데일밴드", link: "https://band.us/band/103043410" },
        { name: "밴드 999", link: "https://band.us/band/999" },
      ]),
    );
  });

  it("freezes a 'latest' band comment spec in 'comment' mode", async () => {
    // 밴드만 선택한 댓글 전용 — plan.band에 최신글 spec이 동결돼, 워커가 기존 글 조회+댓글
    // (band_comment, 새 글 band_publish 아님)로 게시한다.
    const commentDoc: LibraryPost = {
      ...postDoc,
      id: "l-band-comment",
      kind: "comment",
      comments: ["좋아요", "멋지네요"],
      commentTarget: "latest",
      commentCount: 3,
    };
    renderPublish({ doc: commentDoc });
    await userEvent.click(await screen.findByText("value_invest")); // band a7

    const linkInput = screen.getByLabelText("밴드 링크");
    const saveBtn = screen.getByRole("button", { name: "저장" });
    await userEvent.type(linkInput, "https://band.us/band/103043410");
    await waitFor(() => expect(saveBtn).toBeEnabled());
    await userEvent.click(saveBtn);
    await waitFor(() => expect(linkInput).toHaveValue(""));
    await screen.findByPlaceholderText("게시할 밴드 선택");
    await pickOption(0, "데일밴드");
    await screen.findByLabelText("데일밴드 제거");

    await pickStock(); // 기본 forum(a1)에 종목 부여 → 게시 가능(#267-11). 계정 토글 모두 끝낸 뒤 호출.
    const publishBtn = await screen.findByRole("button", {
      name: /^게시 \(\d+\)/,
    });
    await waitFor(() => expect(publishBtn).toBeEnabled());
    await userEvent.click(publishBtn);

    // 밴드 대상에 최신글 spec(mode:latest, count:3)이 동결되고, 댓글 풀 전체가 실린다.
    const plan = enqueuedPlan();
    expect(plan.kind).toBe("comment");
    expect(plan.band).toHaveLength(1);
    expect(plan.band[0]?.commentTarget).toEqual(
      expect.objectContaining({ mode: "latest", count: 3 }),
    );
    expect(plan.comments).toEqual(["좋아요", "멋지네요"]);
  });

  it("밴드 댓글 전용 인기글 대상은 plan.band에 mode=popular로 동결한다", async () => {
    const commentDoc: LibraryPost = {
      ...postDoc,
      id: "l-band-comment-popular",
      kind: "comment",
      comments: ["좋아요", "멋지네요"],
      commentTarget: "popular",
      commentCount: 5,
    };
    renderPublish({ doc: commentDoc });
    await userEvent.click(await screen.findByText("value_invest")); // band a7

    const linkInput = screen.getByLabelText("밴드 링크");
    const saveBtn = screen.getByRole("button", { name: "저장" });
    await userEvent.type(linkInput, "https://band.us/band/103043410");
    await waitFor(() => expect(saveBtn).toBeEnabled());
    await userEvent.click(saveBtn);
    await waitFor(() => expect(linkInput).toHaveValue(""));
    await screen.findByPlaceholderText("게시할 밴드 선택");
    await pickOption(0, "데일밴드");
    await screen.findByLabelText("데일밴드 제거");

    await pickStock(); // 기본 forum(a1)에 종목 부여 → 게시 가능(#267-11). 계정 토글 모두 끝낸 뒤 호출.
    const publishBtn = await screen.findByRole("button", {
      name: /^게시 \(\d+\)/,
    });
    await waitFor(() => expect(publishBtn).toBeEnabled());
    await userEvent.click(publishBtn);

    const plan = enqueuedPlan();
    expect(plan.band[0]?.commentTarget).toEqual(
      expect.objectContaining({ mode: "popular", count: 5 }),
    );
  });

  it("밴드는 url 댓글 미지원이라 plan.band에서 제외한다(네이버 url 대상은 유지)", async () => {
    // 네이버+밴드 혼합 댓글 전용 — 네이버는 url 글에 댓글이 가능하지만 밴드는 불가.
    const commentDoc: LibraryPost = {
      ...postDoc,
      id: "l-band-comment-url",
      kind: "comment",
      comments: ["좋아요", "멋지네요"],
      commentTarget: "url",
      commentUrl: "https://cafe.naver.com/ca-fe/cafes/31732304/articles/9",
    };
    renderPublish({ doc: commentDoc });
    await userEvent.click(await screen.findByText("money_lab")); // naver
    await userEvent.click(await screen.findByText("value_invest")); // band a7

    const linkInput = screen.getByLabelText("밴드 링크");
    const saveBtn = screen.getByRole("button", { name: "저장" });
    await userEvent.type(linkInput, "https://band.us/band/103043410");
    await waitFor(() => expect(saveBtn).toBeEnabled());
    await userEvent.click(saveBtn);
    await waitFor(() => expect(linkInput).toHaveValue(""));
    await screen.findByPlaceholderText("게시할 밴드 선택");
    await pickOption(0, "데일밴드");
    await screen.findByLabelText("데일밴드 제거");

    await pickStock(); // 기본 forum(a1)에 종목 부여 → 게시 가능(#267-11). 계정 토글 모두 끝낸 뒤 호출.
    const publishBtn = await screen.findByRole("button", {
      name: /^게시 \(\d+\)/,
    });
    await waitFor(() => expect(publishBtn).toBeEnabled());
    await userEvent.click(publishBtn);

    // 밴드는 url 댓글 미지원이라 plan.band에서 제외된다(엉뚱한 최신글 오라우팅 방지).
    // 네이버 url 대상은 그대로 실린다 — 워커가 밴드를 빼고 네이버만 게시한다.
    const plan = enqueuedPlan();
    expect(plan.band).toEqual([]);
    expect(plan.naver).toHaveLength(1);
    expect(plan.naver[0]?.commentTarget).toEqual(
      expect.objectContaining({ mode: "url", cafeId: 31732304, articleId: 9 }),
    );
  });

  it("이름이 같은 밴드(다른 band_no) 둘을 등록해도 드롭다운이 깨지지 않고 각각 선택된다", async () => {
    // 회귀: Select data를 밴드명(value)으로 쓰면 동명 밴드 2개 등록 시 중복 value로
    // Mantine이 깨져 흰 화면이 됐다. value를 고유 band_no로 바꾼 수정의 회귀 가드.
    renderPublish();
    await userEvent.click(await screen.findByText("value_invest")); // band a7

    const linkInput = screen.getByLabelText("밴드 링크");
    const saveBtn = screen.getByRole("button", { name: "저장" });
    const save = async (link: string) => {
      await userEvent.type(linkInput, link);
      await waitFor(() => expect(saveBtn).toBeEnabled());
      await userEvent.click(saveBtn);
      await waitFor(() => expect(linkInput).toHaveValue(""));
      await screen.findByPlaceholderText("게시할 밴드 선택");
    };

    // 이름은 같지만(데일밴드) band_no가 다른 두 밴드 — 두 번째는 www. 형식 링크.
    await save("https://band.us/band/103043410");
    await save("https://www.band.us/band/103084867");

    // 드롭다운을 열면 동명이라도 옵션 2개가 렌더된다(크래시 없음).
    const combo = document.querySelector<HTMLInputElement>(
      'input[aria-haspopup="listbox"]',
    )!;
    await userEvent.click(combo);
    expect(
      [...document.querySelectorAll('[role="option"]')].filter(
        (o) => o.textContent === "데일밴드",
      ),
    ).toHaveLength(2);

    // 두 옵션을 각각 선택 → band_no가 달라 칩이 2개 생긴다(이름은 같아도 별개 밴드).
    const pickNth = async (n: number) => {
      await userEvent.click(combo);
      const opts = [...document.querySelectorAll('[role="option"]')].filter(
        (o) => o.textContent === "데일밴드",
      );
      await userEvent.click(opts[n]!);
    };
    await pickNth(0);
    await pickNth(1);
    expect(await screen.findAllByLabelText("데일밴드 제거")).toHaveLength(2);

    // 게시 → 서로 다른 두 밴드 링크(동결)가 각각 plan.band에 실린다(동명이라도 별개 밴드).
    await pickStock(); // 기본 forum(a1)에 종목 부여 → 게시 가능(#267-11). 계정 토글 모두 끝낸 뒤 호출.
    const publishBtn = await screen.findByRole("button", {
      name: /^게시 \(\d+\)/,
    });
    await waitFor(() => expect(publishBtn).toBeEnabled());
    await userEvent.click(publishBtn);
    const plan = enqueuedPlan();
    const links = plan.band.map((b) => b.link);
    expect(links).toContain("https://band.us/band/103043410");
    expect(links).toContain("https://www.band.us/band/103084867");
  });

  it("밴드 글+댓글은 댓글 풀 전체를 plan.comments에 동결한다", async () => {
    // 회귀: 댓글을 2개 이상 써도 1개만 게시되던 문제 — comments 풀 전체를 plan에 실어야 한다
    // (워커가 같은 글에 전부 단다). 프론트는 더 이상 band_publish를 직접 부르지 않는다.
    const bothDoc: LibraryPost = {
      ...postDoc,
      id: "l-band-multi",
      kind: "both",
      body: "<p>본문</p>",
      comments: ["첫 번째 댓글", "두 번째 댓글"],
    };
    renderPublish({ doc: bothDoc });
    await userEvent.click(await screen.findByText("value_invest")); // band a7

    const linkInput = screen.getByLabelText("밴드 링크");
    const saveBtn = screen.getByRole("button", { name: "저장" });
    await userEvent.type(linkInput, "https://band.us/band/103043410");
    await waitFor(() => expect(saveBtn).toBeEnabled());
    await userEvent.click(saveBtn);
    await waitFor(() => expect(linkInput).toHaveValue(""));
    await screen.findByPlaceholderText("게시할 밴드 선택");
    await pickOption(0, "데일밴드");
    await screen.findByLabelText("데일밴드 제거");

    await pickStock(); // 기본 forum(a1)에 종목 부여 → 게시 가능(#267-11). 계정 토글 모두 끝낸 뒤 호출.
    const publishBtn = await screen.findByRole("button", {
      name: /^게시 \(\d+\)/,
    });
    await waitFor(() => expect(publishBtn).toBeEnabled());
    await userEvent.click(publishBtn);

    // plan.comments에 댓글 풀 전체가 실린다(1개만 X). 밴드 대상도 함께 적재된다.
    const plan = enqueuedPlan();
    expect(plan.kind).toBe("both");
    expect(plan.band).toHaveLength(1);
    expect(plan.comments).toEqual(["첫 번째 댓글", "두 번째 댓글"]);
  });

  // 밴드 부분 실패(댓글 N/M)·게시 reject의 "실패로 표기" 회귀는 이제 워커가 담당한다
  // (즉시 게시도 큐를 타므로, #198). build_log_batch가 밴드 성공/실패를 BatchItemStatus로
  // 매핑하는지는 queue_runner의 백엔드 테스트(build_log_batch_maps_band_*)가 검증한다.

  it("밴드 링크 저장 시 밴드명 조회가 실패하면 링크를 이름으로 폴백한다", async () => {
    // 회귀: band_resolve_name 실패 시 빈 이름이 아니라 원문 링크를 표시명으로 쓴다.
    const real = ipcBackend.getMockImplementation()!;
    ipcBackend.mockImplementation(
      (cmd: string, args?: Record<string, unknown>) =>
        cmd === "band_resolve_name"
          ? Promise.reject(new Error("조회 실패"))
          : real(cmd, args),
    );
    try {
      renderPublish();
      await userEvent.click(await screen.findByText("value_invest"));

      const linkInput = screen.getByLabelText("밴드 링크");
      const saveBtn = screen.getByRole("button", { name: "저장" });
      await userEvent.type(linkInput, "https://band.us/band/103043410");
      await waitFor(() => expect(saveBtn).toBeEnabled());
      await userEvent.click(saveBtn);
      await waitFor(() => expect(linkInput).toHaveValue(""));
      await screen.findByPlaceholderText("게시할 밴드 선택");

      // 조회 실패 → 링크가 표시명으로 폴백된다(옵션·칩 라벨이 링크).
      await pickOption(0, "https://band.us/band/103043410");
      await screen.findByLabelText("https://band.us/band/103043410 제거");
    } finally {
      ipcBackend.mockImplementation(real);
    }
  });

  // --- 네이버블로그(#271): 댓글 전용, 카페 글링크 미러 ---

  const blogCommentDoc: LibraryPost = {
    id: "lbg",
    title: "블로그 댓글 글",
    kind: "comment",
    updated: "방금 전",
    words: 20,
    status: "ready",
    excerpt: "요약",
    // 블로그는 댓글 작성의 commentTarget을 따른다(별도 토글 제거). 기본 fixture는 '특정 글'(url) 모드.
    commentTarget: "url",
    comments: ["좋은 글이네요"],
  };

  /** 블로그 계정 하나만 둔 fixture로 교체하고, 그 계정을 선택한다(doc로 댓글 대상 모드 지정). */
  async function selectBlogAccount(doc: LibraryPost = blogCommentDoc) {
    setAccounts([
      {
        id: "ab",
        platform: "blog",
        loginId: "blog_writer",
        pw: "blog#writer1",
        status: "active",
        last: "1시간 전",
        tags: [],
      },
    ]);
    renderPublish({ doc });
    // 모달은 게시 가능 계정이 하나면 그 계정을 기본 선택한다.
    await screen.findByText("blog_writer");
  }

  it("blog gating: 글을 추가·선택하지 않으면 게시가 막히고 안내가 뜬다", async () => {
    await selectBlogAccount();
    // 블로그 섹션이 보인다(글 링크 입력).
    await screen.findByLabelText("블로그 글 링크");
    // 글을 고르기 전에는 게시 버튼이 비활성이고 안내 문구가 뜬다.
    expect(
      await screen.findByText(
        "블로그 링크를 추가하고 게시할 블로그 글을 선택해야 게시할 수 있어요",
      ),
    ).toBeInTheDocument();
  });

  it("blog: 글 링크를 추가·선택하면 게시가 활성화되고 plan.blog가 동결된다(blogId는 문자열)", async () => {
    await selectBlogAccount();
    const input = await screen.findByLabelText("블로그 글 링크");
    await userEvent.type(input, "https://blog.naver.com/press02/224311392458");
    await userEvent.click(screen.getByRole("button", { name: "추가" }));
    // 드롭다운에서 추가된 글을 고른다.
    await pickOption(0, "press02 · 글 224311392458");
    await screen.findByLabelText("press02 · 글 224311392458 제거");

    const publishBtn = await screen.findByRole(
      "button",
      { name: /^게시 \(1\)/ },
      { timeout: 3000 },
    );
    expect(publishBtn).toBeEnabled();
    await userEvent.click(publishBtn);

    const plan = enqueuedPlan();
    expect(plan.naver).toEqual([]);
    expect(plan.blog).toEqual([
      expect.objectContaining({
        accountId: "blog_writer",
        blogId: "press02",
        logNo: "224311392458",
        link: "https://blog.naver.com/press02/224311392458",
      }),
    ]);
  });

  it("blog: 여러 글을 추가·선택하면 각 글마다 plan.blog 대상이 만들어진다", async () => {
    await selectBlogAccount();
    const input = await screen.findByLabelText("블로그 글 링크");
    const addBtn = screen.getByRole("button", { name: "추가" });
    await userEvent.type(input, "https://blog.naver.com/press02/100");
    await userEvent.click(addBtn);
    await pickOption(0, "press02 · 글 100");
    await userEvent.type(input, "https://blog.naver.com/cho41004/200");
    await userEvent.click(addBtn);
    await pickOption(0, "cho41004 · 글 200");
    await screen.findByLabelText("press02 · 글 100 제거");
    await screen.findByLabelText("cho41004 · 글 200 제거");

    const publishBtn = await screen.findByRole(
      "button",
      { name: /^게시 \(2\)/ },
      { timeout: 3000 },
    );
    expect(publishBtn).toBeEnabled();
    await userEvent.click(publishBtn);

    const plan = enqueuedPlan();
    expect(plan.blog).toHaveLength(2);
    expect(plan.blog.map((b) => b.logNo).sort()).toEqual(["100", "200"]);
  });

  it("blog 최신글 모드: 댓글 대상이 최신글이면 블로그 링크만 받고 plan.blog가 count로 동결된다", async () => {
    // 댓글 작성에서 '최신글'을 고르면(commentTarget=latest) 블로그는 토글 없이 '블로그 링크'
    // 입력만 보여주고, 추가한 블로그의 최신 N개(=commentCount)에 댓글을 단다(인기글도 동일).
    await selectBlogAccount({
      ...blogCommentDoc,
      commentTarget: "latest",
      commentCount: 5,
    });
    // url 모드의 '블로그 글 링크'가 아니라 최신글 모드의 '블로그 링크'가 보인다.
    const input = await screen.findByLabelText("블로그 링크");
    await userEvent.type(
      input,
      "https://blog.naver.com/PostList.naver?blogId=press02&categoryNo=7",
    );
    await userEvent.click(screen.getByRole("button", { name: "추가" }));
    await screen.findByLabelText("press02 · 카테고리 7 제거");

    const publishBtn = await screen.findByRole(
      "button",
      { name: /^게시 \(1\)/ },
      { timeout: 3000 },
    );
    expect(publishBtn).toBeEnabled();
    await userEvent.click(publishBtn);

    const plan = enqueuedPlan();
    expect(plan.blog).toEqual([
      expect.objectContaining({
        accountId: "blog_writer",
        blogId: "press02",
        count: 5,
        categoryNo: 7,
      }),
    ]);
  });

  it("나눠서 즉시 게시: 계정마다 별도 큐를 적재한다(1큐=1계정)", async () => {
    renderPublish();
    // invest_king7(forum) 기본 선택 상태. value_pick(forum)을 추가해 forum 계정 2개로 만든다.
    await userEvent.click(await screen.findByText("value_pick"));
    expect(await screen.findByText("2개")).toBeInTheDocument();
    // 종목 2개 선택(계정 선택을 끝낸 뒤 — 계정 토글이 종목 선택을 비우므로).
    await userEvent.click(
      await screen.findByRole("button", { name: /종목 선택/ }),
    );
    const search = await screen.findByPlaceholderText("종목명 또는 코드 검색");
    await userEvent.type(search, "삼성전자");
    await userEvent.click(await screen.findByText("삼성전자"));
    await userEvent.clear(search);
    await userEvent.type(search, "SK하이닉스");
    await userEvent.click(await screen.findByText("SK하이닉스"));
    await userEvent.click(await screen.findByRole("button", { name: /적용/ }));
    // "나눠서 즉시 게시하기" → 계정 2개라 큐(add_queue_now)가 계정마다 1개씩 2개 적재돼야 한다.
    await userEvent.click(
      await screen.findByRole("button", { name: /나눠서 즉시 게시하기/ }),
    );
    await waitFor(() => {
      const calls = ipcBackend.mock.calls.filter(
        (c) => c[0] === "add_queue_now",
      );
      expect(calls).toHaveLength(2);
    });
    const plans = ipcBackend.mock.calls
      .filter((c) => c[0] === "add_queue_now")
      .map((c) => (c[1] as { item: { plan: PublishPlan } }).item.plan);
    // 각 큐의 forum 대상은 정확히 1개 계정(1큐=1계정).
    for (const p of plans) {
      expect(new Set(p.forum.map((f) => f.accountId)).size).toBe(1);
    }
    // 두 큐는 서로 다른 계정이고, 종목은 안 겹치게 총 2개가 분배된다.
    expect(plans.map((p) => p.forum[0]!.accountId).sort()).toEqual([
      "invest_king7",
      "value_pick",
    ]);
    const codes = plans.flatMap((p) => p.forum.map((f) => f.code));
    expect(codes.length).toBe(2);
    expect(new Set(codes).size).toBe(2);
    // [회귀 #6] 두 큐 아이템의 id는 반드시 고유해야 한다. 같은 동기 tick에 만들어지므로 예전
    // "qn"+Date.now()는 밀리초가 같아 동일 id가 됐고, 백엔드가 한 아이템처럼 다뤄 1계정이
    // post 0으로 증발했다. freshIdSuffix(카운터)로 같은 tick에도 달라야 한다.
    const ids = ipcBackend.mock.calls
      .filter((c) => c[0] === "add_queue_now")
      .map((c) => (c[1] as { item: { id: string } }).item.id);
    expect(ids).toHaveLength(2);
    expect(new Set(ids).size).toBe(2);
  });
});
