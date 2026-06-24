import { MantineProvider } from "@mantine/core";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, it, expect, vi } from "vitest";

import { ipc } from "@/shared/ipc";
import { invoke as ipcBackend, resetIpc, setLoginOutcomes } from "@/test/ipc";
import { pickOption } from "@/test/select";

import { Accounts } from "./accounts";

vi.mock("@tauri-apps/api/core", async () => ({
  invoke: (await import("@/test/ipc")).invoke,
}));

vi.mock("@tauri-apps/plugin-dialog", () => ({
  save: vi.fn().mockResolvedValue("/tmp/계정.xlsx"),
  open: vi.fn().mockResolvedValue(null),
}));

// 테스트는 <Notifications/> 없이 렌더하므로 토스트가 DOM에 뜨지 않는다.
// notifications.show를 스파이로 대체해 토스트(성공/실패/오류)를 단언한다.
const { notifShow } = vi.hoisted(() => ({ notifShow: vi.fn() }));
vi.mock("@mantine/notifications", () => ({
  notifications: { show: notifShow },
}));

async function renderAccounts(go = vi.fn()) {
  render(
    <MantineProvider>
      <Accounts go={go} />
    </MantineProvider>,
  );
  // Wait for the async IPC load to populate the table.
  await screen.findByText("invest_king7");
  return go;
}

describe("Accounts", () => {
  // The component loads its rows asynchronously over the IPC wrapper, mocked
  // here by the in-memory backend. Reset between tests so each starts from the
  // pristine 15-account dataset.
  beforeEach(() => {
    resetIpc();
    notifShow.mockClear();
    vi.spyOn(ipc.activity, "append").mockResolvedValue(undefined);
  });

  it("renders the title and first page of accounts (10 rows)", async () => {
    await renderAccounts();
    expect(
      screen.getByRole("heading", { name: "계정 관리" }),
    ).toBeInTheDocument();
    // 15 accounts → first page shows 10 data rows + 1 header row
    expect(screen.getAllByRole("row").length).toBe(11);
  });

  // 'IP 변경' 버튼(#247): 로그인 없이 폰 비행기모드만 토글해 IP 회전.
  describe("IP 변경 버튼 (#247)", () => {
    it("IP가 바뀌면 변경 토스트(원래/바뀐 IP)와 알림을 남긴다", async () => {
      const spy = vi.spyOn(ipc.auth, "rotateIp").mockResolvedValue({
        before: "106.101.76.219",
        after: "106.101.73.32",
        changed: true,
      });
      const append = vi
        .spyOn(ipc.activity, "append")
        .mockResolvedValue(undefined);
      await renderAccounts();
      await userEvent.click(screen.getByRole("button", { name: "IP 변경" }));
      expect(spy).toHaveBeenCalledTimes(1);
      await waitFor(() =>
        expect(notifShow).toHaveBeenCalledWith(
          expect.objectContaining({
            color: "green",
            message: expect.stringContaining("IP 변경됨"),
          }),
        ),
      );
      // 게시 큐 밑 알림에도 변경 내역이 남는다(원래/바뀐 IP 포함).
      expect(append).toHaveBeenCalledWith(
        "success",
        expect.stringContaining("106.101.73.32"),
      );
    });

    it("IP가 그대로면 '그대로' 토스트와 정보 알림을 남긴다", async () => {
      vi.spyOn(ipc.auth, "rotateIp").mockResolvedValue({
        before: "106.101.76.219",
        after: "106.101.76.219",
        changed: false,
      });
      const append = vi
        .spyOn(ipc.activity, "append")
        .mockResolvedValue(undefined);
      await renderAccounts();
      await userEvent.click(screen.getByRole("button", { name: "IP 변경" }));
      await waitFor(() =>
        expect(notifShow).toHaveBeenCalledWith(
          expect.objectContaining({
            color: "orange",
            message: expect.stringContaining("그대로"),
          }),
        ),
      );
      expect(append).toHaveBeenCalledWith(
        "info",
        expect.stringContaining("IP 변경 안 됨"),
      );
    });

    it("실패하면 빨간 토스트를 띄운다", async () => {
      vi.spyOn(ipc.auth, "rotateIp").mockRejectedValue(
        new Error("ADB 디바이스가 연결되지 않았거나 인증되지 않았습니다"),
      );
      await renderAccounts();
      await userEvent.click(screen.getByRole("button", { name: "IP 변경" }));
      await waitFor(() =>
        expect(notifShow).toHaveBeenCalledWith(
          expect.objectContaining({
            color: "red",
            message: expect.stringContaining("IP 변경 실패"),
          }),
        ),
      );
    });
  });

  it("filters by platform via the segment chips", async () => {
    await renderAccounts();
    // band has 2 accounts in the mock
    await userEvent.click(screen.getByRole("button", { name: /밴드/ }));
    expect(screen.getAllByRole("row").length).toBe(3); // header + 2 band rows
  });

  it("navigates to 알림 with an account filter from 보러가기", async () => {
    const go = await renderAccounts();
    const firstBody = screen.getAllByRole("row")[1];
    await userEvent.click(
      within(firstBody!).getByRole("button", { name: /보러가기/ }),
    );
    expect(go).toHaveBeenCalledWith(
      "log",
      expect.objectContaining({
        logFilter: expect.objectContaining({ platform: expect.any(String) }),
      }),
    );
  });

  it("edits a login id inline", async () => {
    await renderAccounts();
    const row = screen.getAllByRole("row")[1]!;
    await userEvent.click(within(row).getByText("invest_king7"));
    const input = within(row).getByDisplayValue("invest_king7");
    await userEvent.clear(input);
    await userEvent.type(input, "renamed_id{Enter}");
    expect(await screen.findByText("renamed_id")).toBeInTheDocument();
  });

  it("reveals a masked password on demand", async () => {
    await renderAccounts();
    const row = screen.getAllByRole("row")[1]!;
    expect(within(row).queryByText("ik7!naver22")).not.toBeInTheDocument();
    await userEvent.click(within(row).getByTitle("보기"));
    expect(within(row).getByText("ik7!naver22")).toBeInTheDocument();
  });

  it("cycles account status when the badge is clicked", async () => {
    await renderAccounts();
    const row = screen.getAllByRole("row")[1]!;
    // 배지는 상태 라벨 텍스트로 찾는다(title은 이제 상태별 안내 문구로 동적).
    expect(within(row).getByText("활성")).toBeInTheDocument();
    await userEvent.click(within(row).getByText("활성"));
    // 수동 순환(new→active→waiting→blocked): 활성 다음은 대기(#267-3 활성 배지 클릭→대기).
    await waitFor(() =>
      expect(within(row).getByText("대기")).toBeInTheDocument(),
    );
    // 대기 배지를 누르면 곧장 활성으로 되돌린다(#267-3 재활성).
    await userEvent.click(within(row).getByText("대기"));
    await waitFor(() =>
      expect(within(row).getByText("활성")).toBeInTheDocument(),
    );
  });

  it("adds a new account row", async () => {
    await renderAccounts();
    expect(screen.getByText(/총 15개 계정/)).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: /계정 추가/ }));
    expect(await screen.findByText(/총 16개 계정/)).toBeInTheDocument();
  });

  it("bulk-deletes the selected page of accounts", async () => {
    await renderAccounts();
    const header = screen.getAllByRole("row")[0]!;
    await userEvent.click(within(header).getByRole("checkbox"));
    await userEvent.click(screen.getByRole("button", { name: /10개 삭제/ }));
    // 15 − 10 = 5 remain → header + 5 rows
    await waitFor(() => expect(screen.getAllByRole("row").length).toBe(6));
  });

  it("deletes a single account via the row trash action", async () => {
    await renderAccounts();
    const row = screen.getAllByRole("row")[1]!;
    await userEvent.click(within(row).getByTitle("삭제"));
    await waitFor(() =>
      expect(screen.queryByText("invest_king7")).not.toBeInTheDocument(),
    );
  });

  it("filters accounts by the search box", async () => {
    await renderAccounts();
    await userEvent.type(
      screen.getByPlaceholderText("계정·태그 검색"),
      "value_pick",
    );
    expect(screen.getAllByRole("row").length).toBe(2); // header + 1 match
  });

  it("saves an edited cell on blur", async () => {
    await renderAccounts();
    const row = screen.getAllByRole("row")[1]!;
    await userEvent.click(within(row).getByText("invest_king7"));
    const input = within(row).getByDisplayValue("invest_king7");
    await userEvent.clear(input);
    await userEvent.type(input, "blur_id");
    await userEvent.tab();
    expect(await screen.findByText("blur_id")).toBeInTheDocument();
  });

  it("cancels an edit on Escape", async () => {
    await renderAccounts();
    const row = screen.getAllByRole("row")[1]!;
    await userEvent.click(within(row).getByText("invest_king7"));
    const input = within(row).getByDisplayValue("invest_king7");
    await userEvent.clear(input);
    await userEvent.type(input, "discard{Escape}");
    expect(within(row).getByText("invest_king7")).toBeInTheDocument();
  });

  it("edits a password inline", async () => {
    await renderAccounts();
    const row = screen.getAllByRole("row")[1]!;
    await userEvent.click(within(row).getByTitle("보기"));
    await userEvent.click(within(row).getByText("ik7!naver22"));
    const input = within(row).getByDisplayValue("ik7!naver22");
    await userEvent.clear(input);
    await userEvent.type(input, "newpass99{Enter}");
    expect(await screen.findByText("newpass99")).toBeInTheDocument();
  });

  it("saves a password edit on blur", async () => {
    await renderAccounts();
    const row = screen.getAllByRole("row")[1]!;
    await userEvent.click(within(row).getByTitle("보기"));
    await userEvent.click(within(row).getByText("ik7!naver22"));
    const input = within(row).getByDisplayValue("ik7!naver22");
    await userEvent.clear(input);
    await userEvent.type(input, "blurpass11");
    await userEvent.tab();
    expect(await screen.findByText("blurpass11")).toBeInTheDocument();
  });

  it("cancels a password edit on Escape", async () => {
    await renderAccounts();
    const row = screen.getAllByRole("row")[1]!;
    await userEvent.click(within(row).getByTitle("보기"));
    await userEvent.click(within(row).getByText("ik7!naver22"));
    const input = within(row).getByDisplayValue("ik7!naver22");
    await userEvent.clear(input);
    await userEvent.type(input, "discard{Escape}");
    expect(within(row).getByText("ik7!naver22")).toBeInTheDocument();
  });

  it("fires excel export — opens save dialog and calls exportAccounts", async () => {
    const { save } = await import("@tauri-apps/plugin-dialog");
    vi.mocked(ipcBackend).mockClear();
    await renderAccounts();
    await userEvent.click(screen.getByRole("button", { name: /내보내기/ }));
    expect(vi.mocked(save)).toHaveBeenCalledWith(
      expect.objectContaining({ defaultPath: "계정.xlsx" }),
    );
    expect(
      vi
        .mocked(ipcBackend)
        .mock.calls.some((c) => c[0] === "export_accounts_xlsx"),
    ).toBe(true);
  });

  it("does not invoke export when save dialog is cancelled", async () => {
    const { save } = await import("@tauri-apps/plugin-dialog");
    vi.mocked(save).mockResolvedValueOnce(null);
    vi.mocked(ipcBackend).mockClear();
    await renderAccounts();
    await userEvent.click(screen.getByRole("button", { name: /내보내기/ }));
    expect(
      vi
        .mocked(ipcBackend)
        .mock.calls.some((c) => c[0] === "export_accounts_xlsx"),
    ).toBe(false);
  });

  it("shows a red error toast when the export IPC command rejects", async () => {
    const { save } = await import("@tauri-apps/plugin-dialog");
    vi.mocked(save).mockResolvedValueOnce("/tmp/계정.xlsx");
    const realImpl = vi.mocked(ipcBackend).getMockImplementation()! as (
      cmd: string,
      args?: Record<string, unknown>,
    ) => Promise<unknown>;
    vi.mocked(ipcBackend).mockImplementation((cmd, args) =>
      cmd === "export_accounts_xlsx"
        ? Promise.reject(new Error("disk full"))
        : realImpl(cmd, args),
    );
    try {
      await renderAccounts();
      await userEvent.click(screen.getByRole("button", { name: /내보내기/ }));
      await waitFor(() =>
        expect(notifShow).toHaveBeenCalledWith(
          expect.objectContaining({
            color: "red",
            message: expect.stringContaining("내보내기 실패"),
          }),
        ),
      );
      expect(notifShow).not.toHaveBeenCalledWith(
        expect.objectContaining({
          message: expect.stringContaining("내보냈어요"),
        }),
      );
      // Also logs to the activity feed with the failure message.
      await waitFor(() =>
        expect(ipc.activity.append).toHaveBeenCalledWith(
          "error",
          expect.stringContaining("내보내기"),
        ),
      );
    } finally {
      vi.mocked(ipcBackend).mockImplementation(realImpl);
    }
  });

  it("logs to activity feed when import IPC command rejects", async () => {
    const { open } = await import("@tauri-apps/plugin-dialog");
    vi.mocked(open).mockResolvedValueOnce("/tmp/계정.xlsx");
    const realImpl = vi.mocked(ipcBackend).getMockImplementation()! as (
      cmd: string,
      args?: Record<string, unknown>,
    ) => Promise<unknown>;
    vi.mocked(ipcBackend).mockImplementation((cmd, args) =>
      cmd === "import_accounts_xlsx"
        ? Promise.reject(new Error("corrupt file"))
        : realImpl(cmd, args),
    );
    try {
      await renderAccounts();
      await userEvent.click(
        screen.getByRole("button", { name: /엑셀 가져오기/ }),
      );
      await waitFor(() =>
        expect(ipc.activity.append).toHaveBeenCalledWith(
          "error",
          expect.stringContaining("가져오기"),
        ),
      );
    } finally {
      vi.mocked(ipcBackend).mockImplementation(realImpl);
    }
  });

  it("fires excel import — opens open dialog and calls importAccounts", async () => {
    const { open } = await import("@tauri-apps/plugin-dialog");
    vi.mocked(open).mockResolvedValueOnce("/tmp/계정.xlsx");
    vi.mocked(ipcBackend).mockClear();
    await renderAccounts();
    await userEvent.click(
      screen.getByRole("button", { name: /엑셀 가져오기/ }),
    );
    expect(vi.mocked(open)).toHaveBeenCalledWith(
      expect.objectContaining({ multiple: false }),
    );
    await waitFor(() =>
      expect(
        vi
          .mocked(ipcBackend)
          .mock.calls.some((c) => c[0] === "import_accounts_xlsx"),
      ).toBe(true),
    );
    await waitFor(() =>
      expect(notifShow).toHaveBeenCalledWith(
        expect.objectContaining({
          color: "green",
          message: expect.stringContaining("가져옴"),
        }),
      ),
    );
  });

  it("does not invoke import when open dialog is cancelled", async () => {
    const { open } = await import("@tauri-apps/plugin-dialog");
    vi.mocked(open).mockResolvedValueOnce(null);
    vi.mocked(ipcBackend).mockClear();
    await renderAccounts();
    await userEvent.click(
      screen.getByRole("button", { name: /엑셀 가져오기/ }),
    );
    expect(
      vi
        .mocked(ipcBackend)
        .mock.calls.some((c) => c[0] === "import_accounts_xlsx"),
    ).toBe(false);
  });

  it("adds a tag through the tag cell popover", async () => {
    await renderAccounts();
    const row = screen.getAllByRole("row")[1]!;
    await userEvent.click(within(row).getByText("대형주"));
    const tagInput = await screen.findByPlaceholderText("태그 추가");
    await userEvent.type(tagInput, "신규태그{Enter}");
    // rendered both as a TagsInput pill and a cell badge
    expect((await screen.findAllByText("신규태그")).length).toBeGreaterThan(0);
  });

  it("paginates to the second page", async () => {
    await renderAccounts();
    await userEvent.click(screen.getByRole("button", { name: "2" }));
    // 15 accounts → page 2 has 5 rows + header
    expect(screen.getAllByRole("row").length).toBe(6);
  });

  it("filters by tag via the tag select", async () => {
    await renderAccounts();
    await pickOption(0, "# 반도체"); // toolbar tag select is the first listbox
    // a1, a2 carry 반도체 → header + 2 rows
    expect(screen.getAllByRole("row").length).toBe(3);
  });

  it("changes a row's platform via its select", async () => {
    await renderAccounts();
    // combos[0] = tag filter; combos[1] = first row's platform select
    await pickOption(1, "밴드");
    expect(
      screen.getByRole("heading", { name: "계정 관리" }),
    ).toBeInTheDocument();
  });

  // 종토방(forum) 전용 선택 로그인(#228). 게시 직전 백엔드가 카페·밴드 로그인을 원자
  // 처리하므로 선택 로그인은 종토방에만 남긴다 — 선택 계정이 전부 forum일 때만 버튼 노출.
  describe("선택 로그인 (종토방 전용, #228)", () => {
    // loginId 텍스트로 해당 행을 찾아 체크박스를 토글한다(인덱스 의존 회피).
    const toggleRow = async (loginId: string) => {
      const row = screen.getByText(loginId).closest("tr")!;
      await userEvent.click(within(row).getByRole("checkbox"));
    };

    it("종토방 계정만 선택했을 때만 버튼이 보이고 카운트가 갱신된다", async () => {
      await renderAccounts();
      // 선택 전에는 버튼이 없다.
      expect(
        screen.queryByRole("button", { name: /선택 로그인/ }),
      ).not.toBeInTheDocument();
      // 종토방 계정 1개 선택 → 노출(1).
      await toggleRow("invest_king7");
      expect(
        screen.getByRole("button", { name: /선택 로그인 \(1\)/ }),
      ).toBeInTheDocument();
      // 종토방 계정 추가 선택 → 카운트(2).
      await toggleRow("value_pick");
      expect(
        screen.getByRole("button", { name: /선택 로그인 \(2\)/ }),
      ).toBeInTheDocument();
    });

    it("선택한 계정이 행에서 사라지면 카운트에서 빠진다(유령 선택 제거)", async () => {
      await renderAccounts();
      // 종토방 계정 2개 선택 → 카운트(2).
      await toggleRow("invest_king7");
      await toggleRow("value_pick");
      expect(
        screen.getByRole("button", { name: /선택 로그인 \(2\)/ }),
      ).toBeInTheDocument();
      // 선택해 둔 행 하나를 행 단위 삭제(휴지통)로 제거 — 이 경로는 sel을 비우지 않아
      // 예전엔 카운트가 (2)로 남았다. 이제 존재하지 않는 id가 정리돼 (1)이 된다.
      const row = screen.getByText("invest_king7").closest("tr")!;
      await userEvent.click(within(row).getByRole("button", { name: "삭제" }));
      await waitFor(() =>
        expect(screen.queryByText("invest_king7")).not.toBeInTheDocument(),
      );
      expect(
        screen.getByRole("button", { name: /선택 로그인 \(1\)/ }),
      ).toBeInTheDocument();
    });

    it("종토방이 아닌 계정(밴드·네이버)이 섞이면 버튼이 숨겨진다", async () => {
      await renderAccounts();
      await toggleRow("invest_king7"); // forum → 노출
      expect(
        screen.getByRole("button", { name: /선택 로그인/ }),
      ).toBeInTheDocument();

      await toggleRow("value_invest"); // + 밴드 → 숨김
      expect(
        screen.queryByRole("button", { name: /선택 로그인/ }),
      ).not.toBeInTheDocument();

      await toggleRow("value_invest"); // 밴드 해제 → 다시 노출
      expect(
        screen.getByRole("button", { name: /선택 로그인/ }),
      ).toBeInTheDocument();

      await toggleRow("money_lab"); // + 네이버 → 숨김
      expect(
        screen.queryByRole("button", { name: /선택 로그인/ }),
      ).not.toBeInTheDocument();
    });

    it("종토방 계정 로그인을 now 큐(plan.login)에 적재한다", async () => {
      await renderAccounts();
      vi.mocked(ipcBackend).mockClear();

      // checkbox[0]은 전체선택 헤더, [1]이 첫 데이터 행(invest_king7, forum).
      const checkboxes = screen.getAllByRole("checkbox");
      await userEvent.click(checkboxes[1]!);
      await userEvent.click(
        screen.getByRole("button", { name: /선택 로그인/ }),
      );

      // 자격증명을 저장(loginId 키)하고 로그인을 now 큐에 적재한다(#210). 종토방(forum)
      // 계정은 platform=naver·useAdb=true(모바일 IP 로테이션)로, 명시적 재로그인이라 force=true.
      await waitFor(() => {
        const call = vi
          .mocked(ipcBackend)
          .mock.calls.find((c) => c[0] === "add_queue_now");
        expect(call).toBeTruthy();
        const item = (call![1] as { item: { plan?: { login?: unknown[] } } })
          .item;
        expect(item.plan?.login).toEqual([
          {
            accountId: "invest_king7",
            platform: "naver",
            headless: false,
            useAdb: true,
            force: true,
          },
        ]);
      });

      // 완료 시 권위 계정 목록을 재조회(list_accounts)해 배지를 반영하고 녹색 토스트가 뜬다.
      await waitFor(
        () =>
          expect(notifShow).toHaveBeenCalledWith(
            expect.objectContaining({
              color: "green",
              message: expect.stringContaining("로그인이 끝났"),
            }),
          ),
        { timeout: 4000 },
      );
      expect(
        vi.mocked(ipcBackend).mock.calls.some((c) => c[0] === "list_accounts"),
      ).toBe(true);
    });

    it("로그인이 실패해도 배치는 완료되고 계정 목록을 재동기화한다", async () => {
      await renderAccounts();
      // 워커가 이 계정의 로그인을 error로 보고하도록 시뮬레이션한다.
      setLoginOutcomes({
        invest_king7: {
          status: "error",
          message: "아이디 또는 비밀번호가 올바르지 않습니다.",
        },
      });
      vi.mocked(ipcBackend).mockClear();

      const checkboxes = screen.getAllByRole("checkbox");
      await userEvent.click(checkboxes[1]!);
      await userEvent.click(
        screen.getByRole("button", { name: /선택 로그인/ }),
      );

      // 실패해도 배치는 완료되고(멈추지 않음) 녹색 완료 토스트가 뜬다 — 개별 실패는 토스트가
      // 아니라 상태 배지/알림 로그(자세히 보기 백트레이스)로 표시된다(#210).
      await waitFor(
        () =>
          expect(notifShow).toHaveBeenCalledWith(
            expect.objectContaining({
              color: "green",
              message: expect.stringContaining("로그인이 끝났"),
            }),
          ),
        { timeout: 4000 },
      );
      expect(
        vi.mocked(ipcBackend).mock.calls.some((c) => c[0] === "list_accounts"),
      ).toBe(true);
    });

    it("완료 폴링이 실패하면 스피너를 멈추고 빨간 토스트를 띄운다", async () => {
      await renderAccounts();
      const realInvoke = vi.mocked(ipcBackend).getMockImplementation()! as (
        cmd: string,
        args?: Record<string, unknown>,
      ) => Promise<unknown>;
      // 완료 폴링(list_queue_now)만 reject시킨다; 나머지는 정상 동작.
      vi.mocked(ipcBackend).mockImplementation((cmd, args) =>
        cmd === "list_queue_now"
          ? Promise.reject(new Error("큐 상태 조회 실패"))
          : realInvoke(cmd, args),
      );
      try {
        const checkboxes = screen.getAllByRole("checkbox");
        await userEvent.click(checkboxes[1]!);
        await userEvent.click(
          screen.getByRole("button", { name: /선택 로그인/ }),
        );

        await waitFor(
          () =>
            expect(notifShow).toHaveBeenCalledWith(
              expect.objectContaining({
                color: "red",
                message: expect.stringContaining("로그인 상태 확인 중 오류"),
              }),
            ),
          { timeout: 4000 },
        );
      } finally {
        vi.mocked(ipcBackend).mockImplementation(realInvoke);
      }
    });

    it("선택 계정에 아이디·비밀번호가 없으면 큐에 적재하지 않고 경고한다", async () => {
      await renderAccounts();
      // 첫 행(종토방)의 아이디를 비워 유효하지 않은 로그인 대상으로 만든다.
      await userEvent.click(screen.getByText("invest_king7"));
      const idInput = screen.getByDisplayValue("invest_king7");
      await userEvent.clear(idInput);
      await userEvent.tab();

      const checkboxes = screen.getAllByRole("checkbox");
      await userEvent.click(checkboxes[1]!);
      vi.mocked(ipcBackend).mockClear();
      await userEvent.click(
        screen.getByRole("button", { name: /선택 로그인/ }),
      );

      // 아이디가 없으니 now 큐에 아무것도 적재하지 않는다.
      expect(
        vi.mocked(ipcBackend).mock.calls.some((c) => c[0] === "add_queue_now"),
      ).toBe(false);
    });

    it("로그인 시작 IPC가 실패하면 활동 로그에 기록한다", async () => {
      const realImpl = vi.mocked(ipcBackend).getMockImplementation()! as (
        cmd: string,
        args?: Record<string, unknown>,
      ) => Promise<unknown>;
      vi.mocked(ipcBackend).mockImplementation((cmd, args) =>
        cmd === "add_queue_now"
          ? Promise.reject(new Error("sidecar missing"))
          : realImpl(cmd, args),
      );
      try {
        await renderAccounts();
        const checkboxes = screen.getAllByRole("checkbox");
        await userEvent.click(checkboxes[1]!);
        await userEvent.click(
          screen.getByRole("button", { name: /선택 로그인/ }),
        );
        await waitFor(() =>
          expect(ipc.activity.append).toHaveBeenCalledWith(
            "error",
            expect.stringContaining("로그인"),
          ),
        );
      } finally {
        vi.mocked(ipcBackend).mockImplementation(realImpl);
      }
    });
  });
});
