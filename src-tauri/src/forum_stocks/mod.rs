//! 종목토론방 종목 선택용 네이버 모바일(m.stock.naver.com) 종목 데이터.
//!
//! 카테고리 목록(거래대금/거래량/상승/하락/인기)·토론 랭킹·🔥 활발 종목·전체 검색을
//! 백엔드에서 합쳐 완성형 [`ForumStockPage`]로 반환한다. 프론트는 렌더만 한다.
//! 기존 `discussion_batch::search_naver_stocks`(`search_stocks` 커맨드)는 그대로 둔다.

mod client;

use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use client::ForumStockClient;

/// 종목 선택 화면 상단 카테고리 6종. 프론트는 camelCase 문자열로 전달한다.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../src/shared/bindings/")]
#[serde(rename_all = "camelCase")]
pub enum ForumStockCategory {
    Discussion,
    TradingValue,
    Popular,
    Rising,
    Falling,
    Volume,
}

/// 거래소(KRX/NXT). 프론트는 "krx"/"nxt"로 전달한다.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../src/shared/bindings/")]
#[serde(rename_all = "lowercase")]
pub enum StockExchange {
    Krx,
    Nxt,
}

impl StockExchange {
    /// 네이버 쿼리 파라미터 값(`domesticStockExchangeType` / `stockExchangeType`).
    fn as_query(self) -> &'static str {
        match self {
            StockExchange::Krx => "KRX",
            StockExchange::Nxt => "NXT",
        }
    }
}

/// 종목 한 줄(선택 가능한 종목). 프론트가 그대로 렌더한다.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../src/shared/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct ForumStock {
    pub code: String,
    pub name: String,
    /// KOSPI / KOSDAQ (표시용). 알 수 없으면 빈 문자열.
    pub exchange: String,
    /// 현재가(포맷된 문자열). 검색 결과처럼 가격이 없으면 빈 문자열.
    pub price: String,
    /// 등락률(예 "-7.68"). 없으면 빈 문자열.
    pub change_rate: String,
    /// "rising" | "falling" | "even" (색상용). 없으면 "even".
    pub change_type: String,
    /// 지금 토론 활발 종목이면 true(🔥).
    pub is_hot_discussion: bool,
}

/// 한 페이지 결과 + 페이지네이션 메타.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../src/shared/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct ForumStockPage {
    pub stocks: Vec<ForumStock>,
    pub total_count: u32,
    pub page: u32,
    pub has_next: bool,
}

/// 활발 종목 집합에 든 코드만 🔥 표시.
fn merge_hot(stocks: &mut [ForumStock], hot: &HashSet<String>) {
    for s in stocks.iter_mut() {
        if hot.contains(&s.code) {
            s.is_hot_discussion = true;
        }
    }
}

/// 카테고리 한 페이지 조회(토론은 자체 🔥, 그 외는 itemCodes 병합).
pub async fn fetch_list(
    category: ForumStockCategory,
    exchange: StockExchange,
    page: u32,
) -> Result<ForumStockPage, String> {
    let client = ForumStockClient::new();
    match category {
        ForumStockCategory::Discussion => client.fetch_discussion_page(exchange, page).await,
        _ => {
            let mut result = client.fetch_category_page(category, exchange, page).await?;
            let hot = client.fetch_hot_codes().await;
            merge_hot(&mut result.stocks, &hot);
            Ok(result)
        }
    }
}

/// 검색어 포함 국내 종목 한 페이지(🔥 병합).
pub async fn fetch_search(query: String, page: u32) -> Result<ForumStockPage, String> {
    let client = ForumStockClient::new();
    let hot = client.fetch_hot_codes().await;
    client.fetch_search_page(&query, page, &hot).await
}

/// IPC: 카테고리 목록.
#[tauri::command]
pub async fn list_forum_stocks(
    category: ForumStockCategory,
    exchange: StockExchange,
    page: u32,
) -> Result<ForumStockPage, String> {
    fetch_list(category, exchange, page).await
}

/// IPC: 전체 검색(국내).
#[tauri::command]
pub async fn search_forum_stocks(query: String, page: u32) -> Result<ForumStockPage, String> {
    fetch_search(query, page).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn category_serializes_as_camel_case() {
        let json = serde_json::to_string(&ForumStockCategory::TradingValue).unwrap();
        assert_eq!(json, "\"tradingValue\"");
    }

    #[test]
    fn exchange_query_values() {
        assert_eq!(StockExchange::Krx.as_query(), "KRX");
        assert_eq!(StockExchange::Nxt.as_query(), "NXT");
    }

    #[test]
    fn forum_stock_page_roundtrips_camel_case() {
        let page = ForumStockPage {
            stocks: vec![ForumStock {
                code: "122630".into(),
                name: "KODEX 레버리지".into(),
                exchange: "KOSPI".into(),
                price: "158,165".into(),
                change_rate: "-16.68".into(),
                change_type: "falling".into(),
                is_hot_discussion: true,
            }],
            total_count: 4396,
            page: 1,
            has_next: true,
        };
        let json = serde_json::to_string(&page).unwrap();
        assert!(json.contains("\"isHotDiscussion\":true"));
        assert!(json.contains("\"totalCount\":4396"));
        let back: ForumStockPage = serde_json::from_str(&json).unwrap();
        assert_eq!(page, back);
    }

    #[test]
    fn merge_hot_marks_only_listed_codes() {
        let mut stocks = vec![
            ForumStock {
                code: "000660".into(),
                name: "SK하이닉스".into(),
                exchange: "KOSPI".into(),
                price: "1,911,000".into(),
                change_rate: "-7.68".into(),
                change_type: "falling".into(),
                is_hot_discussion: false,
            },
            ForumStock {
                code: "122630".into(),
                name: "KODEX 레버리지".into(),
                exchange: "KOSPI".into(),
                price: "158,165".into(),
                change_rate: "-16.68".into(),
                change_type: "falling".into(),
                is_hot_discussion: false,
            },
        ];
        let hot: HashSet<String> = ["000660".to_string()].into_iter().collect();
        merge_hot(&mut stocks, &hot);
        assert!(stocks[0].is_hot_discussion);
        assert!(!stocks[1].is_hot_discussion);
    }
}
