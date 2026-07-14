import React from "react";
import ReactDOM from "react-dom/client";

import { AppProviders } from "@/app/providers";

import { AdminApp } from "./admin-app";

// Admin 웹 미리보기 전용 엔트리. Tauri(`ipc`/`invoke`)에 의존하지 않으므로 일반
// 브라우저에서 바로 열린다(http://localhost:1420/admin.html). 기존 앱의 테마·토스트
// 프로바이더(AppProviders)를 그대로 재사용해 룩앤필을 일치시킨다.
ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <AppProviders>
      <AdminApp />
    </AppProviders>
  </React.StrictMode>,
);
