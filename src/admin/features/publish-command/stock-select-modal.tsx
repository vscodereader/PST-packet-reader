import { useMemo } from "react";

import {
  type StockSelectAdapter,
  StockSelectModalView,
} from "@/shared/ui/stock-select-modal-view";

import { api } from "../../api";

export interface AdminStockSelectModalProps {
  open: boolean;
  deviceId: string;
  preselected: string[];
  onClose: () => void;
  onConfirm: (stocks: { code: string; name: string }[]) => void;
}

// Admin(웹) 종목 선택 모달 — 공유 표현 컴포넌트에 **api 어댑터**를 주입하는 얇은 래퍼. UI는
// 데스크톱과 동일(StockSelectModalView 공유). 데이터는 서버 프록시(api.forumStocks)로 부른다.
export function StockSelectModal(props: AdminStockSelectModalProps) {
  const { deviceId } = props;
  const adapter = useMemo<StockSelectAdapter>(
    () => ({
      list: (category, exchange, market, page) =>
        api.forumStocks.list({ category, exchange, market, page }),
      search: (query, page) => api.forumStocks.search(query, page),
      // 최근 1시간 내 게시 성공 종목(#267-8, §18-8-2). 이 하위(deviceId)의 게시 결과 보고에서
      // forum + success + 1h 이내인 항목의 대상(종목명)을 모은다. 서버 게시결과(PostItemDto)에는
      // 종목 코드가 없으므로 종목명(target)으로 매칭한다(공유 뷰가 코드·이름 둘 다 매칭).
      recentPostedCodes: async () => {
        const reports = await api.postReports.list();
        const cutoff = Date.now() - 60 * 60 * 1000;
        const keys = new Set<string>();
        for (const r of reports) {
          if (r.deviceId !== deviceId) continue;
          if (r.at < cutoff) continue;
          for (const it of r.items) {
            if (
              it.platform === "forum" &&
              it.status === "success" &&
              it.target
            ) {
              keys.add(it.target);
            }
          }
        }
        return keys;
      },
    }),
    [deviceId],
  );

  return (
    <StockSelectModalView
      open={props.open}
      preselected={props.preselected}
      onClose={props.onClose}
      onConfirm={props.onConfirm}
      adapter={adapter}
    />
  );
}
