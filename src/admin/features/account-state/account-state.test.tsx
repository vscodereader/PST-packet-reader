import { MantineProvider } from "@mantine/core";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { pickOption } from "@/test/select";

// 삭제(휴지통) 배선 검증용 — api·알림은 mock. 순수 헬퍼 테스트는 아래 describe에서 그대로 유지.
const deleteFn = vi.hoisted(() =>
  vi.fn((_deviceId: string, _loginIds: string[]) =>
    Promise.resolve({ ok: true, commandId: "c-del-1" }),
  ),
);
const listFn = vi.hoisted(() =>
  vi.fn(() =>
    Promise.resolve([{ id: "d1", name: "하위-001", connected: true }]),
  ),
);
const inventoryFn = vi.hoisted(() =>
  vi.fn(() =>
    Promise.resolve({
      posts: [],
      accounts: [],
      accountRows: [
        { loginId: "invest_king7", platform: "forum", status: "active" },
        { loginId: "blog_press02", platform: "blog", status: "waiting" },
      ],
      receivedAt: null,
    }),
  ),
);

vi.mock("../../api", () => ({
  isOffline: () => false,
  api: {
    devices: { list: listFn, inventory: inventoryFn },
    accounts: { updateMeta: vi.fn(), delete: deleteFn },
  },
}));
vi.mock("@mantine/notifications", () => ({
  notifications: { show: vi.fn() },
}));

import {
  AccountState,
  diffAccountRows,
  isReversibleStatus,
  statusLabel,
  type AccountRow,
} from "./account-state";

// 렌더/네트워크 비의존 순수 헬퍼만 검증(다른 Admin 테스트와 동일 방침).
describe("account-state 헬퍼", () => {
  describe("diffAccountRows (§2 저장: 바뀐 행만)", () => {
    const original: AccountRow[] = [
      { loginId: "a", platform: "forum", status: "waiting" },
      { loginId: "b", platform: "blog", status: "active" },
      { loginId: "c", platform: "clip", status: "onHold" },
    ];

    it("바뀐 게 없으면 빈 배열", () => {
      expect(diffAccountRows(original, original)).toEqual([]);
    });

    it("플랫폼만 바뀐 행은 platform만 담는다", () => {
      const edited: AccountRow[] = [
        { loginId: "a", platform: "blog", status: "waiting" },
        ...original.slice(1),
      ];
      expect(diffAccountRows(original, edited)).toEqual([
        { loginId: "a", platform: "blog" },
      ]);
    });

    it("상태만 바뀐 행은 status만 담는다", () => {
      const edited: AccountRow[] = [
        original[0]!,
        { loginId: "b", platform: "blog", status: "waiting" },
        original[2]!,
      ];
      expect(diffAccountRows(original, edited)).toEqual([
        { loginId: "b", status: "waiting" },
      ]);
    });

    it("둘 다 바뀌면 둘 다 담고, 여러 행을 모은다", () => {
      const edited: AccountRow[] = [
        { loginId: "a", platform: "naver", status: "active" },
        original[1]!,
        { loginId: "c", platform: "clip", status: "active" },
      ];
      expect(diffAccountRows(original, edited)).toEqual([
        { loginId: "a", platform: "naver", status: "active" },
        { loginId: "c", status: "active" },
      ]);
    });

    it("original에 없는 loginId는 무시한다", () => {
      const edited: AccountRow[] = [
        ...original,
        { loginId: "ghost", platform: "band", status: "active" },
      ];
      expect(diffAccountRows(original, edited)).toEqual([]);
    });
  });

  describe("isReversibleStatus (§5: 사람이 되돌릴 수 있는 3종)", () => {
    it("active/waiting/onHold만 true", () => {
      expect(isReversibleStatus("active")).toBe(true);
      expect(isReversibleStatus("waiting")).toBe(true);
      expect(isReversibleStatus("onHold")).toBe(true);
    });
    it("워커 판정값은 false(드롭다운 제외)", () => {
      expect(isReversibleStatus("blocked")).toBe(false);
      expect(isReversibleStatus("badCredentials")).toBe(false);
      expect(isReversibleStatus("timedOut")).toBe(false);
    });
  });

  describe("statusLabel", () => {
    it("알려진 상태는 한글 라벨", () => {
      expect(statusLabel("onHold")).toBe("보류");
      expect(statusLabel("blocked")).toBe("차단");
    });
    it("미상은 원문 그대로", () => {
      expect(statusLabel("mystery")).toBe("mystery");
    });
  });
});

// 삭제(휴지통) 렌더/배선 — 각 계정 행에 휴지통이 뜨고, 클릭→확인 시 api.accounts.delete가 그 loginId로
// 호출되며, 성공 후 그 행이 목록에서 사라진다(낙관적 삭제). 다른 Admin 렌더 테스트와 동일 방침(api·알림 mock).
describe("AccountState 삭제(휴지통) 렌더/배선", () => {
  beforeEach(() => {
    deleteFn.mockClear();
  });

  // 하위 COM을 골라 그 하위의 계정 행이 표시될 때까지 기다린다(inventory 폴링 재사용). Mantine Select
  // 열기·옵션 선택은 프로젝트 공용 pickOption 헬퍼로(jsdom에서 옵션이 listbox role로 안 잡히는 문제 회피).
  async function selectDeviceAndWait() {
    render(
      <MantineProvider>
        <AccountState />
      </MantineProvider>,
    );
    await screen.findByPlaceholderText("하위 COM 선택");
    await pickOption(0, "하위-001");
    // 첫 계정 행의 휴지통(aria-label = "<masked> 삭제")이 뜰 때까지.
    await screen.findByRole("button", { name: "in•••••••••• 삭제" });
  }

  it("계정 행마다 휴지통이 렌더된다", async () => {
    await selectDeviceAndWait();
    expect(
      screen.getByRole("button", { name: "in•••••••••• 삭제" }),
    ).toBeTruthy();
    expect(
      screen.getByRole("button", { name: "bl•••••••••• 삭제" }),
    ).toBeTruthy();
  });

  it("휴지통 클릭→확인 시 그 loginId로 삭제하고 행을 제거한다", async () => {
    const user = userEvent.setup();
    await selectDeviceAndWait();

    await user.click(screen.getByRole("button", { name: "in•••••••••• 삭제" }));
    // 확인 모달의 [삭제] 버튼(정확 이름 "삭제")을 누른다.
    await user.click(screen.getByRole("button", { name: "삭제" }));

    await waitFor(() =>
      expect(deleteFn).toHaveBeenCalledWith("d1", ["invest_king7"]),
    );
    // 낙관적 제거 — 그 행 휴지통이 사라진다(다른 행은 유지).
    await waitFor(() =>
      expect(
        screen.queryByRole("button", { name: "in•••••••••• 삭제" }),
      ).toBeNull(),
    );
    expect(
      screen.getByRole("button", { name: "bl•••••••••• 삭제" }),
    ).toBeTruthy();
  });
});
