import { useMemo } from "react";

import type { StockCandidate } from "@/shared/data/types";
import { ipc } from "@/shared/ipc";
import {
  type StockSelectAdapter,
  StockSelectModalView,
} from "@/shared/ui/stock-select-modal-view";

export interface StockCrawlModalProps {
  open: boolean;
  preselected: string[];
  onClose: () => void;
  onConfirm: (stocks: StockCandidate[]) => void;
}

// 데스크톱(Tauri) 종목 선택 모달 — 공유 표현 컴포넌트에 **ipc 어댑터**를 주입하는 얇은 래퍼.
// UI/상호작용은 전부 StockSelectModalView가 담당한다(Admin과 공유). 데이터는 Tauri ipc로 부른다.
export function StockCrawlModal(props: StockCrawlModalProps) {
  const adapter = useMemo<StockSelectAdapter>(
    () => ({
      list: (category, exchange, market, page) =>
        ipc.forumStocks.list(category, exchange, market, page),
      search: (query, page) => ipc.forumStocks.search(query, page),
      // 최근 1시간 안에 게시 성공한 종목토론방 종목 코드 집합(#267-8). 완료 로그(LogBatch)에서
      // forum + status=success + code를, 배치 시각(at)이 1시간 이내인 것만 모은다.
      recentPostedCodes: async () => {
        const batches = await ipc.logBatches.list();
        const cutoff = Date.now() - 60 * 60 * 1000;
        const codes = new Set<string>();
        for (const b of batches) {
          if (b.at < cutoff) continue;
          for (const it of b.items) {
            if (it.platform === "forum" && it.status === "success" && it.code) {
              codes.add(it.code);
            }
          }
        }
        return codes;
      },
    }),
    [],
  );

  return (
    <StockSelectModalView
      open={props.open}
      preselected={props.preselected}
      onClose={props.onClose}
      onConfirm={(stocks) =>
        props.onConfirm(
          stocks.map((s) => ({
            code: s.code,
            name: s.name,
            link: `https://stock.naver.com/domestic/stock/${s.code}/discussion?chip=all`,
          })),
        )
      }
      adapter={adapter}
    />
  );
}
