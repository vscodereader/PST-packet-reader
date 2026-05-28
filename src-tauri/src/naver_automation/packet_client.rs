use std::collections::BTreeMap;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use reqwest::blocking::Client;
use reqwest::header::{
    HeaderMap, HeaderValue, ACCEPT, ACCEPT_LANGUAGE, CONTENT_TYPE, COOKIE, ORIGIN, REFERER,
    USER_AGENT,
};
use serde_json::{json, Value};
use url::form_urlencoded::Serializer;

use super::{AutomationError, AutomationResult, CdpClient};

const STOCK_ORIGIN: &str = "https://stock.naver.com";
const M_STOCK_ORIGIN: &str = "https://m.stock.naver.com";
const CBOX_ORIGIN: &str = "https://apis.naver.com";

pub(super) struct NaverPacketClient {
    client: Client,
    cookie_header: String,
    user_agent: String,
}

struct DiscussionTarget {
    discussion_type: String,
    item_code: String,
}

impl CdpClient {
    // Chrome DevTools에서 로그인된 네이버 쿠키를 읽어 Rust HTTP 패킷 클라이언트를 만드는 함수입니다.
    pub(super) fn build_naver_packet_client(&mut self) -> AutomationResult<NaverPacketClient> {
        self.call("Network.enable", json!({}))?;

        let result = self.call(
            "Network.getCookies",
            json!({
                "urls": [
                    "https://stock.naver.com",
                    "https://m.stock.naver.com",
                    "https://apis.naver.com",
                    "https://static.nid.naver.com"
                ]
            }),
        )?;
        let mut cookies = BTreeMap::new();

        for cookie in result
            .get("cookies")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let Some(name) = cookie.get("name").and_then(Value::as_str) else {
                continue;
            };
            let Some(value) = cookie.get("value").and_then(Value::as_str) else {
                continue;
            };
            let domain = cookie
                .get("domain")
                .and_then(Value::as_str)
                .unwrap_or_default();

            if domain.contains("naver.com") || domain.contains("pstatic.net") {
                cookies.insert(name.to_owned(), value.to_owned());
            }
        }

        if !cookies.contains_key("NID_AUT") || !cookies.contains_key("NID_SES") {
            return Err(AutomationError::new(
                "Chrome에서 네이버 로그인 쿠키를 찾지 못했습니다. 로그인 후 다시 실행하세요.",
            ));
        }

        let cookie_header = cookies
            .into_iter()
            .map(|(name, value)| format!("{name}={value}"))
            .collect::<Vec<_>>()
            .join("; ");
        let user_agent = self.evaluate_string("navigator.userAgent")?;
        let client = Client::builder()
            .timeout(Duration::from_secs(20))
            .redirect(reqwest::redirect::Policy::limited(10))
            .build()
            .map_err(|error| {
                AutomationError::new(format!("Rust HTTP 클라이언트 생성 실패: {error}"))
            })?;

        Ok(NaverPacketClient {
            client,
            cookie_header,
            user_agent,
        })
    }
}

impl NaverPacketClient {
    // Rust HTTP 클라이언트로 글쓰기 form 패킷에서 txId를 받고 add 패킷으로 글을 등록하는 함수입니다.
    pub(super) fn submit_post(
        &self,
        page_url: &str,
        title: &str,
        body: &str,
    ) -> AutomationResult<String> {
        let target = discussion_target_from_url(page_url)?;
        let tx_id = self.issue_post_tx_id(page_url, &target)?;
        let payload = build_post_payload(title, body, &target, &tx_id);
        let response_text = self
            .client
            .post(format!("{M_STOCK_ORIGIN}/front-api/discussion/add"))
            .headers(self.json_headers(page_url)?)
            .json(&payload)
            .send()
            .map_err(|error| AutomationError::new(format!("글쓰기 add 패킷 전송 실패: {error}")))?
            .error_for_status()
            .map_err(|error| AutomationError::new(format!("글쓰기 add 패킷 HTTP 실패: {error}")))?
            .text()
            .map_err(|error| AutomationError::new(format!("글쓰기 add 응답 읽기 실패: {error}")))?;
        let value = parse_json(&response_text, "글쓰기 add")?;

        if !value
            .get("isSuccess")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            return Err(AutomationError::new(format!(
                "글쓰기 add 패킷 API 실패: {}",
                packet_error_message(&value, &response_text)
            )));
        }

        Ok(value
            .pointer("/result/id")
            .and_then(Value::as_i64)
            .map(|value| value.to_string())
            .or_else(|| {
                value
                    .pointer("/result/id")
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned)
            })
            .unwrap_or_default())
    }

    // Rust HTTP 클라이언트로 cbox 토큰 발급 패킷과 댓글 생성 패킷을 차례대로 호출하는 함수입니다.
    pub(super) fn submit_comment(&self, page_url: &str, body: &str) -> AutomationResult<String> {
        let object_id = object_id_from_url(page_url)?;
        let object_url = page_url.split('#').next().unwrap_or(page_url);
        let cbox_token = self.issue_cbox_token(&object_id, object_url, page_url)?;
        let form_body = build_comment_form(&object_id, object_url, body, &cbox_token);
        let response_text = self
            .client
            .post(format!(
                "{CBOX_ORIGIN}/commentBox/cbox/web_naver_create_json.json?ticket=finance&templateId=community&pool=cbox12&_cv="
            ))
            .headers(self.form_headers(page_url)?)
            .body(form_body)
            .send()
            .map_err(|error| AutomationError::new(format!("댓글 생성 패킷 전송 실패: {error}")))?
            .error_for_status()
            .map_err(|error| AutomationError::new(format!("댓글 생성 패킷 HTTP 실패: {error}")))?
            .text()
            .map_err(|error| AutomationError::new(format!("댓글 생성 응답 읽기 실패: {error}")))?;
        let value = parse_json(&response_text, "댓글 생성")?;
        let created = value
            .get("success")
            .and_then(Value::as_bool)
            .unwrap_or(false)
            || value.pointer("/result/comment").is_some()
            || value.pointer("/result/commentList").is_some();

        if !created {
            return Err(AutomationError::new(format!(
                "댓글 생성 패킷 API 실패: {}",
                packet_error_message(&value, &response_text)
            )));
        }

        Ok(value
            .pointer("/result/comment/commentNo")
            .and_then(Value::as_i64)
            .map(|value| value.to_string())
            .or_else(|| {
                value
                    .pointer("/result/comment/commentNo")
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned)
            })
            .unwrap_or_default())
    }

    fn issue_post_tx_id(
        &self,
        page_url: &str,
        target: &DiscussionTarget,
    ) -> AutomationResult<String> {
        let form_url = format!(
            "{M_STOCK_ORIGIN}/front-api/discussion/form?discussionType={}&itemCode={}",
            target.discussion_type, target.item_code
        );
        let response_text = self
            .client
            .post(form_url)
            .headers(self.json_headers(page_url)?)
            .send()
            .map_err(|error| AutomationError::new(format!("글쓰기 form 패킷 전송 실패: {error}")))?
            .error_for_status()
            .map_err(|error| AutomationError::new(format!("글쓰기 form 패킷 HTTP 실패: {error}")))?
            .text()
            .map_err(|error| {
                AutomationError::new(format!("글쓰기 form 응답 읽기 실패: {error}"))
            })?;
        let value = parse_json(&response_text, "글쓰기 form")?;

        if !value
            .get("isSuccess")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            return Err(AutomationError::new(format!(
                "글쓰기 form 패킷 API 실패: {}",
                packet_error_message(&value, &response_text)
            )));
        }

        value
            .pointer("/result/txId")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned)
            .ok_or_else(|| AutomationError::new("글쓰기 form 응답에서 txId를 찾지 못했습니다."))
    }

    fn issue_cbox_token(
        &self,
        object_id: &str,
        object_url: &str,
        page_url: &str,
    ) -> AutomationResult<String> {
        let query = Serializer::new(String::new())
            .append_pair("ticket", "finance")
            .append_pair("templateId", "community")
            .append_pair("pool", "cbox12")
            .append_pair("_cv", "")
            .append_pair("lang", "ko")
            .append_pair("pageType", "more")
            .append_pair("country", "")
            .append_pair("objectId", object_id)
            .append_pair("categoryId", "")
            .append_pair("pageSize", "10")
            .append_pair("indexSize", "10")
            .append_pair("groupId", "")
            .append_pair("listType", "OBJECT")
            .append_pair("clientType", "web-pc")
            .append_pair("objectUrl", object_url)
            .finish();
        let response_text = self
            .client
            .get(format!(
                "{CBOX_ORIGIN}/commentBox/cbox/web_naver_token_json.json?{query}"
            ))
            .headers(self.json_headers(page_url)?)
            .send()
            .map_err(|error| AutomationError::new(format!("댓글 토큰 패킷 전송 실패: {error}")))?
            .error_for_status()
            .map_err(|error| AutomationError::new(format!("댓글 토큰 패킷 HTTP 실패: {error}")))?
            .text()
            .map_err(|error| AutomationError::new(format!("댓글 토큰 응답 읽기 실패: {error}")))?;
        let value = parse_json(&response_text, "댓글 토큰")?;

        value
            .pointer("/result/cbox_token")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned)
            .ok_or_else(|| {
                AutomationError::new(format!(
                    "댓글 토큰 응답에서 cbox_token을 찾지 못했습니다: {}",
                    packet_error_message(&value, &response_text)
                ))
            })
    }

    fn json_headers(&self, referer: &str) -> AutomationResult<HeaderMap> {
        let mut headers = self.base_headers(referer)?;
        headers.insert(
            ACCEPT,
            HeaderValue::from_static("application/json, text/plain, */*"),
        );
        Ok(headers)
    }

    fn form_headers(&self, referer: &str) -> AutomationResult<HeaderMap> {
        let mut headers = self.json_headers(referer)?;
        headers.insert(
            CONTENT_TYPE,
            HeaderValue::from_static("application/x-www-form-urlencoded; charset=UTF-8"),
        );
        Ok(headers)
    }

    fn base_headers(&self, referer: &str) -> AutomationResult<HeaderMap> {
        let mut headers = HeaderMap::new();
        headers.insert(ORIGIN, HeaderValue::from_static(STOCK_ORIGIN));
        headers.insert(REFERER, header_value(referer, "referer")?);
        headers.insert(USER_AGENT, header_value(&self.user_agent, "user-agent")?);
        headers.insert(COOKIE, header_value(&self.cookie_header, "cookie")?);
        headers.insert(
            ACCEPT_LANGUAGE,
            HeaderValue::from_static("ko-KR,ko;q=0.9,en-US;q=0.8,en;q=0.7"),
        );
        headers.insert("sec-fetch-site", HeaderValue::from_static("same-site"));
        headers.insert("sec-fetch-mode", HeaderValue::from_static("cors"));
        headers.insert("sec-fetch-dest", HeaderValue::from_static("empty"));
        Ok(headers)
    }
}

fn discussion_target_from_url(page_url: &str) -> AutomationResult<DiscussionTarget> {
    let path = url::Url::parse(page_url)
        .map_err(|error| AutomationError::new(format!("현재 URL 해석 실패: {error}")))?
        .path()
        .to_owned();
    let item_code = path
        .split("/stock/")
        .nth(1)
        .or_else(|| path.split("/index/").nth(1))
        .and_then(|value| value.split('/').next())
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .ok_or_else(|| {
            AutomationError::new(format!(
                "현재 URL에서 종목 코드를 찾지 못했습니다: {page_url}"
            ))
        })?;
    let discussion_type = if path.contains("/domestic/index/") {
        "domesticIndex"
    } else if path.contains("/domestic/stock/") {
        "domesticStock"
    } else if path.contains("/worldstock/index/") {
        "foreignIndex"
    } else if path.contains("/worldstock/stock/") {
        "foreignStock"
    } else {
        "domesticStock"
    };

    Ok(DiscussionTarget {
        discussion_type: discussion_type.to_owned(),
        item_code,
    })
}

fn object_id_from_url(page_url: &str) -> AutomationResult<String> {
    page_url
        .split("/discussion/")
        .nth(1)
        .map(|value| {
            value
                .chars()
                .take_while(|character| character.is_ascii_digit())
                .collect::<String>()
        })
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            AutomationError::new(format!(
                "현재 URL에서 댓글 objectId를 찾지 못했습니다: {page_url}"
            ))
        })
}

fn build_post_payload(title: &str, body: &str, target: &DiscussionTarget, tx_id: &str) -> Value {
    let document_id = packet_id("DOC");
    let component_id = packet_id("TEXT");
    let paragraph_id = packet_id("PARAGRAPH");
    let node_id = packet_id("NODE");
    let body_length = body.chars().count();

    json!({
        "title": title,
        "contentJson": {
            "document": {
                "version": "2.9.0",
                "theme": "default",
                "language": "ko-KR",
                "id": document_id,
                "components": [{
                    "id": component_id,
                    "layout": "default",
                    "value": [{
                        "id": paragraph_id,
                        "nodes": [{
                            "id": node_id,
                            "value": body,
                            "@ctype": "textNode"
                        }],
                        "@ctype": "paragraph"
                    }],
                    "@ctype": "text"
                }],
                "di": {
                    "dif": false,
                    "dio": [{
                        "dis": "N",
                        "dia": {
                            "t": 0,
                            "p": 0,
                            "st": body_length,
                            "sk": 0
                        }
                    }]
                }
            },
            "documentId": ""
        },
        "isCleanbotDisabled": false,
        "danglingImages": [],
        "discussionType": target.discussion_type,
        "itemCode": target.item_code,
        "txId": tx_id,
        "inflow": "NFS-P-P"
    })
}

fn build_comment_form(object_id: &str, object_url: &str, body: &str, cbox_token: &str) -> String {
    Serializer::new(String::new())
        .append_pair("lang", "ko")
        .append_pair("pageType", "more")
        .append_pair("country", "")
        .append_pair("objectId", object_id)
        .append_pair("categoryId", "")
        .append_pair("pageSize", "10")
        .append_pair("indexSize", "10")
        .append_pair("groupId", "")
        .append_pair("listType", "OBJECT")
        .append_pair("clientType", "web-pc")
        .append_pair("objectUrl", object_url)
        .append_pair("contents", body)
        .append_pair("userType", "")
        .append_pair("pick", "false")
        .append_pair("manager", "false")
        .append_pair("score", "0")
        .append_pair("likeItId", "")
        .append_pair("secret", "false")
        .append_pair("refresh", "true")
        .append_pair("imageCount", "0")
        .append_pair("commentType", "txt")
        .append_pair("validateBanWords", "true")
        .append_pair("invalidateCleanbotAlert", "false")
        .append_pair("cbox_token", cbox_token)
        .finish()
}

fn parse_json(response_text: &str, label: &str) -> AutomationResult<Value> {
    serde_json::from_str(response_text).map_err(|error| {
        AutomationError::new(format!(
            "{label} 패킷 응답 JSON 해석 실패: {error}; body={}",
            response_text.chars().take(200).collect::<String>()
        ))
    })
}

fn packet_error_message(value: &Value, fallback: &str) -> String {
    value
        .get("message")
        .and_then(Value::as_str)
        .or_else(|| value.get("detailCode").and_then(Value::as_str))
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| fallback.chars().take(200).collect())
}

fn packet_id(prefix: &str) -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    format!("SE-{prefix}-{nanos}")
}

fn header_value(value: &str, label: &str) -> AutomationResult<HeaderValue> {
    HeaderValue::from_str(value)
        .map_err(|error| AutomationError::new(format!("{label} 헤더 값 생성 실패: {error}")))
}
