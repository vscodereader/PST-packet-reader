// Admin 미리보기에서 보여줄 화면 식별자. 실제 앱에서는 인증 상태·라우팅으로 결정되지만,
// 미리보기 단계에선 상단 "미리보기 화면" 선택기로 아무 화면이나 바로 띄울 수 있게 한다.
export type Screen =
  | "login"
  | "signup"
  | "force-pw"
  | "devices"
  | "distribute"
  | "report"
  | "operators"
  | "change-pw";

// 로그인 전(인증) 화면들 — 사이드바 없이 전체화면으로 렌더.
export const AUTH_SCREENS: Screen[] = ["login", "signup", "force-pw"];

export interface ScreenMeta {
  value: Screen;
  label: string;
  group: "인증(로그인 전)" | "앱(로그인 후)";
}

export const PREVIEW_SCREENS: ScreenMeta[] = [
  { value: "login", label: "로그인", group: "인증(로그인 전)" },
  { value: "signup", label: "회원가입", group: "인증(로그인 전)" },
  { value: "force-pw", label: "강제 비밀번호 변경", group: "인증(로그인 전)" },
  { value: "devices", label: "기기 연결", group: "앱(로그인 후)" },
  { value: "distribute", label: "계정 분배", group: "앱(로그인 후)" },
  { value: "report", label: "결과 보고", group: "앱(로그인 후)" },
  { value: "operators", label: "운영자 관리", group: "앱(로그인 후)" },
  { value: "change-pw", label: "비밀번호 변경", group: "앱(로그인 후)" },
];
