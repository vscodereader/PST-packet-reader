//! 네이버 모바일(m.stock.naver.com) front-api 호출 클라이언트.
//! 테스트는 [`ForumStockClient::with_base_url`]로 wiremock 서버를 주입한다.

use std::collections::HashSet;

use serde_json::Value;

use super::{ForumStock, ForumStockCategory, ForumStockPage, StockExchange, StockMarket};

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

    /// `m.stock.naver.com` GET 후 JSON 파싱(공통). 실패는 사람이 읽을 메시지 문자열로.
    async fn get_json(&self, path_and_query: &str) -> Result<Value, String> {
        let url = format!("{}{}", self.base_url, path_and_query);
        let res = self
            .http
            .get(&url)
            .header("accept", "application/json, text/plain, */*")
            .header(
                "referer",
                "https://m.stock.naver.com/domestic/home/priceTop/total",
            )
            .header("user-agent", "Mozilla/5.0")
            .send()
            .await
            .map_err(|e| format!("종목 조회 전송 오류: {e}"))?;
        if !res.status().is_success() {
            return Err(format!("종목 조회 HTTP 오류: {}", res.status().as_u16()));
        }
        let text = res
            .text()
            .await
            .map_err(|e| format!("종목 응답 읽기 오류: {e}"))?;
        serde_json::from_str(&text).map_err(|e| format!("종목 응답 파싱 오류: {e}"))
    }

    /// 천 단위 콤마 포맷(현재가 정수 → "158,165"). 0/음수도 안전.
    fn format_price(n: i64) -> String {
        let neg = n < 0;
        let digits = n.unsigned_abs().to_string();
        let mut out = String::new();
        for (i, ch) in digits.chars().enumerate() {
            if i > 0 && (digits.len() - i).is_multiple_of(3) {
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
        market: StockMarket,
        page: u32,
    ) -> Result<ForumStockPage, String> {
        let sort = sort_type(category).ok_or("토론 카테고리는 별도 경로를 사용합니다")?;
        let q = format!(
            "/front-api/domestic/stock/list?sortType={sort}&category={}&domesticStockExchangeType={}&page={page}&pageSize={PAGE_SIZE}",
            market.as_query(),
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
                    change_type: change_type(it["fluctuationsType"].as_str().unwrap_or(""))
                        .to_string(),
                    is_hot_discussion: false,
                    code,
                });
            }
        }
        let has_next = (page * PAGE_SIZE) < total_count;
        Ok(ForumStockPage {
            stocks,
            total_count,
            page,
            has_next,
        })
    }

    /// 지금 토론 활발한 종목코드 집합(🔥). 실패해도 비치명적 → 빈 집합 반환.
    pub async fn fetch_hot_codes(&self) -> HashSet<String> {
        let value = match self
            .get_json("/front-api/discussion/rankings/itemCodes")
            .await
        {
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
        Ok(ForumStockPage {
            stocks,
            total_count,
            page,
            has_next,
        })
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
                        d["stockExchangeType"]["nameKor"]
                            .as_str()
                            .unwrap_or("")
                            .to_string(),
                        d["closePrice"].as_str().unwrap_or("").to_string(),
                        d["fluctuationsRatio"].as_str().unwrap_or("").to_string(),
                        change_type(d["compareToPreviousPrice"]["name"].as_str().unwrap_or(""))
                            .to_string(),
                    ),
                );
            }
        }
        map
    }

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
        Ok(ForumStockPage {
            stocks,
            total_count,
            page,
            has_next,
        })
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

    use wiremock::matchers::{method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    const LIST_FIXTURE: &str = include_str!("fixtures/stock_list_krx_price_top.json");

    #[test]
    fn sort_type_maps_each_category() {
        assert_eq!(
            sort_type(ForumStockCategory::TradingValue),
            Some("priceTop")
        );
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

    #[tokio::test]
    async fn fetch_category_page_parses_stocks_and_meta() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/front-api/domestic/stock/list"))
            .and(query_param("sortType", "priceTop"))
            .and(query_param("category", "all"))
            .and(query_param("domesticStockExchangeType", "KRX"))
            .and(query_param("page", "1"))
            .respond_with(ResponseTemplate::new(200).set_body_string(LIST_FIXTURE))
            .mount(&server)
            .await;

        let client = ForumStockClient::with_base_url(server.uri());
        let page = client
            .fetch_category_page(
                ForumStockCategory::TradingValue,
                StockExchange::Krx,
                StockMarket::All,
                1,
            )
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

    #[tokio::test]
    async fn fetch_category_page_sends_market_category_param() {
        // 코스피/코스닥 선택은 네이버 `category` 파라미터로 서버단에서 갈린다.
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/front-api/domestic/stock/list"))
            .and(query_param("sortType", "up"))
            .and(query_param("category", "KOSDAQ"))
            .and(query_param("domesticStockExchangeType", "NXT"))
            .respond_with(ResponseTemplate::new(200).set_body_string(LIST_FIXTURE))
            .mount(&server)
            .await;

        let client = ForumStockClient::with_base_url(server.uri());
        // category=KOSDAQ가 아니면 mock이 매치되지 않아 404 → Err로 떨어진다.
        let page = client
            .fetch_category_page(
                ForumStockCategory::Rising,
                StockExchange::Nxt,
                StockMarket::Kosdaq,
                1,
            )
            .await
            .unwrap();
        assert_eq!(page.total_count, 4396);
    }

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
}
