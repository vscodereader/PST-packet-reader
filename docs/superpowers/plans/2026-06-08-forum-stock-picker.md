# 종목 선택 화면 재디자인 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 종목토론방 종목 선택 화면을 네이버 모바일(`m.stock.naver.com` priceTop) 페이지처럼 재디자인하고, 검색이 검색어 포함 국내 종목을 빠짐없이 반환하도록 백엔드를 교체한다.

**Architecture:** 신규 Rust 모듈 `forum_stocks`가 네이버 모바일 `front-api`를 호출해 카테고리 목록·토론 랭킹·🔥 itemCodes·전체 검색을 가져와 종목별 `is_hot_discussion`까지 채운 완성형 `ForumStockPage`를 IPC로 반환한다. 프론트(`stock-crawl-modal.tsx`)는 받아서 렌더만 한다. 기존 `search_stocks`/게시 엔진/로그인은 불변.

**Tech Stack:** Rust(reqwest async, serde, ts-rs, wiremock 테스트) · React + Mantine + Vitest · Tauri IPC.

**Spec:** `docs/superpowers/specs/2026-06-08-forum-stock-picker-design.md`

---

## File Structure

- **Create** `src-tauri/src/forum_stocks/mod.rs` — 타입(`ForumStock`, `ForumStockPage`, `ForumStockCategory`, `StockExchange`) + 고수준 오케스트레이션(`list_forum_stocks`/`search_forum_stocks`).
- **Create** `src-tauri/src/forum_stocks/client.rs` — `ForumStockClient`(`with_base_url` 주입형) + 네이버 호출/파싱 + wiremock 테스트.
- **Create** `src-tauri/src/forum_stocks/fixtures/*.json` — 테스트 픽스처(실제 응답 축소판).
- **Modify** `src-tauri/src/lib.rs` — 모듈 선언 + 커맨드 2개 등록.
- **Modify** `src/shared/ipc/index.ts` — `forumStocks` 파사드 추가.
- **Create(자동)** `src/shared/bindings/ForumStock.ts` 등 — ts-rs 생성.
- **Modify** `src/features/posts/stock-crawl-modal.tsx` — 모달 재디자인(계약 유지).
- **Create** `src/features/posts/stock-crawl-modal.test.tsx` 갱신 — 신규 동작 테스트(기존 파일 대체).
- **Modify** `src/features/posts/publish-modal.tsx` — 선택 칩 이름 보강(최소).

> 네이버 호출은 모두 `m.stock.naver.com` 호스트. 모든 함수는 `base_url`을 주입받아 wiremock으로 테스트한다(기존 `naver_cafe` 클라이언트와 동일 패턴).

---

## Task 1: 백엔드 타입 + ts-rs 바인딩

**Files:**

- Create: `src-tauri/src/forum_stocks/mod.rs`
- Modify: `src-tauri/src/lib.rs` (모듈 선언만)

- [ ] **Step 1: 모듈 선언 추가**

`src-tauri/src/lib.rs`의 다른 `mod` 선언들 근처(예: `mod discussion_batch;` 아래)에 추가:

```rust
mod forum_stocks;
```

- [ ] **Step 2: 실패 테스트 작성**

`src-tauri/src/forum_stocks/mod.rs` 생성, 아래 전체 내용:

```rust
//! 종목토론방 종목 선택용 네이버 모바일(m.stock.naver.com) 종목 데이터.
//!
//! 카테고리 목록(거래대금/거래량/상승/하락/인기)·토론 랭킹·🔥 활발 종목·전체 검색을
//! 백엔드에서 합쳐 완성형 [`ForumStockPage`]로 반환한다. 프론트는 렌더만 한다.
//! 기존 `discussion_batch::search_naver_stocks`(`search_stocks` 커맨드)는 그대로 둔다.

mod client;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

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

#[cfg(test)]
mod tests {
    use super::*;

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
}
```

> `mod client;`는 Task 2에서 파일을 만들기 전까지 컴파일 에러를 낸다. Task 2 전까지 일시적으로 `// mod client;` 주석 처리해도 되지만, 바로 Task 2로 이어서 진행하는 것을 권장한다.

- [ ] **Step 3: 테스트 실패 확인**

Run: `cd src-tauri && cargo test --lib forum_stocks::tests 2>&1 | tail -20`
Expected: `client.rs` 부재로 컴파일 실패(또는 `mod client;` 주석 시 테스트 통과). Task 2를 끝내면 정상화.

- [ ] **Step 4: 커밋**

```bash
git add src-tauri/src/forum_stocks/mod.rs src-tauri/src/lib.rs
git commit -m "feat(forum): ForumStock 종목 타입·카테고리·거래소 enum 추가 (#157)"
```

---

## Task 2: `ForumStockClient` 골격 + 매핑 (순수 함수)

**Files:**

- Create: `src-tauri/src/forum_stocks/client.rs`

- [ ] **Step 1: 실패 테스트 + 골격 작성**

`src-tauri/src/forum_stocks/client.rs` 생성:

```rust
//! 네이버 모바일(m.stock.naver.com) front-api 호출 클라이언트.
//! 테스트는 [`ForumStockClient::with_base_url`]로 wiremock 서버를 주입한다.

use std::collections::HashSet;

use serde_json::Value;

use super::{ForumStock, ForumStockCategory, ForumStockPage, StockExchange};

const HOST: &str = "https://m.stock.naver.com";
const PAGE_SIZE: u32 = 50;
const SEARCH_SIZE: u32 = 20;
const SEARCH_TARGET: &str = "stock,index,marketindicator,coin,ipo,fund";

/// 카테고리 → `/front-api/domestic/stock/list` 의 `sortType` 값.
/// 토론(Discussion)은 별도 엔드포인트라 여기서 None.
fn sort_type(category: ForumStockCategory) -> Option<&'static str> {
    match category {
        ForumStockCategory::TradingValue => Some("priceTop"),
        ForumStockCategory::Volume => Some("quantTop"),
        ForumStockCategory::Rising => Some("up"),
        ForumStockCategory::Falling => Some("down"),
        ForumStockCategory::Popular => Some("searchTop"),
        ForumStockCategory::Discussion => None,
    }
}

/// fluctuationsType / compareToPreviousPrice.name → 색상 키.
fn change_type(raw: &str) -> &'static str {
    match raw {
        "RISING" => "rising",
        "FALLING" => "falling",
        _ => "even",
    }
}

/// 6자리 영숫자 종목 코드만 허용(ETF 특수코드 `0193T0` 포함). 숫자 강제 금지.
fn looks_like_code(value: &str) -> bool {
    value.chars().count() == 6 && value.chars().all(|c| c.is_ascii_alphanumeric())
}

pub struct ForumStockClient {
    base_url: String,
    http: reqwest::Client,
}

impl ForumStockClient {
    pub fn new() -> Self {
        Self::with_base_url(HOST)
    }

    pub fn with_base_url(base_url: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into(),
            http: crate::naver_cafe::shared_http_client(),
        }
    }
}

impl Default for ForumStockClient {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sort_type_maps_each_category() {
        assert_eq!(sort_type(ForumStockCategory::TradingValue), Some("priceTop"));
        assert_eq!(sort_type(ForumStockCategory::Volume), Some("quantTop"));
        assert_eq!(sort_type(ForumStockCategory::Rising), Some("up"));
        assert_eq!(sort_type(ForumStockCategory::Falling), Some("down"));
        assert_eq!(sort_type(ForumStockCategory::Popular), Some("searchTop"));
        assert_eq!(sort_type(ForumStockCategory::Discussion), None);
    }

    #[test]
    fn change_type_maps_colors() {
        assert_eq!(change_type("RISING"), "rising");
        assert_eq!(change_type("FALLING"), "falling");
        assert_eq!(change_type("EVEN"), "even");
        assert_eq!(change_type("anything"), "even");
    }

    #[test]
    fn looks_like_code_allows_alphanumeric_six() {
        assert!(looks_like_code("005930"));
        assert!(looks_like_code("0193T0")); // ETF 특수코드
        assert!(!looks_like_code("00593"));
        assert!(!looks_like_code("0059300"));
        assert!(!looks_like_code("00-930"));
    }
}
```

- [ ] **Step 2: 테스트 실행**

Run: `cd src-tauri && cargo test --lib forum_stocks 2>&1 | tail -20`
Expected: PASS (Task 1 테스트 + Task 2 순수함수 테스트 통과). `mod client;` 주석을 풀었는지 확인.

- [ ] **Step 3: 커밋**

```bash
git add src-tauri/src/forum_stocks/
git commit -m "feat(forum): ForumStockClient 골격 + sortType/색상/코드검증 매핑 (#157)"
```

---

## Task 3: 카테고리 목록 fetch + parse (wiremock)

**Files:**

- Modify: `src-tauri/src/forum_stocks/client.rs`
- Create: `src-tauri/src/forum_stocks/fixtures/stock_list_krx_price_top.json`

- [ ] **Step 1: 픽스처 작성**

`src-tauri/src/forum_stocks/fixtures/stock_list_krx_price_top.json`:

```json
{
  "isSuccess": true,
  "result": {
    "totalCount": 4396,
    "page": 1,
    "pageSize": 50,
    "stocks": [
      {
        "id": "000660",
        "name": "SK하이닉스",
        "itemCode": "000660",
        "stockExchangeType": "KOSPI",
        "currentPrice": 1911000,
        "fluctuationsType": "FALLING",
        "fluctuationsRatio": "-7.68"
      },
      {
        "id": "122630",
        "name": "KODEX 레버리지",
        "itemCode": "122630",
        "stockExchangeType": "KOSPI",
        "currentPrice": 158165,
        "fluctuationsType": "FALLING",
        "fluctuationsRatio": "-16.68"
      }
    ]
  }
}
```

- [ ] **Step 2: 실패 테스트 작성**

`client.rs`의 `#[cfg(test)] mod tests` 안에 추가(상단에 wiremock import도 추가):

```rust
    use wiremock::matchers::{method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    const LIST_FIXTURE: &str = include_str!("fixtures/stock_list_krx_price_top.json");

    #[tokio::test]
    async fn fetch_category_page_parses_stocks_and_meta() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/front-api/domestic/stock/list"))
            .and(query_param("sortType", "priceTop"))
            .and(query_param("domesticStockExchangeType", "KRX"))
            .and(query_param("page", "1"))
            .respond_with(ResponseTemplate::new(200).set_body_string(LIST_FIXTURE))
            .mount(&server)
            .await;

        let client = ForumStockClient::with_base_url(server.uri());
        let page = client
            .fetch_category_page(ForumStockCategory::TradingValue, StockExchange::Krx, 1)
            .await
            .unwrap();

        assert_eq!(page.total_count, 4396);
        assert_eq!(page.stocks.len(), 2);
        let kodex = page.stocks.iter().find(|s| s.code == "122630").unwrap();
        assert_eq!(kodex.name, "KODEX 레버리지");
        assert_eq!(kodex.exchange, "KOSPI");
        assert_eq!(kodex.price, "158,165");
        assert_eq!(kodex.change_rate, "-16.68");
        assert_eq!(kodex.change_type, "falling");
        // 🔥는 이 함수 단계에서는 아직 false(병합 전).
        assert!(!kodex.is_hot_discussion);
        assert!(page.has_next); // 50 < 4396
    }
```

- [ ] **Step 3: 실패 확인**

Run: `cd src-tauri && cargo test --lib forum_stocks::client::tests::fetch_category_page 2>&1 | tail -20`
Expected: FAIL — `fetch_category_page` 미정의.

- [ ] **Step 4: 구현 추가**

`client.rs`의 `impl ForumStockClient`에 추가:

```rust
    /// `m.stock.naver.com` GET 후 JSON 파싱(공통). 실패는 사람이 읽을 메시지 문자열로.
    async fn get_json(&self, path_and_query: &str) -> Result<Value, String> {
        let url = format!("{}{}", self.base_url, path_and_query);
        let res = self
            .http
            .get(&url)
            .header("accept", "application/json, text/plain, */*")
            .header("referer", "https://m.stock.naver.com/domestic/home/priceTop/total")
            .header("user-agent", "Mozilla/5.0")
            .send()
            .await
            .map_err(|e| format!("종목 조회 전송 오류: {e}"))?;
        if !res.status().is_success() {
            return Err(format!("종목 조회 HTTP 오류: {}", res.status().as_u16()));
        }
        let text = res.text().await.map_err(|e| format!("종목 응답 읽기 오류: {e}"))?;
        serde_json::from_str(&text).map_err(|e| format!("종목 응답 파싱 오류: {e}"))
    }

    /// 천 단위 콤마 포맷(현재가 정수 → "158,165"). 0/음수도 안전.
    fn format_price(n: i64) -> String {
        let neg = n < 0;
        let digits = n.unsigned_abs().to_string();
        let mut out = String::new();
        for (i, ch) in digits.chars().enumerate() {
            if i > 0 && (digits.len() - i) % 3 == 0 {
                out.push(',');
            }
            out.push(ch);
        }
        if neg {
            format!("-{out}")
        } else {
            out
        }
    }

    /// 카테고리(토론 제외) 한 페이지 조회. 🔥는 아직 채우지 않는다(병합은 상위에서).
    pub async fn fetch_category_page(
        &self,
        category: ForumStockCategory,
        exchange: StockExchange,
        page: u32,
    ) -> Result<ForumStockPage, String> {
        let sort = sort_type(category).ok_or("토론 카테고리는 별도 경로를 사용합니다")?;
        let q = format!(
            "/front-api/domestic/stock/list?sortType={sort}&category=all&domesticStockExchangeType={}&page={page}&pageSize={PAGE_SIZE}",
            exchange.as_query()
        );
        let value = self.get_json(&q).await?;
        let result = &value["result"];
        let total_count = result["totalCount"].as_u64().unwrap_or(0) as u32;
        let mut stocks = Vec::new();
        if let Some(items) = result["stocks"].as_array() {
            for it in items {
                let code = it["itemCode"].as_str().unwrap_or("").to_string();
                if !looks_like_code(&code) {
                    continue;
                }
                let price = match it["currentPrice"].as_i64() {
                    Some(n) => Self::format_price(n),
                    None => String::new(),
                };
                stocks.push(ForumStock {
                    name: it["name"].as_str().unwrap_or(&code).to_string(),
                    exchange: it["stockExchangeType"].as_str().unwrap_or("").to_string(),
                    price,
                    change_rate: it["fluctuationsRatio"].as_str().unwrap_or("").to_string(),
                    change_type: change_type(it["fluctuationsType"].as_str().unwrap_or("")).to_string(),
                    is_hot_discussion: false,
                    code,
                });
            }
        }
        let has_next = (page * PAGE_SIZE) < total_count;
        Ok(ForumStockPage { stocks, total_count, page, has_next })
    }
```

- [ ] **Step 5: 통과 확인**

Run: `cd src-tauri && cargo test --lib forum_stocks 2>&1 | tail -20`
Expected: PASS.

- [ ] **Step 6: 커밋**

```bash
git add src-tauri/src/forum_stocks/
git commit -m "feat(forum): 카테고리 목록 fetch+parse (가격 포맷·색상) (#157)"
```

---

## Task 4: 🔥 활발 종목 itemCodes fetch (wiremock)

**Files:**

- Modify: `src-tauri/src/forum_stocks/client.rs`

- [ ] **Step 1: 실패 테스트 작성**

`client.rs` 테스트 모듈에 추가:

```rust
    #[tokio::test]
    async fn fetch_hot_codes_returns_set() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/front-api/discussion/rankings/itemCodes"))
            .respond_with(ResponseTemplate::new(200).set_body_string(
                r#"{"isSuccess":true,"result":{"itemCodes":["000660","035420"]}}"#,
            ))
            .mount(&server)
            .await;

        let client = ForumStockClient::with_base_url(server.uri());
        let hot = client.fetch_hot_codes().await;
        assert!(hot.contains("000660"));
        assert!(hot.contains("035420"));
        assert!(!hot.contains("005930"));
    }

    #[tokio::test]
    async fn fetch_hot_codes_empty_on_error() {
        let client = ForumStockClient::with_base_url("http://127.0.0.1:1");
        // 연결 실패 → 🔥는 비치명적이므로 빈 집합.
        assert!(client.fetch_hot_codes().await.is_empty());
    }
```

- [ ] **Step 2: 실패 확인**

Run: `cd src-tauri && cargo test --lib forum_stocks::client::tests::fetch_hot_codes 2>&1 | tail -15`
Expected: FAIL — `fetch_hot_codes` 미정의.

- [ ] **Step 3: 구현 추가**

`impl ForumStockClient`에 추가:

```rust
    /// 지금 토론 활발한 종목코드 집합(🔥). 실패해도 비치명적 → 빈 집합 반환.
    pub async fn fetch_hot_codes(&self) -> HashSet<String> {
        let value = match self.get_json("/front-api/discussion/rankings/itemCodes").await {
            Ok(v) => v,
            Err(_) => return HashSet::new(),
        };
        value["result"]["itemCodes"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default()
    }
```

- [ ] **Step 4: 통과 확인**

Run: `cd src-tauri && cargo test --lib forum_stocks 2>&1 | tail -15`
Expected: PASS.

- [ ] **Step 5: 커밋**

```bash
git add src-tauri/src/forum_stocks/client.rs
git commit -m "feat(forum): 🔥 활발 종목 itemCodes fetch (비치명적) (#157)"
```

---

## Task 5: 토론 랭킹 codes + 배치 메타 (wiremock)

**Files:**

- Modify: `src-tauri/src/forum_stocks/client.rs`

- [ ] **Step 1: 실패 테스트 작성**

`client.rs` 테스트 모듈에 추가:

```rust
    #[tokio::test]
    async fn fetch_discussion_page_orders_by_ranking_with_meta() {
        let server = MockServer::start().await;
        // 토론 랭킹: 순위순 itemCodes
        Mock::given(method("GET"))
            .and(path("/front-api/discussion/ranking/list/price"))
            .and(query_param("stockExchangeType", "KRX"))
            .respond_with(ResponseTemplate::new(200).set_body_string(
                r#"{"isSuccess":true,"result":{"totalCount":100,"hasNextPage":true,"itemCodes":["018260","000660"]}}"#,
            ))
            .mount(&server)
            .await;
        // 배치 메타: 이름/가격/등락
        Mock::given(method("GET"))
            .and(path("/front-api/realTime/marketPrice"))
            .respond_with(ResponseTemplate::new(200).set_body_string(
                r#"{"isSuccess":true,"result":{"datas":[
                   {"itemCode":"000660","stockName":"SK하이닉스","stockExchangeType":{"nameKor":"코스피"},
                    "closePrice":"1,911,000","compareToPreviousPrice":{"name":"FALLING"},"fluctuationsRatio":"-7.68"},
                   {"itemCode":"018260","stockName":"삼성에스디에스","stockExchangeType":{"nameKor":"코스피"},
                    "closePrice":"180,000","compareToPreviousPrice":{"name":"RISING"},"fluctuationsRatio":"1.10"}
                ]}}"#,
            ))
            .mount(&server)
            .await;

        let client = ForumStockClient::with_base_url(server.uri());
        let page = client
            .fetch_discussion_page(StockExchange::Krx, 1)
            .await
            .unwrap();

        // 순위 순서 보존: 018260이 먼저
        assert_eq!(page.stocks[0].code, "018260");
        assert_eq!(page.stocks[0].name, "삼성에스디에스");
        assert_eq!(page.stocks[0].exchange, "코스피");
        assert_eq!(page.stocks[0].price, "180,000");
        assert_eq!(page.stocks[0].change_type, "rising");
        // 토론 탭은 전부 🔥
        assert!(page.stocks.iter().all(|s| s.is_hot_discussion));
        assert!(page.has_next);
    }
```

- [ ] **Step 2: 실패 확인**

Run: `cd src-tauri && cargo test --lib forum_stocks::client::tests::fetch_discussion_page 2>&1 | tail -15`
Expected: FAIL — `fetch_discussion_page` 미정의.

- [ ] **Step 3: 구현 추가**

`impl ForumStockClient`에 추가:

```rust
    /// 토론 랭킹 한 페이지(순위순 itemCodes) + 배치 메타 보강. 전부 🔥.
    pub async fn fetch_discussion_page(
        &self,
        exchange: StockExchange,
        page: u32,
    ) -> Result<ForumStockPage, String> {
        let q = format!(
            "/front-api/discussion/ranking/list/price?nationType=KOR&size={PAGE_SIZE}&stockExchangeType={}&page={page}",
            exchange.as_query()
        );
        let value = self.get_json(&q).await?;
        let result = &value["result"];
        let total_count = result["totalCount"].as_u64().unwrap_or(0) as u32;
        let has_next = result["hasNextPage"].as_bool().unwrap_or(false);
        let codes: Vec<String> = result["itemCodes"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str().map(str::to_string))
                    .filter(|c| looks_like_code(c))
                    .collect()
            })
            .unwrap_or_default();

        let meta = self.fetch_meta(&codes).await; // code → (name, exchange, price, rate, type)
        let stocks = codes
            .into_iter()
            .map(|code| {
                let m = meta.get(&code);
                ForumStock {
                    name: m.map(|m| m.0.clone()).unwrap_or_else(|| code.clone()),
                    exchange: m.map(|m| m.1.clone()).unwrap_or_default(),
                    price: m.map(|m| m.2.clone()).unwrap_or_default(),
                    change_rate: m.map(|m| m.3.clone()).unwrap_or_default(),
                    change_type: m.map(|m| m.4.clone()).unwrap_or_else(|| "even".into()),
                    is_hot_discussion: true,
                    code,
                }
            })
            .collect();
        Ok(ForumStockPage { stocks, total_count, page, has_next })
    }

    /// 여러 종목코드의 이름/거래소/현재가/등락을 배치로 조회. 실패 시 빈 맵.
    /// 반환: code → (name, exchangeKor, price, change_rate, change_type)
    async fn fetch_meta(
        &self,
        codes: &[String],
    ) -> std::collections::HashMap<String, (String, String, String, String, String)> {
        use std::collections::HashMap;
        if codes.is_empty() {
            return HashMap::new();
        }
        let joined = codes.join(",");
        let q = format!(
            "/front-api/realTime/marketPrice?itemCodes={joined}&endType=stock&stockType=domestic"
        );
        let value = match self.get_json(&q).await {
            Ok(v) => v,
            Err(_) => return HashMap::new(),
        };
        let mut map = HashMap::new();
        if let Some(arr) = value["result"]["datas"].as_array() {
            for d in arr {
                let code = d["itemCode"].as_str().unwrap_or("").to_string();
                if code.is_empty() {
                    continue;
                }
                map.insert(
                    code,
                    (
                        d["stockName"].as_str().unwrap_or("").to_string(),
                        d["stockExchangeType"]["nameKor"].as_str().unwrap_or("").to_string(),
                        d["closePrice"].as_str().unwrap_or("").to_string(),
                        d["fluctuationsRatio"].as_str().unwrap_or("").to_string(),
                        change_type(d["compareToPreviousPrice"]["name"].as_str().unwrap_or("")).to_string(),
                    ),
                );
            }
        }
        map
    }
```

- [ ] **Step 4: 통과 확인**

Run: `cd src-tauri && cargo test --lib forum_stocks 2>&1 | tail -15`
Expected: PASS.

- [ ] **Step 5: 커밋**

```bash
git add src-tauri/src/forum_stocks/client.rs
git commit -m "feat(forum): 토론 랭킹 + realTime/marketPrice 배치 메타 보강 (#157)"
```

---

## Task 6: 전체 검색 fetch + 국내 필터 (wiremock)

**Files:**

- Modify: `src-tauri/src/forum_stocks/client.rs`

- [ ] **Step 1: 실패 테스트 작성**

`client.rs` 테스트 모듈에 추가:

```rust
    #[tokio::test]
    async fn fetch_search_page_keeps_only_domestic_stocks() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/front-api/search"))
            .and(query_param("q", "ko"))
            .and(query_param("page", "1"))
            .respond_with(ResponseTemplate::new(200).set_body_string(
                r#"{"isSuccess":true,"result":{"totalCount":1161,"items":[
                  {"code":"KO","name":"코카콜라","category":"stock","nationCode":"USA","typeName":"뉴욕 거래소"},
                  {"code":"069500","name":"KODEX 200","category":"stock","nationCode":"KOR","typeName":"코스피"},
                  {"code":"0193T0","name":"KODEX SK하이닉스단일종목레버리지","category":"stock","nationCode":"KOR","typeName":"코스피"},
                  {"code":"KOSPI","name":"코스피지수","category":"index","nationCode":"KOR","typeName":"지수"}
                ]}}"#,
            ))
            .mount(&server)
            .await;

        let client = ForumStockClient::with_base_url(server.uri());
        let hot: std::collections::HashSet<String> = std::collections::HashSet::new();
        let page = client.fetch_search_page("ko", 1, &hot).await.unwrap();

        let codes: Vec<&str> = page.stocks.iter().map(|s| s.code.as_str()).collect();
        assert!(codes.contains(&"069500")); // 국내 ETF
        assert!(codes.contains(&"0193T0")); // 영숫자 코드 허용
        assert!(!codes.contains(&"KO")); // 해외 제외
        assert!(!codes.contains(&"KOSPI")); // 지수 제외(category!=stock)
        assert_eq!(page.total_count, 1161);
        assert!(page.has_next); // 20 < 1161
    }
```

- [ ] **Step 2: 실패 확인**

Run: `cd src-tauri && cargo test --lib forum_stocks::client::tests::fetch_search_page 2>&1 | tail -15`
Expected: FAIL — `fetch_search_page` 미정의.

- [ ] **Step 3: 구현 추가**

`impl ForumStockClient`에 추가:

```rust
    /// 검색어 포함 **국내 종목**(nationCode=KOR & category=stock)만 한 페이지 조회.
    /// 🔥는 인자로 받은 활발 종목 집합으로 채운다. 가격은 검색 응답에 없어 빈 값.
    pub async fn fetch_search_page(
        &self,
        query: &str,
        page: u32,
        hot: &HashSet<String>,
    ) -> Result<ForumStockPage, String> {
        let encoded = urlencoding::encode(query);
        let q = format!(
            "/front-api/search?q={encoded}&size={SEARCH_SIZE}&target={SEARCH_TARGET}&page={page}"
        );
        let value = self.get_json(&q).await?;
        let result = &value["result"];
        let total_count = result["totalCount"].as_u64().unwrap_or(0) as u32;
        let mut stocks = Vec::new();
        if let Some(items) = result["items"].as_array() {
            for it in items {
                let nation = it["nationCode"].as_str().unwrap_or("");
                let category = it["category"].as_str().unwrap_or("");
                if nation != "KOR" || category != "stock" {
                    continue;
                }
                let code = it["code"].as_str().unwrap_or("").to_string();
                if !looks_like_code(&code) {
                    continue;
                }
                stocks.push(ForumStock {
                    name: it["name"].as_str().unwrap_or(&code).to_string(),
                    exchange: it["typeName"].as_str().unwrap_or("").to_string(),
                    price: String::new(),
                    change_rate: String::new(),
                    change_type: "even".to_string(),
                    is_hot_discussion: hot.contains(&code),
                    code,
                });
            }
        }
        let has_next = (page * SEARCH_SIZE) < total_count;
        Ok(ForumStockPage { stocks, total_count, page, has_next })
    }
```

> `urlencoding` 크레이트가 `src-tauri/Cargo.toml`에 없으면 추가한다: `urlencoding = "2"`. (이미 reqwest가 끌어올 수 있으나 직접 의존 명시 권장.) 있으면 생략.

- [ ] **Step 4: 통과 확인**

Run: `cd src-tauri && cargo test --lib forum_stocks 2>&1 | tail -15`
Expected: PASS. (urlencoding 미존재로 실패 시 Cargo.toml 추가 후 재실행.)

- [ ] **Step 5: 커밋**

```bash
git add src-tauri/src/forum_stocks/client.rs src-tauri/Cargo.toml src-tauri/Cargo.lock
git commit -m "feat(forum): 전체 검색 fetch + 국내(KOR·stock) 필터 (#157)"
```

---

## Task 7: 고수준 오케스트레이션 (🔥 병합) + 커맨드

**Files:**

- Modify: `src-tauri/src/forum_stocks/mod.rs`
- Modify: `src-tauri/src/forum_stocks/client.rs` (필요 메서드 pub 노출 — 이미 pub)

- [ ] **Step 1: 실패 테스트 작성**

`mod.rs`의 `#[cfg(test)] mod tests`에 추가(상단 `use` 보강):

```rust
    use std::collections::HashSet;

    #[test]
    fn merge_hot_marks_only_listed_codes() {
        let mut stocks = vec![
            ForumStock {
                code: "000660".into(), name: "SK하이닉스".into(), exchange: "KOSPI".into(),
                price: "1,911,000".into(), change_rate: "-7.68".into(),
                change_type: "falling".into(), is_hot_discussion: false,
            },
            ForumStock {
                code: "122630".into(), name: "KODEX 레버리지".into(), exchange: "KOSPI".into(),
                price: "158,165".into(), change_rate: "-16.68".into(),
                change_type: "falling".into(), is_hot_discussion: false,
            },
        ];
        let hot: HashSet<String> = ["000660".to_string()].into_iter().collect();
        merge_hot(&mut stocks, &hot);
        assert!(stocks[0].is_hot_discussion);
        assert!(!stocks[1].is_hot_discussion);
    }
```

- [ ] **Step 2: 실패 확인**

Run: `cd src-tauri && cargo test --lib forum_stocks::tests::merge_hot 2>&1 | tail -15`
Expected: FAIL — `merge_hot` 미정의.

- [ ] **Step 3: 구현 + 커맨드 추가**

`mod.rs`(테스트 모듈 위, `use` 아래)에 추가:

```rust
use std::collections::HashSet;

use client::ForumStockClient;

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
```

- [ ] **Step 4: 통과 확인**

Run: `cd src-tauri && cargo test --lib forum_stocks 2>&1 | tail -15`
Expected: PASS.

- [ ] **Step 5: 커밋**

```bash
git add src-tauri/src/forum_stocks/
git commit -m "feat(forum): list/search 오케스트레이션 + 🔥 병합 + IPC 커맨드 (#157)"
```

---

## Task 8: 커맨드 등록 + ts-rs 바인딩 생성

**Files:**

- Modify: `src-tauri/src/lib.rs`

- [ ] **Step 1: invoke_handler에 커맨드 등록**

`src-tauri/src/lib.rs`의 `tauri::generate_handler![ ... ]` 목록에서 `search_stocks` 다음 줄에 추가:

```rust
            forum_stocks::list_forum_stocks,
            forum_stocks::search_forum_stocks,
```

- [ ] **Step 2: 컴파일 + 전체 Rust 테스트**

Run: `cd src-tauri && cargo test --lib 2>&1 | tail -20`
Expected: PASS (전체). 등록 누락/시그니처 오류 없으면 통과.

- [ ] **Step 3: ts-rs 바인딩 생성**

Run: `pnpm gen:bindings 2>&1 | tail -5 && ls src/shared/bindings/ | grep -i forum`
Expected: `ForumStock.ts`, `ForumStockPage.ts`, `ForumStockCategory.ts`, `StockExchange.ts` 생성됨.

- [ ] **Step 4: 커밋**

```bash
git add src-tauri/src/lib.rs src/shared/bindings/
git commit -m "feat(forum): IPC 커맨드 등록 + ts-rs 바인딩 생성 (#157)"
```

---

## Task 9: 프론트 IPC 파사드 추가

**Files:**

- Modify: `src/shared/ipc/index.ts`

- [ ] **Step 1: 실패 테스트 작성**

`src/shared/ipc/index.test.ts`에 추가(기존 테스트 패턴을 따름 — `call` 목킹):

```ts
it("forumStocks.list invokes list_forum_stocks with category/exchange/page", async () => {
  invokeMock.mockResolvedValueOnce({
    stocks: [],
    totalCount: 0,
    page: 1,
    hasNext: false,
  });
  await ipc.forumStocks.list("tradingValue", "krx", 1);
  expect(invokeMock).toHaveBeenCalledWith("list_forum_stocks", {
    category: "tradingValue",
    exchange: "krx",
    page: 1,
  });
});

it("forumStocks.search invokes search_forum_stocks with query/page", async () => {
  invokeMock.mockResolvedValueOnce({
    stocks: [],
    totalCount: 0,
    page: 1,
    hasNext: false,
  });
  await ipc.forumStocks.search("코", 2);
  expect(invokeMock).toHaveBeenCalledWith("search_forum_stocks", {
    query: "코",
    page: 2,
  });
});
```

> `invokeMock`/`ipc` 셋업은 `index.test.ts` 기존 코드와 동일하게 사용한다(파일 상단에 이미 존재).

- [ ] **Step 2: 실패 확인**

Run: `pnpm test src/shared/ipc/index.test.ts 2>&1 | tail -20`
Expected: FAIL — `ipc.forumStocks` undefined.

- [ ] **Step 3: 구현 추가**

`src/shared/ipc/index.ts` 상단 import에 바인딩 타입 추가:

```ts
import type { ForumStockPage } from "@/shared/bindings/ForumStockPage";
import type { ForumStockCategory } from "@/shared/bindings/ForumStockCategory";
import type { StockExchange } from "@/shared/bindings/StockExchange";
```

`stocks: { ... }` 블록 **바로 아래**에 새 그룹 추가:

```ts
  forumStocks: {
    /** 카테고리(토론/거래대금/인기/상승/하락/거래량) × 거래소(krx/nxt) 한 페이지. */
    list: (category: ForumStockCategory, exchange: StockExchange, page: number) =>
      call<ForumStockPage>("list_forum_stocks", { category, exchange, page }),
    /** 검색어 포함 국내 종목 한 페이지(80개 상한 없음). */
    search: (query: string, page: number) =>
      call<ForumStockPage>("search_forum_stocks", { query, page }),
  },
```

- [ ] **Step 4: 통과 확인**

Run: `pnpm test src/shared/ipc/index.test.ts 2>&1 | tail -15`
Expected: PASS.

- [ ] **Step 5: 커밋**

```bash
git add src/shared/ipc/index.ts src/shared/ipc/index.test.ts
git commit -m "feat(forum): ipc.forumStocks 파사드(list/search) 추가 (#157)"
```

---

## Task 10: 모달 재디자인 — 카테고리 탭 + 데이터 로드

**Files:**

- Modify: `src/features/posts/stock-crawl-modal.tsx` (전면 재작성)
- Modify: `src/features/posts/stock-crawl-modal.test.tsx`

> 이 Task부터 Task 13까지 같은 파일을 점진적으로 완성한다. Props 계약(`open/preselected/onClose/onConfirm`)은 **절대 변경 금지**.

- [ ] **Step 1: 실패 테스트 작성**

`stock-crawl-modal.test.tsx`를 새 동작 기준으로 작성(기존 테스트는 신규 동작으로 대체). 핵심 케이스:

```tsx
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi, beforeEach } from "vitest";

import { StockCrawlModal } from "./stock-crawl-modal";
import { renderWithProviders } from "@/test/render"; // 기존 헬퍼(있으면 사용, 없으면 render+MantineProvider)

const listMock = vi.fn();
const searchMock = vi.fn();
vi.mock("@/shared/ipc", () => ({
  ipc: {
    forumStocks: {
      list: (...a: unknown[]) => listMock(...a),
      search: (...a: unknown[]) => searchMock(...a),
    },
  },
}));

const page = (stocks: unknown[], hasNext = false) => ({
  stocks,
  totalCount: stocks.length,
  page: 1,
  hasNext,
});

beforeEach(() => {
  listMock.mockReset();
  searchMock.mockReset();
  listMock.mockResolvedValue(
    page([
      {
        code: "000660",
        name: "SK하이닉스",
        exchange: "KOSPI",
        price: "1,911,000",
        changeRate: "-7.68",
        changeType: "falling",
        isHotDiscussion: true,
      },
    ]),
  );
});

describe("StockCrawlModal", () => {
  it("기본 진입 시 거래대금 카테고리를 KRX로 로드한다", async () => {
    renderWithProviders(
      <StockCrawlModal
        open
        preselected={[]}
        onClose={() => {}}
        onConfirm={() => {}}
      />,
    );
    await waitFor(() =>
      expect(listMock).toHaveBeenCalledWith("tradingValue", "krx", 1),
    );
    expect(await screen.findByText("SK하이닉스")).toBeInTheDocument();
  });

  it("카테고리 탭(상승)을 누르면 해당 category로 재호출한다", async () => {
    renderWithProviders(
      <StockCrawlModal
        open
        preselected={[]}
        onClose={() => {}}
        onConfirm={() => {}}
      />,
    );
    await waitFor(() => expect(listMock).toHaveBeenCalled());
    fireEvent.click(screen.getByRole("button", { name: "상승" }));
    await waitFor(() =>
      expect(listMock).toHaveBeenCalledWith("rising", "krx", 1),
    );
  });
});
```

> `renderWithProviders`가 없으면 테스트 상단에 간단한 헬퍼를 정의: `MantineProvider`로 감싼 `render`. 기존 `posts.test.tsx`가 쓰는 방식을 따른다.

- [ ] **Step 2: 실패 확인**

Run: `pnpm test src/features/posts/stock-crawl-modal.test.tsx 2>&1 | tail -25`
Expected: FAIL — 새 UI 미구현.

- [ ] **Step 3: 모달 재작성 (카테고리 탭 + 로드)**

`stock-crawl-modal.tsx` 전체를 아래로 교체(이 Task에서는 탭+로드+행 렌더까지; 거래소/검색/더보기는 다음 Task에서 확장):

```tsx
import {
  Box,
  Button,
  Checkbox,
  Group,
  Modal,
  Text,
  TextInput,
} from "@mantine/core";
import { useCallback, useEffect, useRef, useState } from "react";

import type { ForumStock } from "@/shared/bindings/ForumStock";
import type { ForumStockCategory } from "@/shared/bindings/ForumStockCategory";
import type { StockExchange } from "@/shared/bindings/StockExchange";
import type { StockCandidate } from "@/shared/data/types";
import { ipc } from "@/shared/ipc";
import { Icon } from "@/shared/ui/icons";

export interface StockCrawlModalProps {
  open: boolean;
  preselected: string[];
  onClose: () => void;
  onConfirm: (stocks: StockCandidate[]) => void;
}

const CATEGORIES: { key: ForumStockCategory; label: string }[] = [
  { key: "discussion", label: "토론" },
  { key: "tradingValue", label: "거래대금" },
  { key: "popular", label: "인기 종목" },
  { key: "rising", label: "상승" },
  { key: "falling", label: "하락" },
  { key: "volume", label: "거래량" },
];

function changeColor(t: string): string {
  if (t === "rising") return "var(--mantine-color-red-6)";
  if (t === "falling") return "var(--mantine-color-blue-6)";
  return "var(--mantine-color-gray-6)";
}

function StockCrawlModalInner({
  preselected,
  onClose,
  onConfirm,
}: StockCrawlModalProps) {
  const [category, setCategory] = useState<ForumStockCategory>("tradingValue");
  const [exchange] = useState<StockExchange>("krx");
  const [rows, setRows] = useState<ForumStock[]>([]);
  const [sel, setSel] = useState<string[]>(preselected);
  const [names, setNames] = useState<Record<string, string>>({});

  // 현재 탭/거래소의 1페이지를 로드.
  useEffect(() => {
    let alive = true;
    void ipc.forumStocks.list(category, exchange, 1).then((p) => {
      if (alive) setRows(p.stocks);
    });
    return () => {
      alive = false;
    };
  }, [category, exchange]);

  const toggle = useCallback((s: ForumStock) => {
    setNames((m) => ({ ...m, [s.code]: s.name }));
    setSel((prev) =>
      prev.includes(s.code)
        ? prev.filter((x) => x !== s.code)
        : [...prev, s.code],
    );
  }, []);

  const confirm = () =>
    onConfirm(
      sel.map((code) => ({
        code,
        name: names[code] ?? rows.find((r) => r.code === code)?.name ?? "",
        link: `https://stock.naver.com/domestic/stock/${code}/discussion?chip=all`,
      })),
    );

  return (
    <Modal opened onClose={onClose} title="종목 선택" size={580} radius="lg">
      <Group gap={6} mb={12} wrap="wrap">
        {CATEGORIES.map((c) => (
          <Button
            key={c.key}
            size="xs"
            variant={category === c.key ? "filled" : "default"}
            color="forum"
            onClick={() => setCategory(c.key)}
          >
            {c.label}
          </Button>
        ))}
      </Group>

      <Box
        style={{
          border: "1px solid var(--mantine-color-gray-2)",
          borderRadius: "var(--mantine-radius-md)",
          overflow: "hidden",
          maxHeight: 360,
          overflowY: "auto",
        }}
      >
        {rows.map((s) => {
          const checked = sel.includes(s.code);
          return (
            <Group
              key={s.code}
              gap={11}
              px={13}
              py={10}
              wrap="nowrap"
              onClick={() => toggle(s)}
              style={{
                borderBottom: "1px solid var(--mantine-color-gray-2)",
                cursor: "pointer",
                background: checked
                  ? "var(--mantine-color-blue-light)"
                  : "transparent",
              }}
            >
              <Checkbox checked={checked} readOnly size="sm" />
              {s.isHotDiscussion && (
                <Icon.flame size={15} color="var(--mantine-color-orange-6)" />
              )}
              <Box style={{ flex: 1, minWidth: 0 }}>
                <Group gap={7} wrap="nowrap">
                  <Text fz={14} fw={700}>
                    {s.name}
                  </Text>
                  <Text fz={10.5} fw={700} c="dimmed" ff="monospace">
                    {s.code}
                  </Text>
                </Group>
              </Box>
              <Text fz={13} fw={600}>
                {s.price}
              </Text>
              <Text
                fz={12}
                fw={700}
                c={changeColor(s.changeType)}
                style={{ minWidth: 56, textAlign: "right" }}
              >
                {s.changeRate}
              </Text>
            </Group>
          );
        })}
      </Box>

      <Group mt={16} gap={10}>
        <Text fz={12.5} c="dimmed">
          {sel.length}개 종목 선택됨
        </Text>
        <Box style={{ flex: 1 }} />
        <Button size="sm" variant="default" onClick={onClose}>
          취소
        </Button>
        <Button
          size="sm"
          disabled={!sel.length}
          leftSection={<Icon.check size={16} />}
          onClick={confirm}
        >
          적용 ({sel.length})
        </Button>
      </Group>
    </Modal>
  );
}

export function StockCrawlModal(props: StockCrawlModalProps) {
  if (!props.open) return null;
  return <StockCrawlModalInner key="open" {...props} />;
}
```

> `Icon.flame`이 `src/shared/ui/icons.ts`에 없으면 추가한다(예: `import { IconFlame } from "@tabler/icons-react"` → 레지스트리에 `flame: IconFlame`). 등록 확인 후 사용.

- [ ] **Step 4: 통과 확인**

Run: `pnpm test src/features/posts/stock-crawl-modal.test.tsx 2>&1 | tail -20`
Expected: PASS.

- [ ] **Step 5: 커밋**

```bash
git add src/features/posts/stock-crawl-modal.tsx src/features/posts/stock-crawl-modal.test.tsx src/shared/ui/icons.ts
git commit -m "feat(forum): 종목 선택 모달 카테고리 탭 + 종목 행 렌더 (#157)"
```

---

## Task 11: 거래소 선택(KRX/NXT) 서브 모달

**Files:**

- Modify: `src/features/posts/stock-crawl-modal.tsx`
- Modify: `src/features/posts/stock-crawl-modal.test.tsx`

- [ ] **Step 1: 실패 테스트 작성**

테스트 추가:

```tsx
it("거래소 버튼 → 모달에서 NXT 선택 시 nxt로 재호출한다", async () => {
  renderWithProviders(
    <StockCrawlModal
      open
      preselected={[]}
      onClose={() => {}}
      onConfirm={() => {}}
    />,
  );
  await waitFor(() =>
    expect(listMock).toHaveBeenCalledWith("tradingValue", "krx", 1),
  );
  fireEvent.click(screen.getByRole("button", { name: /KRX/ }));
  fireEvent.click(await screen.findByRole("button", { name: "NXT" }));
  await waitFor(() =>
    expect(listMock).toHaveBeenCalledWith("tradingValue", "nxt", 1),
  );
});
```

- [ ] **Step 2: 실패 확인**

Run: `pnpm test src/features/posts/stock-crawl-modal.test.tsx -t 거래소 2>&1 | tail -20`
Expected: FAIL — 거래소 버튼/모달 없음.

- [ ] **Step 3: 구현 — exchange를 상태로 + 선택 모달**

`stock-crawl-modal.tsx` 수정:

1. `const [exchange] = useState(...)` → `const [exchange, setExchange] = useState<StockExchange>("krx");`
2. `const [exchangeOpen, setExchangeOpen] = useState(false);` 추가.
3. 카테고리 `Group` 옆(또는 위)에 거래소 버튼 추가:

```tsx
<Group justify="space-between" mb={8} wrap="nowrap">
  <Button
    size="xs"
    variant="light"
    color="gray"
    rightSection={<Icon.chevronDown size={14} />}
    onClick={() => setExchangeOpen(true)}
  >
    {exchange.toUpperCase()}
  </Button>
</Group>
```

4. 모달 본문 어딘가(최상단 권장)에 거래소 선택 서브 모달:

```tsx
<Modal
  opened={exchangeOpen}
  onClose={() => setExchangeOpen(false)}
  title="거래소 선택"
  size={300}
  centered
  overlayProps={{ backgroundOpacity: 0.55, blur: 2 }}
>
  <Group grow>
    {(["krx", "nxt"] as StockExchange[]).map((ex) => (
      <Button
        key={ex}
        variant={exchange === ex ? "filled" : "default"}
        color="forum"
        onClick={() => {
          setExchange(ex);
          setExchangeOpen(false);
        }}
      >
        {ex.toUpperCase()}
      </Button>
    ))}
  </Group>
</Modal>
```

> `Icon.chevronDown`이 레지스트리에 없으면 추가(`IconChevronDown`).

- [ ] **Step 4: 통과 확인**

Run: `pnpm test src/features/posts/stock-crawl-modal.test.tsx 2>&1 | tail -20`
Expected: PASS.

- [ ] **Step 5: 커밋**

```bash
git add src/features/posts/stock-crawl-modal.tsx src/features/posts/stock-crawl-modal.test.tsx src/shared/ui/icons.ts
git commit -m "feat(forum): 거래소 선택(KRX/NXT) 딤 모달 (#157)"
```

---

## Task 12: 검색 모드 전환 (전체 검색)

**Files:**

- Modify: `src/features/posts/stock-crawl-modal.tsx`
- Modify: `src/features/posts/stock-crawl-modal.test.tsx`

- [ ] **Step 1: 실패 테스트 작성**

```tsx
it("검색어 입력 시 search로 전환하고, 비우면 카테고리로 복귀한다", async () => {
  searchMock.mockResolvedValue(
    page([
      {
        code: "069500",
        name: "KODEX 200",
        exchange: "코스피",
        price: "",
        changeRate: "",
        changeType: "even",
        isHotDiscussion: false,
      },
    ]),
  );
  renderWithProviders(
    <StockCrawlModal
      open
      preselected={[]}
      onClose={() => {}}
      onConfirm={() => {}}
    />,
  );
  await waitFor(() => expect(listMock).toHaveBeenCalled());
  fireEvent.change(screen.getByPlaceholderText("종목명 또는 코드 검색"), {
    target: { value: "ko" },
  });
  await waitFor(() => expect(searchMock).toHaveBeenCalledWith("ko", 1));
  expect(await screen.findByText("KODEX 200")).toBeInTheDocument();
});
```

- [ ] **Step 2: 실패 확인**

Run: `pnpm test src/features/posts/stock-crawl-modal.test.tsx -t 검색어 2>&1 | tail -20`
Expected: FAIL — 검색창/전환 없음.

- [ ] **Step 3: 구현 — 검색 상태 + 디바운스 전환**

`stock-crawl-modal.tsx` 수정:

1. `const [q, setQ] = useState("");` 추가.
2. 데이터 로드 `useEffect`를 검색/카테고리 분기로 교체:

```tsx
useEffect(() => {
  let alive = true;
  const query = q.trim();
  const id = window.setTimeout(() => {
    const req = query
      ? ipc.forumStocks.search(query, 1)
      : ipc.forumStocks.list(category, exchange, 1);
    void req.then((p) => {
      if (alive) setRows(p.stocks);
    });
  }, 250);
  return () => {
    alive = false;
    window.clearTimeout(id);
  };
}, [q, category, exchange]);
```

3. 카테고리 탭 `Group` 위에 검색창 추가:

```tsx
<TextInput
  mb={10}
  value={q}
  onChange={(e) => setQ(e.currentTarget.value)}
  placeholder="종목명 또는 코드 검색"
  leftSection={<Icon.search size={16} />}
/>
```

> 검색 중(`q.trim()` 비어있지 않음)일 때는 카테고리 탭을 숨기거나 비활성화해도 좋다(선택). 최소 구현은 그대로 두어도 테스트 통과.

- [ ] **Step 4: 통과 확인**

Run: `pnpm test src/features/posts/stock-crawl-modal.test.tsx 2>&1 | tail -20`
Expected: PASS.

- [ ] **Step 5: 커밋**

```bash
git add src/features/posts/stock-crawl-modal.tsx src/features/posts/stock-crawl-modal.test.tsx
git commit -m "feat(forum): 전체 검색 모드 전환(검색어 입력 시 search) (#157)"
```

---

## Task 13: 더보기 페이지네이션 + 선택 누적

**Files:**

- Modify: `src/features/posts/stock-crawl-modal.tsx`
- Modify: `src/features/posts/stock-crawl-modal.test.tsx`

- [ ] **Step 1: 실패 테스트 작성**

```tsx
it("더보기 클릭 시 다음 페이지를 append한다", async () => {
  listMock.mockReset();
  listMock
    .mockResolvedValueOnce(
      page(
        [
          {
            code: "000660",
            name: "SK하이닉스",
            exchange: "KOSPI",
            price: "1,911,000",
            changeRate: "-7.68",
            changeType: "falling",
            isHotDiscussion: true,
          },
        ],
        true, // hasNext
      ),
    )
    .mockResolvedValueOnce(
      page(
        [
          {
            code: "005930",
            name: "삼성전자",
            exchange: "KOSPI",
            price: "295,500",
            changeRate: "-10.18",
            changeType: "falling",
            isHotDiscussion: false,
          },
        ],
        false,
      ),
    );
  renderWithProviders(
    <StockCrawlModal
      open
      preselected={[]}
      onClose={() => {}}
      onConfirm={() => {}}
    />,
  );
  expect(await screen.findByText("SK하이닉스")).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "더보기" }));
  await waitFor(() =>
    expect(listMock).toHaveBeenCalledWith("tradingValue", "krx", 2),
  );
  expect(await screen.findByText("삼성전자")).toBeInTheDocument();
  expect(screen.getByText("SK하이닉스")).toBeInTheDocument(); // 이전 페이지 유지
});
```

- [ ] **Step 2: 실패 확인**

Run: `pnpm test src/features/posts/stock-crawl-modal.test.tsx -t 더보기 2>&1 | tail -20`
Expected: FAIL — 더보기 버튼 없음.

- [ ] **Step 3: 구현 — page/hasNext 상태 + append**

`stock-crawl-modal.tsx` 수정:

1. 상태 추가: `const [hasNext, setHasNext] = useState(false); const [loading, setLoading] = useState(false);`
2. 1페이지 로드 `useEffect`에서 `setHasNext(p.hasNext)` 도 설정(검색/카테고리 공통). `setRows(p.stocks)` 는 1페이지이므로 교체.
3. 더보기 로더 추가:

```tsx
const loadMore = useCallback(() => {
  const query = q.trim();
  const next = Math.floor(rows.length / (query ? 20 : 50)) + 1;
  setLoading(true);
  const req = query
    ? ipc.forumStocks.search(query, next)
    : ipc.forumStocks.list(category, exchange, next);
  void req
    .then((p) => {
      setRows((prev) => [...prev, ...p.stocks]);
      setHasNext(p.hasNext);
    })
    .finally(() => setLoading(false));
}, [q, category, exchange, rows.length]);
```

> 더 단순/견고하게: `page` 상태(1부터)를 두고 `next = page + 1`로 관리해도 된다. 위 `rows.length` 기반은 pageSize 가정에 의존하므로, **권장**은 `const [page, setPage] = useState(1)`을 두고 1페이지 로드시 `setPage(1)`, 더보기에서 `setPage(p=>p+1)` 후 그 값으로 호출하는 방식. 구현 시 택1하되 테스트의 `page:2` 호출과 일치시킬 것.

4. 목록 `Box` 아래에 더보기 버튼:

```tsx
{
  hasNext && (
    <Button
      mt={10}
      fullWidth
      variant="subtle"
      loading={loading}
      onClick={loadMore}
    >
      더보기
    </Button>
  );
}
```

- [ ] **Step 4: 통과 확인**

Run: `pnpm test src/features/posts/stock-crawl-modal.test.tsx 2>&1 | tail -20`
Expected: PASS (전체 모달 테스트).

- [ ] **Step 5: 커밋**

```bash
git add src/features/posts/stock-crawl-modal.tsx src/features/posts/stock-crawl-modal.test.tsx
git commit -m "feat(forum): 더보기 페이지네이션(append) + 선택 누적 유지 (#157)"
```

---

## Task 14: publish-modal 선택 칩 이름 보강

**Files:**

- Modify: `src/features/posts/publish-modal.tsx`
- Modify: `src/features/posts/publish-modal.test.tsx`

- [ ] **Step 1: 실패 테스트 작성**

`publish-modal.test.tsx`에, 선택 결과로 받은(시드 `stocks` 밖) 종목 코드가 **이름으로** 칩에 표시되는지 검증하는 케이스를 추가한다. 기존 publish-modal 테스트 셋업(forum 플랫폼 선택 + StockCrawlModal onConfirm 모킹)을 따른다:

```tsx
it("선택한 종목(시드 밖)도 이름으로 칩에 표시된다", async () => {
  // ... forum 플랫폼 선택 후 종목 선택 모달에서 onConfirm으로
  //     [{ code: "0193T0", name: "KODEX SK하이닉스단일종목레버리지", link: "" }] 전달
  expect(
    await screen.findByText("KODEX SK하이닉스단일종목레버리지"),
  ).toBeInTheDocument();
});
```

> 정확한 셋업은 기존 `publish-modal.test.tsx`의 forum 흐름 테스트를 복사해 종목 코드만 시드 밖 값으로 바꾼다.

- [ ] **Step 2: 실패 확인**

Run: `pnpm test src/features/posts/publish-modal.test.tsx -t 시드 2>&1 | tail -20`
Expected: FAIL — 시드 밖 코드라 이름 대신 코드가 보임.

- [ ] **Step 3: 구현 — 선택 이름 맵 우선 사용**

`publish-modal.tsx`에서:

1. 선택된 종목의 이름을 누적 저장하는 상태 추가: `const [selStockNames, setSelStockNames] = useState<Record<string, string>>({});`
2. `StockCrawlModal`의 `onConfirm` 핸들러에서 받은 `StockCandidate[]`의 name을 맵에 병합:

```tsx
        onConfirm={(picked) => {
          setSelStockNames((m) => {
            const next = { ...m };
            for (const p of picked) next[p.code] = p.name;
            return next;
          });
          // ... 기존 선택 코드 반영 로직 유지
        }}
```

3. 칩 이름 렌더 lookup을 보강(라인 222 부근 `stocks.find(...)?.name ?? code`):

```tsx
{
  selStockNames[code] ?? stocks.find((s) => s.code === code)?.name ?? code;
}
```

(라인 837 부근, forum job 빌드 시 종목명도 동일하게 `selStockNames[code] ??` 우선 적용.)

- [ ] **Step 4: 통과 확인**

Run: `pnpm test src/features/posts/publish-modal.test.tsx 2>&1 | tail -20`
Expected: PASS.

- [ ] **Step 5: 커밋**

```bash
git add src/features/posts/publish-modal.tsx src/features/posts/publish-modal.test.tsx
git commit -m "feat(forum): 선택 종목(시드 밖) 이름 칩 표시 보강 (#157)"
```

---

## Task 15: 전체 검증 + PR

**Files:** (없음 — 검증/정리)

- [ ] **Step 1: 프론트 전체 검사**

Run: `pnpm typecheck && pnpm lint && pnpm test:coverage 2>&1 | tail -30`
Expected: typecheck/lint 통과, 커버리지 게이트(93/93/90/80) 통과. 미달 시 부족한 분기 테스트 보강.

- [ ] **Step 2: Rust 전체 검사**

Run: `cd src-tauri && cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test 2>&1 | tail -30`
Expected: 전부 통과. clippy 경고 없음.

- [ ] **Step 3: 바인딩 최신화 확인**

Run: `pnpm gen:bindings && git diff --exit-code src/shared/bindings/`
Expected: diff 없음(이미 커밋됨). 있으면 add+commit.

- [ ] **Step 4: 푸시 + PR**

```bash
git push -u origin feat/157
gh pr create --base master --head feat/157 \
  --title "feat(forum): 종목 선택 화면 네이버 모바일 재디자인" \
  --body "$(cat <<'EOF'
Closes #157

종목토론방 종목 선택 화면을 m.stock.naver.com priceTop 페이지처럼 재디자인.

- 카테고리 6탭(토론/거래대금/인기/상승/하락/거래량) + KRX/NXT 거래소 선택
- 🔥 활발 종목 표시(discussion/rankings/itemCodes)
- 전체 검색: 검색어 포함 국내 종목 전부(80개 상한 제거)
- 데이터 페치는 전부 Rust(forum_stocks 모듈), 프론트는 렌더만
- 기존 search_stocks/게시 엔진/로그인/카페/밴드/큐 불변

설계: docs/superpowers/specs/2026-06-08-forum-stock-picker-design.md
계획: docs/superpowers/plans/2026-06-08-forum-stock-picker.md
EOF
)"
```

- [ ] **Step 5: CI 확인**

Run: `gh pr checks 2>&1 | tail -20` (또는 PR 페이지). 모든 체크 green인지 확인. 실패 시 수정 후 재푸시.

---

## Self-Review (작성자 체크 결과)

- **스펙 커버리지**: 카테고리6(T3/T5/T10·T11) · 거래소(T11) · 더보기(T13) · 🔥(T4,T7,T10) · 전체검색 국내필터(T6) · 영숫자코드(T2) · Rust 페치/프론트 렌더 분리(전반) · 계약 유지(T10) · publish-modal 보강(T14) — 전부 태스크 존재. 누락 없음.
- **플레이스홀더**: 코드 스텝은 전부 실제 코드 포함. T14만 기존 테스트 셋업 복사를 지시(기존 파일 패턴 의존이라 불가피) — 실행 시 해당 파일 확인.
- **타입 일관성**: `ForumStock{code,name,exchange,price,changeRate,changeType,isHotDiscussion}` / `ForumStockPage{stocks,totalCount,page,hasNext}` / 커맨드 인자 `category,exchange,page` / `query,page` — Task 1·3·6·9·10 전반 동일. 검색은 가격 빈 값(설계 일치).
- **주의(실행자)**: ① `mod client;`는 T2 완료 전까지 컴파일 깨짐 — T1→T2 연속 진행. ② `Icon.flame`/`Icon.chevronDown` 미존재 시 레지스트리 등록 필요(T10/T11). ③ T13 페이지 카운트는 `page` 상태 방식 권장(테스트 `page:2`와 일치).
