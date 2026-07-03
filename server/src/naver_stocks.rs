//! 종목토론방 종목 프록시(07-게시명령 2단계). 네이버 모바일(m.stock.naver.com) 공개 front-api를
//! **무쿠키**로 호출해 Admin이 실제 종목 목록을 미리보게 한다. 하위 `src-tauri/src/forum_stocks`
//! 클라이언트를 서버 크레이트로 이식(별도 크레이트라 import 불가 → 충실 포팅).
//!
//! 원칙: 서버가 종목의 **원천(코드 확정)**이다. Admin은 이 목록에서 불꽃우선 N을 골라(그 실코드로)
//! 게시 명령을 만든다. 통신로그에는 **네이버 원문 응답 전체를 자르지 않고** 남긴다(사용자 지시).

use std::collections::{HashMap, HashSet};

use serde::Serialize;
use serde_json::Value;

const HOST: &str = "https://m.stock.naver.com";
const PAGE_SIZE: u32 = 50;

// ── 프론트에서 문자열로 오는 카테고리/거래소/시장(하위 ForumStock* enum과 1:1) ──

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Category {
    Discussion,
    TradingValue,
    Popular,
    Rising,
    Falling,
    Volume,
}

impl Category {
    /// 프론트 camelCase 문자열 → enum. 알 수 없으면 None(400).
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "discussion" => Category::Discussion,
            "tradingValue" => Category::TradingValue,
            "popular" => Category::Popular,
            "rising" => Category::Rising,
            "falling" => Category::Falling,
            "volume" => Category::Volume,
            _ => return None,
        })
    }

    /// `/front-api/domestic/stock/list` 의 sortType. 토론은 별도 경로라 None.
    fn sort_type(self) -> Option<&'static str> {
        match self {
            Category::TradingValue => Some("priceTop"),
            Category::Volume => Some("quantTop"),
            Category::Rising => Some("up"),
            Category::Falling => Some("down"),
            Category::Popular => Some("searchTop"),
            Category::Discussion => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Exchange {
    Krx,
    Nxt,
}
impl Exchange {
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "krx" => Exchange::Krx,
            "nxt" => Exchange::Nxt,
            _ => return None,
        })
    }
    fn as_query(self) -> &'static str {
        match self {
            Exchange::Krx => "KRX",
            Exchange::Nxt => "NXT",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Market {
    All,
    Kospi,
    Kosdaq,
}
impl Market {
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "all" => Market::All,
            "kospi" => Market::Kospi,
            "kosdaq" => Market::Kosdaq,
            _ => return None,
        })
    }
    fn as_query(self) -> &'static str {
        match self {
            Market::All => "all",
            Market::Kospi => "KOSPI",
            Market::Kosdaq => "KOSDAQ",
        }
    }
}

/// 종목 한 줄(Admin이 그대로 렌더). 하위 ForumStock과 동일 필드(camelCase).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ForumStock {
    pub code: String,
    pub name: String,
    pub exchange: String,
    pub price: String,
    pub change_rate: String,
    pub change_type: String,
    pub is_hot_discussion: bool,
}

/// 한 페이지 결과 + 페이지네이션 메타(하위 ForumStockPage와 동일).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ForumStockPage {
    pub stocks: Vec<ForumStock>,
    pub total_count: u32,
    pub page: u32,
    pub has_next: bool,
}

/// 네이버 원문 호출 1건(통신로그에 **원문 전체**를 남기기 위한 기록).
pub struct RawCall {
    pub url: String,
    pub status: u16,
    pub body: String,
}

// ── 순수 헬퍼(하위 client.rs와 동일 규칙) ──

/// fluctuationsType / compareToPreviousPrice.name → 색상 키.
fn change_type(raw: &str) -> &'static str {
    match raw {
        "RISING" => "rising",
        "FALLING" => "falling",
        _ => "even",
    }
}

/// 6자리 영숫자 종목 코드만 허용(ETF 특수코드 `0193T0` 포함).
fn looks_like_code(value: &str) -> bool {
    value.chars().count() == 6 && value.chars().all(|c| c.is_ascii_alphanumeric())
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

/// 이름에 "ETN"(대소문자 무시) 또는 "레버리지"가 들어간 종목인지(#267-7 선택 목록 숨김).
fn is_hidden_stock(stock: &ForumStock) -> bool {
    stock.name.to_uppercase().contains("ETN") || stock.name.contains("레버리지")
}

/// ETN·레버리지 종목을 페이지에서 제거(#267-7). total_count는 네이버 원본 집계라 그대로 둔다.
fn drop_hidden_stocks(page: &mut ForumStockPage) {
    page.stocks.retain(|s| !is_hidden_stock(s));
}

/// 활발 종목 집합에 든 코드만 🔥 표시.
fn merge_hot(stocks: &mut [ForumStock], hot: &HashSet<String>) {
    for s in stocks.iter_mut() {
        if hot.contains(&s.code) {
            s.is_hot_discussion = true;
        }
    }
}

pub struct NaverStockClient {
    base_url: String,
    http: reqwest::Client,
}

impl NaverStockClient {
    pub fn new() -> Self {
        Self::with_base_url(HOST)
    }

    /// 테스트 주입용(wiremock 서버 URL). 하위 forum_stocks `with_base_url`과 동일 역할.
    pub fn with_base_url(base_url: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into(),
            http: reqwest::Client::new(),
        }
    }

    /// GET 후 (파싱된 JSON) 반환 + **원문 응답 전체**를 `raws`에 누적(통신로그용). 실패는 사람이 읽을 문자열.
    async fn get_json(&self, path_and_query: &str, raws: &mut Vec<RawCall>) -> Result<Value, String> {
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
        let status = res.status().as_u16();
        let text = res
            .text()
            .await
            .map_err(|e| format!("종목 응답 읽기 오류: {e}"))?;
        // 원문은 성공/실패 관계없이 그대로 로그로 남긴다(자르지 않음).
        raws.push(RawCall {
            url: url.clone(),
            status,
            body: text.clone(),
        });
        if !(200..300).contains(&status) {
            return Err(format!("종목 조회 HTTP 오류: {status}"));
        }
        serde_json::from_str(&text).map_err(|e| format!("종목 응답 파싱 오류: {e}"))
    }

    /// 카테고리 목록 한 페이지. 토론은 🔥 자체, 그 외는 itemCodes 병합. #267-7 숨김 적용.
    /// 반환: (성공/실패 Result, 네이버 원문 호출 전체). **실패해도 그때까지의 원문을 반드시 함께
    /// 돌려준다** — 통신로그에 성공·실패 모두 원문 전체를 남기기 위함(사용자 지시).
    pub async fn list(
        &self,
        category: Category,
        exchange: Exchange,
        market: Market,
        page: u32,
    ) -> (Result<ForumStockPage, String>, Vec<RawCall>) {
        let mut raws = Vec::new();
        let result = self
            .list_inner(category, exchange, market, page, &mut raws)
            .await;
        (result, raws)
    }

    async fn list_inner(
        &self,
        category: Category,
        exchange: Exchange,
        market: Market,
        page: u32,
        raws: &mut Vec<RawCall>,
    ) -> Result<ForumStockPage, String> {
        let mut result = match category {
            // 토론은 시장 분리가 없어 market 무시(네이버 API 한계).
            Category::Discussion => self.fetch_discussion_page(exchange, page, raws).await?,
            _ => {
                let mut result = self
                    .fetch_category_page(category, exchange, market, page, raws)
                    .await?;
                let hot = self.fetch_hot_codes(raws).await;
                merge_hot(&mut result.stocks, &hot);
                result
            }
        };
        drop_hidden_stocks(&mut result);
        Ok(result)
    }

    async fn fetch_category_page(
        &self,
        category: Category,
        exchange: Exchange,
        market: Market,
        page: u32,
        raws: &mut Vec<RawCall>,
    ) -> Result<ForumStockPage, String> {
        let sort = category
            .sort_type()
            .ok_or("토론 카테고리는 별도 경로를 사용합니다")?;
        let q = format!(
            "/front-api/domestic/stock/list?sortType={sort}&category={}&domesticStockExchangeType={}&page={page}&pageSize={PAGE_SIZE}",
            market.as_query(),
            exchange.as_query()
        );
        let value = self.get_json(&q, raws).await?;
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
                    Some(n) => format_price(n),
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

    /// 지금 토론 활발한 종목코드 집합(🔥). 실패해도 비치명적 → 빈 집합.
    async fn fetch_hot_codes(&self, raws: &mut Vec<RawCall>) -> HashSet<String> {
        let value = match self
            .get_json("/front-api/discussion/rankings/itemCodes", raws)
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
    async fn fetch_discussion_page(
        &self,
        exchange: Exchange,
        page: u32,
        raws: &mut Vec<RawCall>,
    ) -> Result<ForumStockPage, String> {
        let q = format!(
            "/front-api/discussion/ranking/list/price?nationType=KOR&size={PAGE_SIZE}&stockExchangeType={}&page={page}",
            exchange.as_query()
        );
        let value = self.get_json(&q, raws).await?;
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

        let meta = self.fetch_meta(&codes, raws).await;
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

    /// 여러 종목코드의 이름/거래소/현재가/등락 배치 조회. 실패 시 빈 맵.
    /// 반환: code → (name, exchangeKor, price, change_rate, change_type)
    async fn fetch_meta(
        &self,
        codes: &[String],
        raws: &mut Vec<RawCall>,
    ) -> HashMap<String, (String, String, String, String, String)> {
        if codes.is_empty() {
            return HashMap::new();
        }
        let joined = codes.join(",");
        let q = format!(
            "/front-api/realTime/marketPrice?itemCodes={joined}&endType=stock&stockType=domestic"
        );
        let value = match self.get_json(&q, raws).await {
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
}

impl Default for NaverStockClient {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn category_parses_camel_case() {
        assert_eq!(Category::parse("tradingValue"), Some(Category::TradingValue));
        assert_eq!(Category::parse("discussion"), Some(Category::Discussion));
        assert_eq!(Category::parse("nope"), None);
    }

    #[test]
    fn exchange_market_parse_and_query() {
        assert_eq!(Exchange::parse("krx"), Some(Exchange::Krx));
        assert_eq!(Exchange::parse("nxt").unwrap().as_query(), "NXT");
        assert_eq!(Market::parse("kospi").unwrap().as_query(), "KOSPI");
        assert_eq!(Market::parse("all").unwrap().as_query(), "all");
        assert_eq!(Market::parse("bad"), None);
    }

    #[test]
    fn sort_type_maps_each_category() {
        assert_eq!(Category::TradingValue.sort_type(), Some("priceTop"));
        assert_eq!(Category::Volume.sort_type(), Some("quantTop"));
        assert_eq!(Category::Rising.sort_type(), Some("up"));
        assert_eq!(Category::Falling.sort_type(), Some("down"));
        assert_eq!(Category::Popular.sort_type(), Some("searchTop"));
        assert_eq!(Category::Discussion.sort_type(), None);
    }

    #[test]
    fn looks_like_code_allows_alphanumeric_six() {
        assert!(looks_like_code("005930"));
        assert!(looks_like_code("0193T0"));
        assert!(!looks_like_code("00593"));
        assert!(!looks_like_code("0059300"));
    }

    #[test]
    fn format_price_inserts_commas() {
        assert_eq!(format_price(158165), "158,165");
        assert_eq!(format_price(0), "0");
        assert_eq!(format_price(-1234), "-1,234");
    }

    #[test]
    fn hides_etn_and_leverage() {
        let s = |name: &str| ForumStock {
            code: "000000".into(),
            name: name.into(),
            exchange: String::new(),
            price: String::new(),
            change_rate: String::new(),
            change_type: "even".into(),
            is_hot_discussion: false,
        };
        assert!(is_hidden_stock(&s("KODEX 레버리지")));
        assert!(is_hidden_stock(&s("TRUE 코스피 etn")));
        assert!(!is_hidden_stock(&s("삼성전자")));

        let mut page = ForumStockPage {
            stocks: vec![s("삼성전자"), s("KODEX 레버리지"), s("SK하이닉스")],
            total_count: 3,
            page: 1,
            has_next: false,
        };
        drop_hidden_stocks(&mut page);
        assert_eq!(page.stocks.len(), 2);
    }

    #[test]
    fn merge_hot_marks_only_listed() {
        let mut stocks = vec![
            ForumStock {
                code: "000660".into(),
                name: "SK하이닉스".into(),
                exchange: String::new(),
                price: String::new(),
                change_rate: String::new(),
                change_type: "even".into(),
                is_hot_discussion: false,
            },
            ForumStock {
                code: "005930".into(),
                name: "삼성전자".into(),
                exchange: String::new(),
                price: String::new(),
                change_rate: String::new(),
                change_type: "even".into(),
                is_hot_discussion: false,
            },
        ];
        let hot: HashSet<String> = ["000660".to_string()].into_iter().collect();
        merge_hot(&mut stocks, &hot);
        assert!(stocks[0].is_hot_discussion);
        assert!(!stocks[1].is_hot_discussion);
    }

    // ------------------------------------------------------------------
    // HTTP 계약(wiremock 주입) — 하위 forum_stocks 통합 테스트와 동일 수준.
    // ------------------------------------------------------------------

    use wiremock::matchers::{method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn list_category_merges_hot_and_hides_leverage_and_captures_raw() {
        let server = MockServer::start().await;
        // 카테고리 목록: 삼성전자(비🔥), SK하이닉스(🔥 병합 대상), KODEX 레버리지(#267-7 숨김).
        let list_body = r#"{"result":{"totalCount":4396,"stocks":[
            {"itemCode":"005930","name":"삼성전자","stockExchangeType":"KOSPI","currentPrice":71000,"fluctuationsRatio":"0.50","fluctuationsType":"RISING"},
            {"itemCode":"000660","name":"SK하이닉스","stockExchangeType":"KOSPI","currentPrice":191100,"fluctuationsRatio":"-7.68","fluctuationsType":"FALLING"},
            {"itemCode":"122630","name":"KODEX 레버리지","stockExchangeType":"KOSPI","currentPrice":158165,"fluctuationsRatio":"-16.68","fluctuationsType":"FALLING"}
        ]}}"#;
        Mock::given(method("GET"))
            .and(path("/front-api/domestic/stock/list"))
            .and(query_param("sortType", "priceTop"))
            .and(query_param("category", "all"))
            .and(query_param("domesticStockExchangeType", "KRX"))
            .respond_with(ResponseTemplate::new(200).set_body_string(list_body))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/front-api/discussion/rankings/itemCodes"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_string(r#"{"result":{"itemCodes":["000660"]}}"#),
            )
            .mount(&server)
            .await;

        let client = NaverStockClient::with_base_url(server.uri());
        let (result, raws) = client
            .list(Category::TradingValue, Exchange::Krx, Market::All, 1)
            .await;
        let page = result.unwrap();

        // 레버리지 숨김 → 2종목만.
        let codes: Vec<&str> = page.stocks.iter().map(|s| s.code.as_str()).collect();
        assert_eq!(codes, vec!["005930", "000660"]);
        // total_count는 네이버 원본 그대로.
        assert_eq!(page.total_count, 4396);
        // 🔥 병합: SK하이닉스만 표시.
        let sk = page.stocks.iter().find(|s| s.code == "000660").unwrap();
        assert!(sk.is_hot_discussion);
        assert!(!page.stocks.iter().find(|s| s.code == "005930").unwrap().is_hot_discussion);
        // 가격 포맷 확인.
        assert_eq!(sk.price, "191,100");
        // ★ 원문 캡처: 목록 + 🔥 두 호출의 raw body가 통째로 담겨야 한다(통신로그용, 자르지 않음).
        assert_eq!(raws.len(), 2, "list + hot 두 호출 원문이 모두 캡처돼야");
        assert!(raws.iter().any(|r| r.body.contains("KODEX 레버리지")));
        assert!(raws.iter().all(|r| r.status == 200));
    }

    #[tokio::test]
    async fn list_discussion_orders_by_ranking_with_meta() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/front-api/discussion/ranking/list/price"))
            .and(query_param("stockExchangeType", "KRX"))
            .respond_with(ResponseTemplate::new(200).set_body_string(
                r#"{"result":{"totalCount":100,"hasNextPage":true,"itemCodes":["018260","000660"]}}"#,
            ))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/front-api/realTime/marketPrice"))
            .respond_with(ResponseTemplate::new(200).set_body_string(
                r#"{"result":{"datas":[
                   {"itemCode":"000660","stockName":"SK하이닉스","stockExchangeType":{"nameKor":"코스피"},"closePrice":"1,911,000","compareToPreviousPrice":{"name":"FALLING"},"fluctuationsRatio":"-7.68"},
                   {"itemCode":"018260","stockName":"삼성에스디에스","stockExchangeType":{"nameKor":"코스피"},"closePrice":"180,000","compareToPreviousPrice":{"name":"RISING"},"fluctuationsRatio":"1.10"}
                ]}}"#,
            ))
            .mount(&server)
            .await;

        let client = NaverStockClient::with_base_url(server.uri());
        let (result, raws) = client
            .list(Category::Discussion, Exchange::Krx, Market::All, 1)
            .await;
        let page = result.unwrap();

        // 순위 순서 보존: 018260 먼저.
        assert_eq!(page.stocks[0].code, "018260");
        assert_eq!(page.stocks[0].name, "삼성에스디에스");
        assert_eq!(page.stocks[0].change_type, "rising");
        // 토론 탭은 전부 🔥.
        assert!(page.stocks.iter().all(|s| s.is_hot_discussion));
        assert!(page.has_next);
        // 랭킹 + 메타 두 호출 원문 캡처.
        assert_eq!(raws.len(), 2);
    }

    #[tokio::test]
    async fn list_propagates_upstream_http_error_but_still_captures_raw() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/front-api/domestic/stock/list"))
            .respond_with(ResponseTemplate::new(503).set_body_string("upstream down"))
            .mount(&server)
            .await;

        let client = NaverStockClient::with_base_url(server.uri());
        let (result, raws) = client
            .list(Category::TradingValue, Exchange::Krx, Market::All, 1)
            .await;
        let err = result.unwrap_err();
        assert!(err.contains("503"), "상류 HTTP 오류를 사람이 읽을 문자열로");
        // ★ 실패해도 원문은 캡처돼야 한다(통신로그에 실패 원문 전부 남김).
        assert_eq!(raws.len(), 1);
        assert_eq!(raws[0].status, 503);
        assert_eq!(raws[0].body, "upstream down");
    }
}
