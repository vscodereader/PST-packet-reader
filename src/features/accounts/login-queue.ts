import type { LoginTarget } from "@/shared/bindings/LoginTarget";
import type { QueueNowItem } from "@/shared/bindings/QueueNowItem";
import type { Account } from "@/shared/data/types";

/**
 * 선택한 계정들을 즉시 처리 대기열(now 큐) 아이템 1개로 묶는다(#210). 로그인도 게시와 같은
 * 큐에서 처리되도록, 게시 페이로드(naver/forum/band/comments)는 비우고 `plan.login`만 채운다.
 * 워커(`execute_item`)가 이 필드를 보고 게시 대신 계정별 로그인으로 처리한다.
 *
 * - `platform`이 band면 band.us 로그인, 그 외(forum/naver 등)는 네이버 로그인으로 분기한다
 *   (백엔드 `run_login_targets` 미러).
 * - `useAdb: true`: 네이버·밴드 모두 로그인 전에 모바일 IP를 로테이션한다(#196, #210). 밴드도
 *   봇탐지(캡차)를 완화하려고 네이버와 동일하게 ADB IP 회전을 쓴다. 폰 USB 테더링 + PATH의
 *   adb가 필요하며, 없으면 로그인이 실패한다(백엔드 process_*_account의 use_adb 분기).
 * - `force: true`: 명시적 선택 로그인이므로 유효 쿠키여도 실제 재로그인해 새 비밀번호를
 *   검증한다(이슈 #132).
 *
 * `id`는 호출부가 생성해 넘긴다(테스트 결정성을 위해 순수 함수로 유지 — `crypto.randomUUID()`).
 */
export function buildLoginNowItem(
  targets: Account[],
  id: string,
): QueueNowItem {
  const login: LoginTarget[] = targets.map((t) => ({
    accountId: t.loginId,
    platform: t.platform === "band" ? "band" : "naver",
    headless: false,
    // 네이버·밴드 모두 로그인 전 모바일 IP 로테이션(ADB)을 쓴다(#210 — 밴드 캡차 완화).
    useAdb: true,
    force: true,
  }));
  const title = `계정 로그인 ${targets.length}건`;
  return {
    id,
    title,
    kind: "post",
    state: "waiting",
    locs: targets.map((t) => ({ p: t.platform, name: t.loginId })),
    plan: {
      postId: "",
      kind: "post",
      title,
      bodyText: "",
      comments: [],
      naver: [],
      forum: [],
      band: [],
      login,
    },
  };
}
