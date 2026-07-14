import { MantineProvider } from "@mantine/core";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

// 기타 명령 페이지(15-기타명령 §2) 렌더/배선 검증. 네트워크(api)·알림(notifications)은 mock.
// 하위 선택 → 행동 목록 → 각 행동 UI 노출 + 올바른 payload(api.etc.*) 호출을 확인한다.
const like = vi.hoisted(() =>
  vi.fn(() => Promise.resolve({ ok: true, commandId: "c1" })),
);
const dislike = vi.hoisted(() =>
  vi.fn(() => Promise.resolve({ ok: true, commandId: "c2" })),
);
const boostView = vi.hoisted(() =>
  vi.fn(() => Promise.resolve({ ok: true, commandId: "c3" })),
);
const rotateIp = vi.hoisted(() =>
  vi.fn(() => Promise.resolve({ ok: true, commandId: "c4" })),
);
const list = vi.hoisted(() =>
  vi.fn(() =>
    Promise.resolve([
      { id: "d1", name: "하위-001", connected: true, ip: "1.2.3.4" },
    ]),
  ),
);
const inventory = vi.hoisted(() =>
  vi.fn(() =>
    Promise.resolve({
      posts: [],
      accounts: [],
      accountRows: [
        { loginId: "forum_a", platform: "forum", status: "active" },
        { loginId: "forum_b", platform: "forum", status: "active" },
        { loginId: "cafe_x", platform: "naver", status: "active" },
      ],
      receivedAt: null,
    }),
  ),
);

vi.mock("../../api", () => ({
  isOffline: () => false,
  api: {
    devices: { list, inventory },
    etc: { like, dislike, boostView, rotateIp },
  },
}));
vi.mock("@mantine/notifications", () => ({
  notifications: { show: vi.fn() },
}));

import { EtcCommand } from "./etc-command";

function renderPage() {
  return render(
    <MantineProvider>
      <EtcCommand />
    </MantineProvider>,
  );
}

async function selectDevice(user: ReturnType<typeof userEvent.setup>) {
  // 실서버 목록(하위-001)이 뜰 때까지 기다렸다 선택.
  const card = await screen.findByLabelText("하위-001 선택");
  await user.click(card);
}

async function addLink(
  user: ReturnType<typeof userEvent.setup>,
  labelRe: RegExp,
  url: string,
) {
  const input = screen.getByLabelText(labelRe);
  await user.type(input, url);
  await user.click(screen.getByRole("button", { name: "추가" }));
}

describe("EtcCommand (기타 명령 페이지)", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("하위를 골라야 행동 목록이 뜬다", async () => {
    const user = userEvent.setup();
    renderPage();
    // 선택 전엔 행동 선택 라벨 없음.
    expect(screen.queryByText("② 행동을 고르세요")).toBeNull();
    await selectDevice(user);
    expect(await screen.findByText("② 행동을 고르세요")).toBeInTheDocument();
  });

  it("좋아요: 링크 + 종토 active 계정으로 like_posts payload를 보낸다", async () => {
    const user = userEvent.setup();
    renderPage();
    await selectDevice(user);
    await user.click(await screen.findByRole("radio", { name: "좋아요" }));

    // 종토 계정만 뜬다(카페=naver 제외, §6-4).
    expect(await screen.findByLabelText("forum_a 선택")).toBeInTheDocument();
    expect(screen.queryByLabelText("cafe_x 선택")).toBeNull();

    await addLink(user, /좋아요를 누를 게시글 링크/, "https://x/discussion/111");
    await user.click(screen.getByLabelText("forum_a 선택"));

    await user.click(screen.getByRole("button", { name: /^좋아요$/ }));
    await waitFor(() =>
      expect(like).toHaveBeenCalledWith(
        "d1",
        ["https://x/discussion/111"],
        ["forum_a"],
      ),
    );
    expect(dislike).not.toHaveBeenCalled();
  });

  it("싫어요: dislike_posts로 보낸다", async () => {
    const user = userEvent.setup();
    renderPage();
    await selectDevice(user);
    await user.click(await screen.findByRole("radio", { name: "싫어요" }));
    await addLink(user, /싫어요를 누를 게시글 링크/, "https://x/discussion/222");
    await user.click(await screen.findByLabelText("forum_b 선택"));
    await user.click(screen.getByRole("button", { name: /^싫어요$/ }));
    await waitFor(() =>
      expect(dislike).toHaveBeenCalledWith(
        "d1",
        ["https://x/discussion/222"],
        ["forum_b"],
      ),
    );
  });

  it("조회수: 링크 × 횟수 N으로 boost_view를 보낸다(계정 선택 없음)", async () => {
    const user = userEvent.setup();
    renderPage();
    await selectDevice(user);
    await user.click(await screen.findByRole("radio", { name: "조회수" }));
    // 계정 체크박스 없음.
    expect(screen.queryByLabelText("forum_a 선택")).toBeNull();
    await addLink(user, /조회수를 올릴 게시글 링크/, "https://x/discussion/333");
    // 기본 30회.
    await user.click(screen.getByRole("button", { name: /^조회수$/ }));
    await waitFor(() =>
      expect(boostView).toHaveBeenCalledWith(
        "d1",
        ["https://x/discussion/333"],
        30,
      ),
    );
  });

  it("IP 변경: 버튼 하나로 rotate_ip를 보낸다", async () => {
    const user = userEvent.setup();
    renderPage();
    await selectDevice(user);
    await user.click(await screen.findByRole("radio", { name: "IP 변경" }));
    await user.click(screen.getByRole("button", { name: "IP 변경" }));
    await waitFor(() => expect(rotateIp).toHaveBeenCalledWith("d1"));
  });

  it("링크나 계정이 없으면 좋아요 버튼은 비활성", async () => {
    const user = userEvent.setup();
    renderPage();
    await selectDevice(user);
    await user.click(await screen.findByRole("radio", { name: "좋아요" }));
    // 링크·계정 미입력 → 버튼 disabled.
    const btn = screen.getByRole("button", { name: /^좋아요$/ });
    expect(btn).toBeDisabled();
    await waitFor(() => expect(like).not.toHaveBeenCalled());
  });
});
