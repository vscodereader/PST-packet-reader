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

/// 시장 구분(전체/코스피/코스닥). 프론트는 "all"/"kospi"/"kosdaq"로 전달한다.
/// 토론(Discussion)은 네이버가 시장 분리를 제공하지 않으므로 이 값을 무시한다.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../src/shared/bindings/")]
#[serde(rename_all = "lowercase")]
pub enum StockMarket {
    All,
    Kospi,
    Kosdaq,
}

impl StockMarket {
    /// 네이버 `category` 파라미터 값(`all` / `KOSPI` / `KOSDAQ`).
    fn as_query(self) -> &'static str {
        match self {
            StockMarket::All => "all",
            StockMarket::Kospi => "KOSPI",
            StockMarket::Kosdaq => "KOSDAQ",
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

/// 이름에 "ETN" 또는 "레버리지"가 들어간 종목인지(선택 목록에서 숨길 대상, #267-7).
/// "ETN"은 대소문자 무시("etn"/"ETN" 모두), "레버리지"는 한글 고정으로 본다(순수 함수).
fn is_hidden_stock(stock: &ForumStock) -> bool {
    stock.name.to_uppercase().contains("ETN") || stock.name.contains("레버리지")
}

/// ETN·레버리지 종목을 페이지에서 제거한다(#267-7). 토론/카테고리/검색 모든 경로가 공통으로
/// 호출해, 사용자가 어느 탭을 보든 이 종목들이 보이지 않게 한다. total_count는 네이버 원본
/// 집계라 그대로 둔다(소수의 숨김으로 카운트가 어긋나도 표시상 무해).
fn drop_hidden_stocks(page: &mut ForumStockPage) {
    page.stocks.retain(|s| !is_hidden_stock(s));
}

/// 카테고리 한 페이지 조회(토론은 자체 🔥, 그 외는 itemCodes 병합).
///
/// 클라이언트를 주입받아 wiremock으로 테스트 가능하다. 커맨드는 `::new()`를 넘긴다.
async fn fetch_list_with(
    client: &ForumStockClient,
    category: ForumStockCategory,
    exchange: StockExchange,
    market: StockMarket,
    page: u32,
) -> Result<ForumStockPage, String> {
    let mut result = match category {
        // 토론은 시장 분리가 없어 market을 무시한다(네이버 API 한계).
        ForumStockCategory::Discussion => client.fetch_discussion_page(exchange, page).await?,
        _ => {
            let mut result = client
                .fetch_category_page(category, exchange, market, page)
                .await?;
            let hot = client.fetch_hot_codes().await;
            merge_hot(&mut result.stocks, &hot);
            result
        }
    };
    // ETN·레버리지 종목 숨김(#267-7) — 모든 카테고리 공통.
    drop_hidden_stocks(&mut result);
    Ok(result)
}

/// 검색어 포함 국내 종목 한 페이지(🔥 병합). 클라이언트 주입형.
async fn fetch_search_with(
    client: &ForumStockClient,
    query: &str,
    page: u32,
) -> Result<ForumStockPage, String> {
    let hot = client.fetch_hot_codes().await;
    let mut result = client.fetch_search_page(query, page, &hot).await?;
    // ETN·레버리지 종목 숨김(#267-7) — 검색 결과도 동일.
    drop_hidden_stocks(&mut result);
    Ok(result)
}

/// IPC: 카테고리 목록.
#[tauri::command]
pub async fn list_forum_stocks(
    category: ForumStockCategory,
    exchange: StockExchange,
    market: StockMarket,
    page: u32,
) -> Result<ForumStockPage, String> {
    fetch_list_with(&ForumStockClient::new(), category, exchange, market, page).await
}

/// IPC: 전체 검색(국내).
#[tauri::command]
pub async fn search_forum_stocks(query: String, page: u32) -> Result<ForumStockPage, String> {
    fetch_search_with(&ForumStockClient::new(), &query, page).await
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
    fn market_query_values() {
        assert_eq!(StockMarket::All.as_query(), "all");
        assert_eq!(StockMarket::Kospi.as_query(), "KOSPI");
        assert_eq!(StockMarket::Kosdaq.as_query(), "KOSDAQ");
    }

    #[test]
    fn market_deserializes_from_lowercase() {
        let m: StockMarket = serde_json::from_str("\"kospi\"").unwrap();
        assert_eq!(m, StockMarket::Kospi);
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

    #[test]
    fn hides_etn_and_leverage_stocks() {
        // #267-7: 이름에 ETN(대소문자 무시) 또는 레버리지가 들어가면 숨긴다. 일반 종목은 유지.
        let stock = |code: &str, name: &str| ForumStock {
            code: code.into(),
            name: name.into(),
            exchange: "KOSPI".into(),
            price: String::new(),
            change_rate: String::new(),
            change_type: "even".into(),
            is_hot_discussion: false,
        };
        assert!(is_hidden_stock(&stock("122630", "KODEX 레버리지")));
        assert!(is_hidden_stock(&stock(
            "530031",
            "삼성 레버리지 WTI원유 선물 ETN"
        )));
        assert!(is_hidden_stock(&stock("500001", "TRUE 코스피 etn"))); // 소문자도 거른다
        assert!(!is_hidden_stock(&stock("000660", "SK하이닉스")));
        assert!(!is_hidden_stock(&stock("005930", "삼성전자")));

        let mut page = ForumStockPage {
            stocks: vec![
                stock("005930", "삼성전자"),
                stock("122630", "KODEX 레버리지"),
                stock("500001", "TRUE 코스피 ETN"),
                stock("000660", "SK하이닉스"),
            ],
            total_count: 4,
            page: 1,
            has_next: false,
        };
        drop_hidden_stocks(&mut page);
        let codes: Vec<&str> = page.stocks.iter().map(|s| s.code.as_str()).collect();
        assert_eq!(
            codes,
            vec!["005930", "000660"],
            "ETN·레버리지만 제거되어야 한다"
        );
    }

    // ------------------------------------------------------------------
    // 오케스트레이션(클라이언트 주입) — wiremock
    // ------------------------------------------------------------------

    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    const LIST_FIXTURE: &str = include_str!("fixtures/stock_list_krx_price_top.json");

    #[tokio::test]
    async fn fetch_list_with_category_merges_hot_codes() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/front-api/domestic/stock/list"))
            .respond_with(ResponseTemplate::new(200).set_body_string(LIST_FIXTURE))
            .mount(&server)
            .await;
        // 🔥 집합에 SK하이닉스(000660) 포함. (KODEX 레버리지(122630)는 #267-7로 숨겨지므로 제외)
        Mock::given(method("GET"))
            .and(path("/front-api/discussion/rankings/itemCodes"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_string(r#"{"isSuccess":true,"result":{"itemCodes":["000660"]}}"#),
            )
            .mount(&server)
            .await;

        let client = ForumStockClient::with_base_url(server.uri());
        let page = fetch_list_with(
            &client,
            ForumStockCategory::TradingValue,
            StockExchange::Krx,
            StockMarket::All,
            1,
        )
        .await
        .unwrap();

        let sk = page.stocks.iter().find(|s| s.code == "000660").unwrap();
        assert!(
            sk.is_hot_discussion,
            "🔥 집합의 SK하이닉스는 표시되어야 한다"
        );
        // KODEX 레버리지(122630)는 ETN/레버리지 숨김 정책으로 목록에서 빠진다(#267-7).
        assert!(
            page.stocks.iter().all(|s| s.code != "122630"),
            "레버리지 종목은 선택 목록에서 숨겨져야 한다"
        );
    }

    #[tokio::test]
    async fn fetch_list_with_discussion_uses_ranking_branch() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/front-api/discussion/ranking/list/price"))
            .respond_with(ResponseTemplate::new(200).set_body_string(
                r#"{"isSuccess":true,"result":{"totalCount":100,"hasNextPage":false,"itemCodes":["000660"]}}"#,
            ))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/front-api/realTime/marketPrice"))
            .respond_with(ResponseTemplate::new(200).set_body_string(
                r#"{"isSuccess":true,"result":{"datas":[{"itemCode":"000660","stockName":"SK하이닉스","stockExchangeType":{"nameKor":"코스피"},"closePrice":"1,911,000","compareToPreviousPrice":{"name":"FALLING"},"fluctuationsRatio":"-7.68"}]}}"#,
            ))
            .mount(&server)
            .await;

        let client = ForumStockClient::with_base_url(server.uri());
        let page = fetch_list_with(
            &client,
            ForumStockCategory::Discussion,
            StockExchange::Krx,
            StockMarket::All,
            1,
        )
        .await
        .unwrap();

        assert_eq!(page.stocks.len(), 1);
        assert_eq!(page.stocks[0].name, "SK하이닉스");
        // 토론 탭은 전부 🔥.
        assert!(page.stocks[0].is_hot_discussion);
    }

    #[tokio::test]
    async fn fetch_search_with_filters_and_marks_hot() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/front-api/search"))
            .respond_with(ResponseTemplate::new(200).set_body_string(
                r#"{"isSuccess":true,"result":{"totalCount":2,"items":[
                  {"code":"069500","name":"KODEX 200","category":"stock","nationCode":"KOR","typeName":"코스피"},
                  {"code":"KO","name":"코카콜라","category":"stock","nationCode":"USA","typeName":"뉴욕 거래소"}
                ]}}"#,
            ))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/front-api/discussion/rankings/itemCodes"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_string(r#"{"isSuccess":true,"result":{"itemCodes":["069500"]}}"#),
            )
            .mount(&server)
            .await;

        let client = ForumStockClient::with_base_url(server.uri());
        let page = fetch_search_with(&client, "ko", 1).await.unwrap();

        assert_eq!(page.stocks.len(), 1, "해외(USA) 종목은 제외");
        assert_eq!(page.stocks[0].code, "069500");
        assert!(page.stocks[0].is_hot_discussion, "🔥 집합 포함 → 표시");
    }
}
