import { MantineProvider } from "@mantine/core";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, it, expect, vi } from "vitest";

import type { LibraryPost } from "@/shared/data/types";
import {
  invoke as ipcBackend,
  resetIpc,
  setArticleListFailures,
} from "@/test/ipc";
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

  it("starts the publish flow and shows progress", async () => {
    renderPublish();
    await userEvent.click(
      await screen.findByRole("button", { name: /^게시 \(\d+\)/ }),
    );
    expect(await screen.findByText(/게시하는 중/)).toBeInTheDocument();
  });

  it("completes the publish flow and routes to a follow-up view", async () => {
    vi.spyOn(Math, "random").mockReturnValue(1); // force every job to succeed
    const { go } = renderPublish();
    await userEvent.click(
      await screen.findByRole("button", { name: /^게시 \(\d+\)/ }),
    );
    // results render after the simulated upload (~2s)
    expect(
      await screen.findByText("계속 작성", undefined, { timeout: 3000 }),
    ).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "알림 보기" }));
    expect(go).toHaveBeenCalledWith("log");
    vi.restoreAllMocks();
  });

  it("offers a retry control when a job fails", async () => {
    vi.spyOn(Math, "random").mockReturnValue(0); // force every job to fail
    renderPublish();
    await userEvent.click(
      await screen.findByRole("button", { name: /^게시 \(\d+\)/ }),
    );
    expect(
      await screen.findByRole("button", { name: "재시도" }, { timeout: 3000 }),
    ).toBeInTheDocument();
    vi.restoreAllMocks();
  });

  it("publishes naver jobs through the real run_post_jobs command", async () => {
    renderPublish();
    // Swap the preselected forum account for a naver one. Selecting it loads
    // that account's joined cafes; picking a cafe resolves its boards (first
    // board auto-selected), which drives a real backend publish.
    await userEvent.click(await screen.findByText("invest_king7"));
    await userEvent.click(screen.getByText("money_lab"));
    await screen.findByPlaceholderText("가입 카페 선택");
    await pickOption(0, "주식투자연구소 카페");
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
      await screen.findByText(/글 게시 완료/, undefined, { timeout: 3000 }),
    ).toBeInTheDocument();
    expect(ipcBackend).toHaveBeenCalledWith(
      "run_post_jobs",
      expect.objectContaining({
        jobs: [
          expect.objectContaining({
            // 백엔드는 쿠키 파일 키(loginId)로 계정을 찾는다 — UI 고유 id("a5")가 아니다.
            accountId: "money_lab",
            cafe: "11111111",
            menuId: 1,
            boardType: "L",
          }),
        ],
      }),
    );
  });

  it("loads an account's joined cafes when a naver account is selected", async () => {
    renderPublish();
    await userEvent.click(await screen.findByText("money_lab"));
    // The joined-cafe loader populates the per-account cafe select.
    await screen.findByPlaceholderText("가입 카페 선택");
    // 가입 카페는 쿠키 파일 키(loginId)로 조회해야 한다 — UI 고유 id("a5")로 조회하면
    // 백엔드가 쿠키 파일을 못 찾아 빈 목록을 돌려준다(회귀: 가입 카페가 안 뜨던 버그).
    expect(ipcBackend).toHaveBeenCalledWith("list_joined_cafes", {
      accountId: "money_lab",
    });
    expect(ipcBackend).not.toHaveBeenCalledWith("list_joined_cafes", {
      accountId: "a5",
    });
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
    // onConfirm으로 받은 이름이 칩에 그대로 표시된다(코드가 아니라 이름).
    expect(
      await screen.findByText("KODEX SK하이닉스단일종목레버리지"),
    ).toBeInTheDocument();
  });

  it("removes a selected stock chip", async () => {
    renderPublish();
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
    await screen.findByPlaceholderText("가입 카페 선택");
    await pickOption(0, "주식투자연구소 카페");
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
            boardType: "L",
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
    await screen.findByPlaceholderText("가입 카페 선택");
    await pickOption(0, "주식투자연구소 카페");
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

  it("includes only naver/forum targets in a scheduled plan (band excluded)", async () => {
    renderPublish();
    // 기본 forum(a1) 유지 + naver(money_lab) + band(value_invest) 선택.
    await userEvent.click(await screen.findByText("money_lab"));
    await userEvent.click(screen.getByText("value_invest"));
    await screen.findByPlaceholderText("가입 카페 선택");
    await pickOption(0, "개미투자 카페");
    // forum + naver = 2곳 (밴드 계정은 선택됐지만 게시할 밴드 미선택 → 0건).
    await screen.findByRole(
      "button",
      { name: /^게시 \(2\)/ },
      { timeout: 3000 },
    );
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
        item: { plan: { naver: unknown[]; forum: unknown[] } };
      }
    ).item.plan;
    // band는 엔진이 없어 plan에서 제외된다 — naver 1건, forum 1건만.
    expect(plan.naver).toHaveLength(1);
    expect(plan.forum).toEqual([
      expect.objectContaining({ accountId: "invest_king7", code: "005930" }),
    ]);
  });

  it("comments every template comment on the just-posted article in 'both' mode", async () => {
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
    await screen.findByPlaceholderText("가입 카페 선택");
    await pickOption(0, "주식투자연구소 카페");
    await userEvent.click(
      await screen.findByRole(
        "button",
        { name: /^게시 \(1\)/ },
        { timeout: 3000 },
      ),
    );
    // post lands first…
    expect(ipcBackend).toHaveBeenCalledWith("run_post_jobs", expect.anything());
    // …then both 모드는 쓴 글(articleId 1000)에 댓글 풀 전체를 단다: 글을 댓글 수만큼
    // 복제해 보내 백엔드 분배가 글마다 풀 전체를 깔게 한다(여기선 글 1개 × 댓글 2개).
    const call = ipcBackend.mock.calls.find((c) => c[0] === "run_comment_jobs");
    expect(call).toBeDefined();
    const req = (
      call?.[1] as {
        req: {
          targets: { accountId: string; cafeId: number; articleId: number }[];
          comments: string[];
        };
      }
    ).req;
    expect(req.targets).toHaveLength(2);
    expect(
      req.targets.every(
        (t) =>
          t.accountId === "money_lab" &&
          t.cafeId === 11111111 &&
          t.articleId === 1000,
      ),
    ).toBe(true);
    expect(req.comments).toEqual(["좋네요", "굿"]);
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
    // Distribution moved to the backend (issue #98): the front just sends one
    // target per account plus the comment pool; which comment each account gets
    // is RNG-chosen in Rust, so it's not asserted here.
    const commentCalls = ipcBackend.mock.calls.filter(
      (c) => c[0] === "run_comment_jobs",
    );
    const call = commentCalls[commentCalls.length - 1];
    expect(call).toBeDefined();
    const req = (
      call?.[1] as { req: { targets: unknown[]; comments: string[] } }
    ).req;
    expect(req.targets).toHaveLength(1);
    expect(req.targets[0]).toEqual(
      expect.objectContaining({
        accountId: "money_lab",
        cafeId: 31732304,
        articleId: 9,
      }),
    );
    expect(req.comments).toEqual(["댓글1", "댓글2"]);
  });

  it("comments on the top-N latest articles in 'comment' + 'latest' mode", async () => {
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
    await screen.findByPlaceholderText("가입 카페 선택");
    // 주식투자연구소 카페 (cafeId 11111111) has 10 latest articles in the mock.
    // 개수(3)는 템플릿(doc.commentCount)에서 동결 — 게시 모달엔 개수 UI가 없다.
    await pickOption(0, "주식투자연구소 카페");
    await userEvent.click(
      await screen.findByRole(
        "button",
        { name: /^게시 \(1\)/ },
        { timeout: 3000 },
      ),
    );
    // Distribution moved to the backend (issue #98): the top-3 latest articles
    // become 3 targets (articleId 8000..8002 from the mock), all for the picked
    // cafe; the backend deals one comment from the pool to each.
    const call = ipcBackend.mock.calls.find((c) => c[0] === "run_comment_jobs");
    expect(call).toBeDefined();
    const req = (
      call?.[1] as {
        req: {
          targets: { accountId: string; cafeId: number; articleId: number }[];
          comments: string[];
        };
      }
    ).req;
    expect(req.targets).toHaveLength(3);
    expect(req.targets.every((t) => t.cafeId === 11111111)).toBe(true);
    expect(req.targets.every((t) => t.accountId === "money_lab")).toBe(true);
    expect([...new Set(req.targets.map((t) => t.articleId))].sort()).toEqual([
      8000, 8001, 8002,
    ]);
    expect(req.comments).toEqual(["댓글1", "댓글2"]);
  });

  it("queries the popular list when commentTarget is 'popular'", async () => {
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
    await screen.findByPlaceholderText("가입 카페 선택");
    await pickOption(0, "주식투자연구소 카페");
    await userEvent.click(
      await screen.findByRole(
        "button",
        { name: /^게시 \(1\)/ },
        { timeout: 3000 },
      ),
    );
    // The article-list query uses the 'popular' sort.
    expect(ipcBackend).toHaveBeenCalledWith("list_cafe_articles", {
      cafeId: "11111111",
      sortBy: "popular",
      accountId: "money_lab",
    });
    // …and the popular ORDER must propagate into the targets: the mock reverses
    // for popular, so top-1 is 8009 (not latest's 8000). This fails if a
    // regression takes the latest slice / ignores the returned order.
    const call = ipcBackend.mock.calls.find((c) => c[0] === "run_comment_jobs");
    expect(call).toBeDefined();
    const req = (call?.[1] as { req: { targets: { articleId: number }[] } })
      .req;
    expect(req.targets.map((t) => t.articleId)).toEqual([8009]);
  });

  it("falls back to the available articles when the list has fewer than N", async () => {
    const latestDoc: LibraryPost = {
      id: "lf",
      title: "최신글 폴백",
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
    await userEvent.click(await screen.findByText("invest_king7"));
    await userEvent.click(screen.getByText("money_lab"));
    await screen.findByPlaceholderText("가입 카페 선택");
    // 개미투자 카페 (cafeId 22222222) only has 2 articles in the mock, fewer
    // than the requested N=5 — only those two should become targets.
    await pickOption(0, "개미투자 카페");
    await userEvent.click(
      await screen.findByRole(
        "button",
        { name: /^게시 \(1\)/ },
        { timeout: 3000 },
      ),
    );
    const call = ipcBackend.mock.calls.find((c) => c[0] === "run_comment_jobs");
    expect(call).toBeDefined();
    const req = (call?.[1] as { req: { targets: { articleId: number }[] } })
      .req;
    // 2 available articles → 2 targets (not 5).
    expect(req.targets).toHaveLength(2);
    expect([...new Set(req.targets.map((t) => t.articleId))].sort()).toEqual([
      7000, 7001,
    ]);
  });

  it("surfaces a red toast and drops only the failed account when a list fetch rejects", async () => {
    // money_lab → cafe 11111111 (succeeds), insight_note → cafe 33333333 (rejects).
    setArticleListFailures(["33333333"]);
    const latestDoc: LibraryPost = {
      id: "lerr",
      title: "최신글 댓글 — 목록 실패",
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
    // Two naver rows each expose a cafe-select placeholder.
    await screen.findAllByPlaceholderText("가입 카페 선택");
    // Two naver rows → cafe selects at listbox index 0 and 2 (board selects 1/3).
    await pickOption(0, "주식투자연구소 카페");
    await pickOption(2, "가치투자 모임");
    await userEvent.click(
      await screen.findByRole(
        "button",
        { name: /^게시 \(2\)/ },
        { timeout: 3000 },
      ),
    );
    // The failing cafe surfaces a red toast…
    await vi.waitFor(() =>
      expect(notifShow).toHaveBeenCalledWith(
        expect.objectContaining({ color: "red" }),
      ),
    );
    // …and only the good account's article becomes a comment target — the failed
    // account contributes none (other accounts are unaffected).
    const call = ipcBackend.mock.calls.find((c) => c[0] === "run_comment_jobs");
    expect(call).toBeDefined();
    const req = (
      call?.[1] as {
        req: { targets: { accountId: string; cafeId: number }[] };
      }
    ).req;
    expect(req.targets).toEqual([
      expect.objectContaining({ accountId: "money_lab", cafeId: 11111111 }),
    ]);
    expect(req.targets.some((t) => t.accountId === "insight_note")).toBe(false);
  });

  it("uses the template's commentCount (5) for the number of comment targets", async () => {
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
    await screen.findByPlaceholderText("가입 카페 선택");
    // 주식투자연구소 카페 (cafeId 11111111) has 10 latest articles in the mock.
    await pickOption(0, "주식투자연구소 카페");
    await userEvent.click(
      await screen.findByRole(
        "button",
        { name: /^게시 \(1\)/ },
        { timeout: 3000 },
      ),
    );
    const call = ipcBackend.mock.calls.find((c) => c[0] === "run_comment_jobs");
    expect(call).toBeDefined();
    const req = (call?.[1] as { req: { targets: { articleId: number }[] } })
      .req;
    // Top-5 latest → articleId 8000..8004.
    expect(req.targets).toHaveLength(5);
    expect([...new Set(req.targets.map((t) => t.articleId))].sort()).toEqual([
      8000, 8001, 8002, 8003, 8004,
    ]);
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
    await screen.findByPlaceholderText("가입 카페 선택");
    // No cafe picked → no comment job is built, so the publish button is (0) and
    // disabled (listTargetReady guard).
    const publish = await screen.findByRole("button", { name: /^게시 \(0\)/ });
    expect(publish).toBeDisabled();
    // Picking a cafe satisfies the guard and re-enables it.
    await pickOption(0, "주식투자연구소 카페");
    await waitFor(() =>
      expect(screen.getByRole("button", { name: /^게시 \(1\)/ })).toBeEnabled(),
    );
  });

  it("picks a per-account cafe/board with a band account selected too", async () => {
    renderPublish();
    // Add a naver and a band account alongside the default forum one.
    await userEvent.click(await screen.findByText("money_lab")); // a5 naver
    await userEvent.click(screen.getByText("value_invest")); // a7 band
    await screen.findByPlaceholderText("가입 카페 선택");
    // naver row exposes cafe (0) + board (1); 밴드는 링크/실제밴드명 드롭다운(2).
    await pickOption(0, "개미투자 카페");
    // forum (a1) = 1 job; the naver job lands once its first board auto-selects → 2.
    // (밴드 계정은 선택됐지만 게시할 밴드 미선택 → 밴드 잡 0건.)
    await screen.findByRole(
      "button",
      { name: /^게시 \(2\)/ },
      { timeout: 3000 },
    );
    await pickOption(1, "공지사항");
    const combos = [
      ...document.querySelectorAll<HTMLInputElement>(
        'input[aria-haspopup="listbox"]',
      ),
    ];
    expect(combos[0]).toHaveValue("개미투자 카페");
    expect(combos[1]).toHaveValue("공지사항");
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

  it("accumulates bands from links, multi-selects, and publishes to each", async () => {
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

    // 게시 → 선택한 각 밴드 링크로 band_publish가 호출되어야 한다.
    const publishBtn = await screen.findByRole("button", {
      name: /^게시 \(\d+\)/,
    });
    await waitFor(() => expect(publishBtn).toBeEnabled());
    await userEvent.click(publishBtn);
    await waitFor(() => {
      const links = ipcBackend.mock.calls
        .filter((c) => c[0] === "band_publish")
        .map((c) => (c[1] as { bandLink: string }).bandLink);
      expect(links).toContain("https://band.us/band/103043410");
      expect(links).toContain("https://band.us/band/999");
    });
    // 밴드 게시 결과가 알림(게시 배치)에 기록되도록 record_band_batch가 호출된다
    // (종토방처럼 알림에 떠야 함). 선택한 두 밴드가 items로 들어간다.
    await waitFor(() => {
      const rec = ipcBackend.mock.calls.find(
        (c) => c[0] === "record_band_batch",
      );
      expect(rec).toBeTruthy();
      const arg = rec![1] as { items: { target: string }[] };
      const targets = arg.items.map((i) => i.target);
      expect(targets).toContain("데일밴드");
      expect(targets).toContain("밴드 999");
    });
  });

  it("밴드 댓글 전용 모드는 기존 글(최신글)에 band_comment로 댓글을 단다", async () => {
    // 밴드만 선택한 댓글 전용 — 새 글(band_publish)이 아니라 기존 글 조회+댓글(band_comment).
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

    const publishBtn = await screen.findByRole("button", {
      name: /^게시 \(\d+\)/,
    });
    await waitFor(() => expect(publishBtn).toBeEnabled());
    await userEvent.click(publishBtn);

    // band_comment(기존 글에 댓글)가 최신글/개수/댓글 풀과 함께 호출된다.
    await waitFor(() => {
      const call = ipcBackend.mock.calls.find((c) => c[0] === "band_comment");
      expect(call).toBeDefined();
      const arg = call![1] as {
        mode: string;
        count: number;
        comments: string[];
      };
      expect(arg.mode).toBe("latest");
      expect(arg.count).toBe(3);
      expect(arg.comments).toEqual(["좋아요", "멋지네요"]);
    });
    // 댓글 전용은 새 글을 만들지 않는다 — band_publish 미호출.
    expect(
      ipcBackend.mock.calls.find((c) => c[0] === "band_publish"),
    ).toBeUndefined();
  });
});
