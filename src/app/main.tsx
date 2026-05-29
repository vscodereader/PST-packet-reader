import React from "react";
import ReactDOM from "react-dom/client";

import { NaverAccountList } from "@/features/naver-accounts/naver-account-list";

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <NaverAccountList />
  </React.StrictMode>,
);
