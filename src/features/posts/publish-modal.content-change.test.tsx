import { MantineProvider } from "@mantine/core";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, it, expect, vi } from "vitest";

import type { LibraryPost } from "@/shared/data/types";
import { resetIpc } from "@/test/ipc";

import { PublishModal } from "./publish-modal";

vi.mock("@tauri-apps/api/core", async () => ({
  invoke: (await import("@/test/ipc")).invoke,
}));
const { notifShow } = vi.hoisted(() => ({ notifShow: vi.fn() }));
vi.mock("@mantine/notifications", () => ({
  notifications: { show: notifShow },
}));

// 글(post) 문서 — 내용변경 UI는 글/글+댓글 모드에서만 뜬다. 토큰(#{종목명})은 넣지 않아
// showTokens 등 부수 렌더를 배제하고 내용변경↔종목 상호작용만 좁혀 검증한다.
const postDoc: LibraryPost = {
  id: "l1",
  title: "제목입니다",
  kind: "post",
  updated: "방금 전",
  words: 100,
  status: "ready",
  excerpt: "요약",
  body: "<p>본문</p>",
};

function renderPublish() {
  render(
    <MantineProvider>
      <PublishModal open doc={postDoc} onClose={vi.fn()} go={vi.fn()} />
    </MantineProvider>,
  );
}

// 종목 선택 모달을 열어 주어진 종목들을 고르고 적용한다(stockCodes 비우지 않기 위함).
// 한 번의 열기에서 여러 종목을 누적 선택할 수 있다(모달 sel은 preselected에서 시작).
async function pickStocks(pairs: [query: string, name: string][]) {
  await userEvent.click(
    await screen.findByRole("button", { name: /종목 선택/ }),
  );
  const search = await screen.findByPlaceholderText("종목명 또는 코드 검색");
  for (const [query, name] of pairs) {
    await userEvent.clear(search);
    await userEvent.type(search, query);
    await userEvent.click(await screen.findByText(name));
  }
  await userEvent.click(await screen.findByRole("button", { name: /적용/ }));
}

// 내용변경 체크박스는 종목토론방(forum) 컨텍스트에서만 뜬다. 계정 자동선택이 비동기라
// forum 카드("종목 선택" 버튼)가 뜰 때까지 기다린 뒤(=forum이 selPlatforms에 들어온 뒤) 체크한다.
const checkContentChange = async () => {
  await screen.findByRole("button", { name: /종목 선택/ });
  await userEvent.click(await screen.findByLabelText("게시 후 내용 변경"));
};

// 흰화면(렌더 예외)이 없었다면 내용변경 입력들이 DOM에 존재한다.
async function expectNoCrash() {
  expect(
    await screen.findByPlaceholderText("변경할 새 제목"),
  ).toBeInTheDocument();
  expect(screen.getByPlaceholderText("변경할 새 내용")).toBeInTheDocument();
}

describe("PublishModal 내용변경(게시 후 내용 변경) 흰화면 회귀", () => {
  beforeEach(() => {
    resetIpc();
    notifShow.mockClear();
  });

  // 1) 종목 없음 + 체크 — 원래 정상 동작하던 케이스(회귀 방지용).
  it("종목 없음 + 체크 → 크래시 없음", async () => {
    renderPublish();
    await checkContentChange();
    await expectNoCrash();
  });

  // 2a) 종목 선택 → 체크 (수정 대상 크래시).
  it("종목 선택 → 체크 → 크래시 없음", async () => {
    renderPublish();
    await pickStocks([["005930", "삼성전자"]]);
    await checkContentChange();
    await expectNoCrash();
  });

  // 2b) 체크 → 종목 선택 (순서 무관 크래시).
  it("체크 → 종목 선택 → 크래시 없음", async () => {
    renderPublish();
    await checkContentChange();
    await expectNoCrash();
    await pickStocks([["005930", "삼성전자"]]);
    await expectNoCrash();
  });

  // 2c) 체크 → 제목·본문 입력 → 종목 선택 → 해제 → 재체크(값 보존).
  it("체크 → 입력 → 종목 선택 → 해제 → 재체크 → 크래시 없음(값 보존)", async () => {
    renderPublish();
    await checkContentChange();
    await userEvent.type(
      await screen.findByPlaceholderText("변경할 새 제목"),
      "새 제목",
    );
    await userEvent.type(
      screen.getByPlaceholderText("변경할 새 내용"),
      "새 본문",
    );
    await pickStocks([["005930", "삼성전자"]]);
    await checkContentChange(); // 해제
    await checkContentChange(); // 재체크
    const title = await screen.findByPlaceholderText("변경할 새 제목");
    expect((title as HTMLInputElement).value).toBe("새 제목");
  });

  // 3) 종목 여러 개 선택 + 체크.
  it("종목 2개 선택 → 체크 → 크래시 없음", async () => {
    renderPublish();
    await pickStocks([
      ["005930", "삼성전자"],
      ["000660", "SK하이닉스"],
    ]);
    await checkContentChange();
    await expectNoCrash();
  });

  // 4) 종목 선택 + 체크 + 제목·본문 타이핑(#400 타이핑 수정 회귀 가드).
  it("종목 선택 + 체크 + 제목/본문 타이핑 → 크래시 없음", async () => {
    renderPublish();
    await pickStocks([["005930", "삼성전자"]]);
    await checkContentChange();
    const title = await screen.findByPlaceholderText("변경할 새 제목");
    await userEvent.type(title, "바뀐 제목");
    const body = screen.getByPlaceholderText("변경할 새 내용");
    await userEvent.type(body, "바뀐 본문");
    expect((title as HTMLInputElement).value).toBe("바뀐 제목");
    expect((body as HTMLTextAreaElement).value).toBe("바뀐 본문");
  });

  // 5) 종목 선택 + 체크 + 변경 지연 NumberInput 변경.
  it("종목 선택 + 체크 + 변경 지연 입력 → 크래시 없음", async () => {
    renderPublish();
    await pickStocks([["005930", "삼성전자"]]);
    await checkContentChange();
    await expectNoCrash();
    const delay = screen.getByRole("textbox", { name: "변경 지연" });
    await userEvent.clear(delay);
    await userEvent.type(delay, "30");
    expect((delay as HTMLInputElement).value).toBe("30");
  });

  // 6) 미리보기 열기(종목 선택 + 내용변경 + 제목/본문 채운 상태) — 2차 크래시 지점 가드.
  it("종목 선택 + 내용변경 채움 상태에서 미리보기 열기 → 크래시 없음", async () => {
    renderPublish();
    await pickStocks([["005930", "삼성전자"]]);
    await checkContentChange();
    await userEvent.type(
      await screen.findByPlaceholderText("변경할 새 제목"),
      "새 제목",
    );
    await userEvent.type(
      screen.getByPlaceholderText("변경할 새 내용"),
      "새 본문",
    );
    await userEvent.click(screen.getByRole("button", { name: /미리보기/ }));
    expect(
      await screen.findByRole("dialog", { name: "미리보기" }),
    ).toBeInTheDocument();
  });

  // 7) 나눠서 게시 가능 상태(계정 2개 + 종목 2개) + 내용변경 → 크래시 없음.
  it("계정 2개 + 종목 2개(나눠서 게시 가능) + 체크 → 크래시 없음", async () => {
    renderPublish();
    // 두 번째 forum 계정(value_pick, a2)을 추가 선택.
    await userEvent.click(await screen.findByText("value_pick"));
    await pickStocks([
      ["005930", "삼성전자"],
      ["000660", "SK하이닉스"],
    ]);
    await checkContentChange();
    await expectNoCrash();
    // 나눠서 게시 버튼이 활성 상태로 함께 렌더된다(크래시 없이).
    expect(
      screen.getByRole("button", { name: /나눠서 즉시 게시하기/ }),
    ).toBeEnabled();
  });
});
