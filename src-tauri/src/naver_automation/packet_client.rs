use std::collections::BTreeMap;
use std::process;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use reqwest::blocking::Client;
use reqwest::header::{
    HeaderMap, HeaderValue, ACCEPT, ACCEPT_LANGUAGE, CONTENT_TYPE, COOKIE, ORIGIN, REFERER,
    USER_AGENT,
};
use serde_json::{json, Value};
use url::form_urlencoded::Serializer;

use super::types::{DiscussionSelection, NaverLoginProfile};
use super::{AutomationError, AutomationResult, CdpClient};

const STOCK_ORIGIN: &str = "https://stock.naver.com";
const M_STOCK_ORIGIN: &str = "https://m.stock.naver.com";
const CBOX_ORIGIN: &str = "https://apis.naver.com";
const STATIC_NID_ORIGIN: &str = "https://static.nid.naver.com";
const DEFAULT_REFERER: &str = "https://stock.naver.com/discussion";
const DEFAULT_PROFILE_INTRODUCTION: &str = "2222";

pub(super) struct NaverPacketClient {
    client: Client,
    cookie_header: String,
    user_agent: String,
}

struct DiscussionTarget {
    discussion_type: String,
    item_code: String,
}

pub(super) struct PacketDiscussionRoom {
    pub selection: DiscussionSelection,
    pub discussion_url: String,
}

pub(super) struct PacketDiscussionPost {
    pub post_url: String,
}

struct StockCandidate {
    item_code: String,
    item_name: String,
    rank: String,
}

struct PostCandidate {
    post_id: String,
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
    // Wireshark에서 확인한 static.nid.naver.com getProfile 패킷을 Rust HTTP 요청으로 재현하는 함수입니다.
    pub(super) fn read_login_profile(&self) -> AutomationResult<NaverLoginProfile> {
        let callback = format!("pstmacroProfile_{}", timestamp_nanos());
        let url = format!("{STATIC_NID_ORIGIN}/getProfile?svc=my&callback={callback}");
        let response_text = self
            .client
            .get(url)
            .headers(self.static_headers(DEFAULT_REFERER)?)
            .send()
            .map_err(|error| AutomationError::new(format!("getProfile 패킷 전송 실패: {error}")))
            .and_then(|response| response_text(response, "getProfile"))?;
        let json_text = strip_jsonp(&response_text)?;
        let value = parse_json(json_text, "getProfile")?;

        Ok(NaverLoginProfile {
            logged_in: value
                .get("rtn_cd")
                .and_then(Value::as_str)
                .map(|code| code == "0")
                .unwrap_or(false),
            nickname: value
                .get("nick_name")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned),
            image_url: value
                .get("image_url")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned),
            message: value
                .get("rtn_msg")
                .or_else(|| value.get("rtn_cd"))
                .and_then(Value::as_str)
                .unwrap_or("unknown")
                .to_owned(),
        })
    }

    // Wireshark에서 확인한 랭킹/시세 API를 호출해 랜덤 종목 토론방을 선택하는 함수입니다.
    pub(super) fn select_random_discussion_room(&self) -> AutomationResult<PacketDiscussionRoom> {
        let categories = [
            (
                "토론급상승",
                "/api/community/discussion/rankings?nationType=KOR&page=1&size=10&postType=HOT",
            ),
            (
                "상승",
                "/api/domestic/market/stock/default?tradeType=KRX&marketType=ALL&orderType=up&startIdx=0&pageSize=10",
            ),
            (
                "하락",
                "/api/domestic/market/stock/default?tradeType=KRX&marketType=ALL&orderType=down&startIdx=0&pageSize=10",
            ),
            (
                "거래량",
                "/api/domestic/market/stock/default?tradeType=KRX&marketType=ALL&orderType=quantTop&startIdx=0&pageSize=10",
            ),
        ];
        let seed = selection_seed();
        let start = pseudo_index_with_seed(categories.len(), seed, 0xCA7E);

        for offset in 0..categories.len() {
            let (category, path) = categories[(start + offset) % categories.len()];
            let value = self.get_stock_json(path, DEFAULT_REFERER, category)?;
            let candidates = collect_stock_candidates(&value);

            if candidates.is_empty() {
                continue;
            }

            let picked =
                &candidates[pseudo_index_with_seed(candidates.len(), seed, 0x51 + offset as u128)];
            let discussion_url = discussion_url_for("domesticStock", &picked.item_code, None);

            return Ok(PacketDiscussionRoom {
                selection: DiscussionSelection {
                    category: category.to_owned(),
                    rank: picked.rank.clone(),
                    item_text: format!("{} ({})", picked.item_name, picked.item_code),
                    method: "packet-api".to_owned(),
                },
                discussion_url,
            });
        }

        Err(AutomationError::new(
            "패킷 API 응답에서 선택 가능한 랜덤 종목을 찾지 못했습니다.",
        ))
    }

    // Wireshark에서 확인한 posts/by-item API를 호출해 랜덤 토론글 URL을 선택하는 함수입니다.
    pub(super) fn select_random_discussion_post(
        &self,
        page_url: &str,
    ) -> AutomationResult<PacketDiscussionPost> {
        let target = discussion_target_from_url(page_url)?;
        let attempts = [
            format!(
                "/api/community/discussion/posts/by-item?discussionType={}&itemCode={}&isHolderOnly=false&excludesItemNews=false&isItemNewsOnly=false&isCleanbotPassedOnly=true&pageSize=10",
                target.discussion_type, target.item_code
            ),
            format!(
                "/api/community/discussion/posts/by-item?discussionType={}&itemCode={}&isHolderOnly=false&excludesItemNews=false&isItemNewsOnly=false&isCleanbotPassedOnly=false&pageSize=30",
                target.discussion_type, target.item_code
            ),
        ];

        for path in attempts {
            let value = self.get_stock_json(&path, page_url, "토론글 목록")?;
            let posts = collect_post_candidates(&value);

            if posts.is_empty() {
                continue;
            }

            let picked = &posts[pseudo_index_with_seed(posts.len(), selection_seed(), 0xB057)];
            let post_url = discussion_url_for(
                &target.discussion_type,
                &target.item_code,
                Some(&picked.post_id),
            );

            return Ok(PacketDiscussionPost { post_url });
        }

        Err(AutomationError::new(
            "패킷 API 응답에서 선택 가능한 랜덤 토론글을 찾지 못했습니다.",
        ))
    }

    // Wireshark 성공 캡처에서 확인한 status/form/validate/PUT 패킷으로 프로필 소개를 설정하는 함수입니다.
    pub(super) fn ensure_profile_intro_setup(&self, referer: &str) -> AutomationResult<bool> {
        let status = self.get_stock_json(
            "/api/community/profile/users/status",
            referer,
            "프로필 상태",
        )?;
        let status_text = status
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or_default();

        if status_text == "existent" {
            return Ok(false);
        }

        let profile_id = status
            .get("profileId")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned)
            .ok_or_else(|| {
                AutomationError::new("프로필 상태 응답에서 profileId를 찾지 못했습니다.")
            })?;
        let form =
            self.get_stock_json("/api/community/profile/users/form", referer, "프로필 form")?;
        let nickname = form
            .get("nickname")
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .map(ToOwned::to_owned)
            .map(Ok)
            .unwrap_or_else(|| self.recommend_profile_nickname(referer))?;

        self.validate_profile_introduction(referer)?;

        let payload = json!({
            "nickname": nickname,
            "introduction": DEFAULT_PROFILE_INTRODUCTION,
            "imageUrl": form.get("imageUrl").cloned().unwrap_or(Value::Null),
            "danglingImages": [],
        });
        let response_text = self
            .client
            .put(format!(
                "{STOCK_ORIGIN}/api/community/profile/users/{profile_id}"
            ))
            .headers(self.stock_json_headers(referer)?)
            .json(&payload)
            .send()
            .map_err(|error| {
                AutomationError::new(format!("프로필 저장 PUT 패킷 전송 실패: {error}"))
            })
            .and_then(|response| response_text(response, "프로필 저장 PUT"))?;

        if !response_text.trim().is_empty() {
            let _ = parse_json(&response_text, "프로필 저장 PUT");
        }

        let updated = self.get_stock_json(
            "/api/community/profile/users/status",
            referer,
            "프로필 상태 재확인",
        )?;
        let updated_status = updated
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or_default();

        if updated_status != "existent" {
            return Err(AutomationError::new(format!(
                "프로필 저장 후 상태가 existent가 아닙니다: {updated_status}"
            )));
        }

        Ok(true)
    }

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

    // 글쓰기 add 응답의 post_id를 현재 토론방 기준 토론글 URL로 바꾸는 함수입니다.
    pub(super) fn post_url_from_id(
        &self,
        page_url: &str,
        post_id: &str,
    ) -> AutomationResult<String> {
        if post_id.trim().is_empty() {
            return Err(AutomationError::new(
                "글쓰기 add 응답에서 작성 글 ID를 찾지 못했습니다.",
            ));
        }

        let target = discussion_target_from_url(page_url)?;

        Ok(discussion_url_for(
            &target.discussion_type,
            &target.item_code,
            Some(post_id),
        ))
    }

    // stock.naver.com JSON API를 공통 헤더로 호출하고 JSON으로 파싱하는 함수입니다.
    fn get_stock_json(&self, path: &str, referer: &str, label: &str) -> AutomationResult<Value> {
        let response_text = self
            .client
            .get(format!("{STOCK_ORIGIN}{path}"))
            .headers(self.stock_json_headers(referer)?)
            .send()
            .map_err(|error| AutomationError::new(format!("{label} GET 패킷 전송 실패: {error}")))
            .and_then(|response| response_text(response, label))?;

        parse_json(&response_text, label)
    }

    // 프로필 form에 nickname이 없을 때 네이버 추천 닉네임 패킷을 호출하는 함수입니다.
    fn recommend_profile_nickname(&self, referer: &str) -> AutomationResult<String> {
        let response_text = self
            .client
            .post(format!(
                "{STOCK_ORIGIN}/api/community/profile/users/nickname/recommend"
            ))
            .headers(self.stock_json_headers(referer)?)
            .json(&json!({ "unusedNickname": "" }))
            .send()
            .map_err(|error| AutomationError::new(format!("닉네임 추천 패킷 전송 실패: {error}")))
            .and_then(|response| response_text(response, "닉네임 추천"))?;
        let value = parse_json(&response_text, "닉네임 추천")?;

        value
            .get("recommendedNickname")
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .map(ToOwned::to_owned)
            .ok_or_else(|| {
                AutomationError::new("닉네임 추천 응답에서 recommendedNickname을 찾지 못했습니다.")
            })
    }

    // 프로필 소개 2222가 저장 가능한 값인지 검증 패킷으로 확인하는 함수입니다.
    fn validate_profile_introduction(&self, referer: &str) -> AutomationResult<()> {
        let response_text = self
            .client
            .post(format!(
                "{STOCK_ORIGIN}/api/community/profile/users/introduction/validate"
            ))
            .headers(self.stock_json_headers(referer)?)
            .json(&json!({ "targetValue": DEFAULT_PROFILE_INTRODUCTION }))
            .send()
            .map_err(|error| {
                AutomationError::new(format!("프로필 소개 검증 패킷 전송 실패: {error}"))
            })
            .and_then(|response| response_text(response, "프로필 소개 검증"))?;
        let value = parse_json(&response_text, "프로필 소개 검증")?;

        if value
            .get("isValid")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            return Ok(());
        }

        Err(AutomationError::new(format!(
            "프로필 소개 2222 검증 실패: {}",
            packet_error_message(&value, &response_text)
        )))
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

    // 글쓰기 add 패킷에 필요한 txId를 form 패킷으로 발급받는 함수입니다.
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

    // 댓글 생성에 필요한 cbox_token을 토큰 발급 패킷으로 가져오는 함수입니다.
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

    // m.stock.naver.com JSON 요청에 사용하는 공통 헤더를 만드는 함수입니다.
    fn json_headers(&self, referer: &str) -> AutomationResult<HeaderMap> {
        let mut headers = self.base_headers(referer, "same-site")?;
        headers.insert(
            ACCEPT,
            HeaderValue::from_static("application/json, text/plain, */*"),
        );
        Ok(headers)
    }

    // stock.naver.com JSON API 요청에 사용하는 공통 헤더를 만드는 함수입니다.
    fn stock_json_headers(&self, referer: &str) -> AutomationResult<HeaderMap> {
        let mut headers = self.base_headers(referer, "same-origin")?;
        headers.insert(ACCEPT, HeaderValue::from_static("*/*"));
        Ok(headers)
    }

    // static.nid.naver.com getProfile 요청에 사용하는 공통 헤더를 만드는 함수입니다.
    fn static_headers(&self, referer: &str) -> AutomationResult<HeaderMap> {
        let mut headers = self.base_headers(referer, "same-site")?;
        headers.insert(ACCEPT, HeaderValue::from_static("*/*"));
        Ok(headers)
    }

    // apis.naver.com 댓글 form-urlencoded 요청에 사용하는 공통 헤더를 만드는 함수입니다.
    fn form_headers(&self, referer: &str) -> AutomationResult<HeaderMap> {
        let mut headers = self.json_headers(referer)?;
        headers.insert(
            CONTENT_TYPE,
            HeaderValue::from_static("application/x-www-form-urlencoded; charset=UTF-8"),
        );
        Ok(headers)
    }

    // User-Agent, Cookie, Referer 등 패킷 재현에 공통으로 필요한 헤더를 조립하는 함수입니다.
    fn base_headers(
        &self,
        referer: &str,
        sec_fetch_site: &'static str,
    ) -> AutomationResult<HeaderMap> {
        let mut headers = HeaderMap::new();
        headers.insert(ORIGIN, HeaderValue::from_static(STOCK_ORIGIN));
        headers.insert(REFERER, header_value(referer, "referer")?);
        headers.insert(USER_AGENT, header_value(&self.user_agent, "user-agent")?);
        headers.insert(COOKIE, header_value(&self.cookie_header, "cookie")?);
        headers.insert(
            ACCEPT_LANGUAGE,
            HeaderValue::from_static("ko-KR,ko;q=0.9,en-US;q=0.8,en;q=0.7"),
        );
        headers.insert("sec-fetch-site", HeaderValue::from_static(sec_fetch_site));
        headers.insert("sec-fetch-mode", HeaderValue::from_static("cors"));
        headers.insert("sec-fetch-dest", HeaderValue::from_static("empty"));
        Ok(headers)
    }
}

// 현재 토론방 URL에서 네이버 discussionType과 itemCode를 계산하는 함수입니다.
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

// 현재 토론글 URL에서 댓글 API에 필요한 objectId를 추출하는 함수입니다.
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

// 랭킹/시세 API 응답 전체에서 종목 후보를 모으는 함수입니다.
fn collect_stock_candidates(value: &Value) -> Vec<StockCandidate> {
    let mut candidates = Vec::new();
    collect_stock_candidates_from_value(value, &mut candidates);
    dedupe_stock_candidates(candidates)
}

// 중첩 JSON을 재귀적으로 순회하면서 종목 코드와 종목명을 찾는 함수입니다.
fn collect_stock_candidates_from_value(value: &Value, candidates: &mut Vec<StockCandidate>) {
    match value {
        Value::Array(items) => {
            for item in items {
                collect_stock_candidates_from_value(item, candidates);
            }
        }
        Value::Object(object) => {
            if let Some(item_code) = direct_string(
                object,
                &[
                    "itemCode",
                    "stockCode",
                    "code",
                    "symbolCode",
                    "reutersCode",
                    "localCode",
                ],
            ) {
                if looks_like_stock_code(&item_code) {
                    let item_name = direct_string(
                        object,
                        &[
                            "itemName",
                            "stockName",
                            "name",
                            "korName",
                            "stockNameKr",
                            "displayName",
                        ],
                    )
                    .unwrap_or_else(|| item_code.clone());
                    let rank = direct_number(object, &["rank", "ranking", "rankNo", "no"])
                        .map(|rank| rank.to_string())
                        .unwrap_or_else(|| (candidates.len() + 1).to_string());

                    candidates.push(StockCandidate {
                        item_code,
                        item_name,
                        rank,
                    });
                }
            }

            for child in object.values() {
                collect_stock_candidates_from_value(child, candidates);
            }
        }
        _ => {}
    }
}

// 토론글 목록 API 응답 전체에서 게시글 후보를 모으는 함수입니다.
fn collect_post_candidates(value: &Value) -> Vec<PostCandidate> {
    let mut candidates = Vec::new();
    collect_post_candidates_from_value(value, &mut candidates);
    dedupe_post_candidates(candidates)
}

// 중첩 JSON을 재귀적으로 순회하면서 게시글 ID 후보를 찾는 함수입니다.
fn collect_post_candidates_from_value(value: &Value, candidates: &mut Vec<PostCandidate>) {
    match value {
        Value::Array(items) => {
            for item in items {
                collect_post_candidates_from_value(item, candidates);
            }
        }
        Value::Object(object) => {
            if let Some(post_id) = direct_string(
                object,
                &[
                    "postId",
                    "discussionPostId",
                    "discussionId",
                    "id",
                    "articleId",
                ],
            ) {
                if looks_like_post_id(&post_id) {
                    candidates.push(PostCandidate { post_id });
                }
            }

            for child in object.values() {
                collect_post_candidates_from_value(child, candidates);
            }
        }
        _ => {}
    }
}

// JSON 문자열 또는 숫자를 후보 추출용 문자열로 바꾸는 함수입니다.
fn value_to_string(value: &Value) -> Option<String> {
    match value {
        Value::String(value) if !value.trim().is_empty() => Some(value.trim().to_owned()),
        Value::Number(value) => Some(value.to_string()),
        _ => None,
    }
}

// JSON 숫자 또는 숫자 문자열을 u64로 바꾸는 함수입니다.
fn value_to_u64(value: &Value) -> Option<u64> {
    match value {
        Value::Number(value) => value.as_u64(),
        Value::String(value) => value.parse().ok(),
        _ => None,
    }
}

// 현재 JSON 객체의 직접 필드에서만 문자열 후보를 찾는 함수입니다.
fn direct_string(object: &serde_json::Map<String, Value>, keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|key| object.get(*key).and_then(value_to_string))
}

// 현재 JSON 객체의 직접 필드에서만 숫자 후보를 찾는 함수입니다.
fn direct_number(object: &serde_json::Map<String, Value>, keys: &[&str]) -> Option<u64> {
    keys.iter()
        .find_map(|key| object.get(*key).and_then(value_to_u64))
}

// 같은 종목 코드가 여러 번 발견됐을 때 첫 후보만 남기는 함수입니다.
fn dedupe_stock_candidates(candidates: Vec<StockCandidate>) -> Vec<StockCandidate> {
    let mut seen = BTreeMap::new();
    let mut unique = Vec::new();

    for candidate in candidates {
        if seen.insert(candidate.item_code.clone(), ()).is_none() {
            unique.push(candidate);
        }
    }

    unique
}

// 같은 게시글 ID가 여러 번 발견됐을 때 첫 후보만 남기는 함수입니다.
fn dedupe_post_candidates(candidates: Vec<PostCandidate>) -> Vec<PostCandidate> {
    let mut seen = BTreeMap::new();
    let mut unique = Vec::new();

    for candidate in candidates {
        if seen.insert(candidate.post_id.clone(), ()).is_none() {
            unique.push(candidate);
        }
    }

    unique
}

// 문자열이 네이버 종목 코드 형태인지 대략적으로 판단하는 함수입니다.
fn looks_like_stock_code(value: &str) -> bool {
    let len = value.chars().count();
    (5..=8).contains(&len)
        && value
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || character == '.')
        && value.chars().any(|character| character.is_ascii_digit())
}

// 문자열이 네이버 토론글 ID 형태인지 대략적으로 판단하는 함수입니다.
fn looks_like_post_id(value: &str) -> bool {
    let len = value.chars().count();
    (6..=12).contains(&len) && value.chars().all(|character| character.is_ascii_digit())
}

// discussionType, itemCode, postId를 화면 이동용 네이버 토론 URL로 바꾸는 함수입니다.
fn discussion_url_for(discussion_type: &str, item_code: &str, post_id: Option<&str>) -> String {
    let base_path = match discussion_type {
        "domesticIndex" => format!("{STOCK_ORIGIN}/domestic/index/{item_code}/discussion"),
        "foreignStock" => format!("{STOCK_ORIGIN}/worldstock/stock/{item_code}/discussion"),
        "foreignIndex" => format!("{STOCK_ORIGIN}/worldstock/index/{item_code}/discussion"),
        _ => format!("{STOCK_ORIGIN}/domestic/stock/{item_code}/discussion"),
    };

    match post_id {
        Some(post_id) => format!("{base_path}/{post_id}?chip=all"),
        None => format!("{base_path}?chip=all"),
    }
}

// Wireshark에서 확인한 글쓰기 add 요청의 JSON 본문을 만드는 함수입니다.
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

// Wireshark에서 확인한 댓글 create 요청의 form-urlencoded 본문을 만드는 함수입니다.
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

// API 응답 문자열을 JSON으로 파싱하고 오류 메시지에 패킷 이름을 붙이는 함수입니다.
fn parse_json(response_text: &str, label: &str) -> AutomationResult<Value> {
    serde_json::from_str(response_text).map_err(|error| {
        AutomationError::new(format!(
            "{label} 패킷 응답 JSON 해석 실패: {error}; body={}",
            response_text.chars().take(200).collect::<String>()
        ))
    })
}

// getProfile JSONP 응답에서 callback wrapper를 제거하는 함수입니다.
fn strip_jsonp(response_text: &str) -> AutomationResult<&str> {
    let start = response_text
        .find('(')
        .ok_or_else(|| AutomationError::new("JSONP 응답에서 여는 괄호를 찾지 못했습니다."))?;
    let end = response_text
        .rfind(')')
        .ok_or_else(|| AutomationError::new("JSONP 응답에서 닫는 괄호를 찾지 못했습니다."))?;

    if end <= start {
        return Err(AutomationError::new(
            "JSONP 응답 괄호 위치가 올바르지 않습니다.",
        ));
    }

    Ok(&response_text[start + 1..end])
}

// HTTP 응답 상태를 확인하고 본문 문자열을 읽는 함수입니다.
fn response_text(response: reqwest::blocking::Response, label: &str) -> AutomationResult<String> {
    let status = response.status();
    let text = response.text().map_err(|error| {
        AutomationError::new(format!("{label} 패킷 응답 본문 읽기 실패: {error}"))
    })?;

    if status.is_success() {
        return Ok(text);
    }

    Err(AutomationError::new(format!(
        "{label} 패킷 HTTP 실패: status={}, body={}",
        status.as_u16(),
        text.chars().take(300).collect::<String>()
    )))
}

// 네이버 API 실패 응답에서 사람이 읽을 오류 메시지를 뽑는 함수입니다.
fn packet_error_message(value: &Value, fallback: &str) -> String {
    value
        .get("message")
        .and_then(Value::as_str)
        .or_else(|| value.get("detailCode").and_then(Value::as_str))
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| fallback.chars().take(200).collect())
}

// 글쓰기 contentJson에 넣을 임시 문서 ID를 만드는 함수입니다.
fn packet_id(prefix: &str) -> String {
    format!("SE-{prefix}-{}", timestamp_nanos())
}

// 패킷 callback과 임시 ID에 사용할 현재 시간 값을 나노초 단위로 구하는 함수입니다.
fn timestamp_nanos() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default()
}

// 실행 시점, 프로세스 ID, salt를 섞어 후보 목록에서 하나를 고르는 함수입니다.
fn pseudo_index_with_seed(len: usize, seed: u128, salt: u128) -> usize {
    if len == 0 {
        return 0;
    }

    (mix_seed(seed, salt) as usize) % len
}

// 프로세스마다 다른 랜덤 선택 기준 seed를 만드는 함수입니다.
fn selection_seed() -> u128 {
    timestamp_nanos() ^ ((process::id() as u128) << 64)
}

// seed와 salt를 섞어 낮은 자리수 편향을 줄이는 함수입니다.
fn mix_seed(seed: u128, salt: u128) -> u128 {
    let mut value = seed ^ salt.wrapping_mul(0x9E37_79B9_7F4A_7C15_6A09_E667_F3BC_C909_u128);
    value ^= value >> 64;
    value = value.wrapping_mul(0xBF58_476D_1CE4_E5B9_94D0_49BB_1331_11EB_u128);
    value ^ (value >> 61)
}

// 문자열을 reqwest HeaderValue로 변환하고 오류 메시지에 헤더 이름을 붙이는 함수입니다.
fn header_value(value: &str, label: &str) -> AutomationResult<HeaderValue> {
    HeaderValue::from_str(value)
        .map_err(|error| AutomationError::new(format!("{label} 헤더 값 생성 실패: {error}")))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use serde_json::json;

    use super::*;

    #[test]
    fn strip_jsonp_extracts_get_profile_payload() {
        let payload = r#"jsonp_123({"rtn_cd":"0","rtn_msg":"Success","nick_name":"테스트","image_url":"u"});"#;

        let json_text = strip_jsonp(payload).expect("JSONP wrapper should be removed");
        let value = parse_json(json_text, "getProfile").expect("payload should be valid JSON");

        assert_eq!(value["rtn_cd"], "0");
        assert_eq!(value["nick_name"], "테스트");
    }

    #[test]
    fn discussion_target_from_url_detects_stock_and_index_types() {
        let stock = discussion_target_from_url(
            "https://stock.naver.com/domestic/stock/005930/discussion?chip=all",
        )
        .expect("domestic stock URL should parse");
        let index = discussion_target_from_url(
            "https://stock.naver.com/domestic/index/KOSPI/discussion?chip=all",
        )
        .expect("domestic index URL should parse");

        assert_eq!(stock.discussion_type, "domesticStock");
        assert_eq!(stock.item_code, "005930");
        assert_eq!(index.discussion_type, "domesticIndex");
        assert_eq!(index.item_code, "KOSPI");
    }

    #[test]
    fn object_id_from_url_extracts_discussion_post_id() {
        let object_id = object_id_from_url(
            "https://stock.naver.com/domestic/stock/005930/discussion/421063210?chip=all",
        )
        .expect("discussion post URL should contain object id");

        assert_eq!(object_id, "421063210");
    }

    #[test]
    fn collect_stock_candidates_finds_nested_candidates_and_dedupes() {
        let value = json!({
            "result": {
                "stocks": [
                    {
                        "rank": 1,
                        "itemCode": "005930",
                        "itemName": "삼성전자"
                    },
                    {
                        "rank": 2,
                        "stockCode": "000660",
                        "stockName": "SK하이닉스"
                    },
                    {
                        "rank": 3,
                        "itemCode": "005930",
                        "itemName": "삼성전자 중복"
                    },
                    {
                        "rank": 4,
                        "itemCode": "NO_CODE",
                        "itemName": "무효"
                    }
                ]
            }
        });

        let candidates = collect_stock_candidates(&value);

        assert_eq!(candidates.len(), 2);
        assert_eq!(candidates[0].item_code, "005930");
        assert_eq!(candidates[0].item_name, "삼성전자");
        assert_eq!(candidates[0].rank, "1");
        assert_eq!(candidates[1].item_code, "000660");
        assert_eq!(candidates[1].item_name, "SK하이닉스");
    }

    #[test]
    fn collect_stock_candidates_uses_direct_row_fields_for_rank() {
        let value = json!({
            "result": {
                "rank": 99,
                "stocks": [
                    {
                        "rank": 7,
                        "itemCode": "297570",
                        "itemName": "알로이스"
                    }
                ]
            }
        });

        let candidates = collect_stock_candidates(&value);

        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].item_code, "297570");
        assert_eq!(candidates[0].item_name, "알로이스");
        assert_eq!(candidates[0].rank, "7");
    }

    #[test]
    fn collect_post_candidates_finds_nested_ids_and_dedupes() {
        let value = json!({
            "result": {
                "posts": [
                    { "postId": "421063210", "title": "첫 글" },
                    { "id": 421029979, "title": "둘째 글" },
                    { "discussionPostId": "421063210", "title": "중복 글" },
                    { "id": "abc", "title": "무효 글" }
                ]
            }
        });

        let candidates = collect_post_candidates(&value);

        assert_eq!(candidates.len(), 2);
        assert_eq!(candidates[0].post_id, "421063210");
        assert_eq!(candidates[1].post_id, "421029979");
    }

    #[test]
    fn discussion_url_for_builds_room_and_post_urls() {
        let room_url = discussion_url_for("domesticStock", "005930", None);
        let post_url = discussion_url_for("domesticStock", "005930", Some("421063210"));

        assert_eq!(
            room_url,
            "https://stock.naver.com/domestic/stock/005930/discussion?chip=all"
        );
        assert_eq!(
            post_url,
            "https://stock.naver.com/domestic/stock/005930/discussion/421063210?chip=all"
        );
    }

    #[test]
    fn build_post_payload_matches_captured_add_packet_shape() {
        let target = DiscussionTarget {
            discussion_type: "domesticStock".to_owned(),
            item_code: "005930".to_owned(),
        };

        let payload = build_post_payload("테스트 제목", "본문 내용", &target, "tx-123");

        assert_eq!(payload["title"], "테스트 제목");
        assert_eq!(payload["discussionType"], "domesticStock");
        assert_eq!(payload["itemCode"], "005930");
        assert_eq!(payload["txId"], "tx-123");
        assert_eq!(payload["inflow"], "NFS-P-P");
        assert_eq!(payload["contentJson"]["document"]["version"], "2.9.0");
        assert_eq!(
            payload["contentJson"]["document"]["components"][0]["value"][0]["nodes"][0]["value"],
            "본문 내용"
        );
    }

    #[test]
    fn build_comment_form_contains_captured_create_packet_fields() {
        let form = build_comment_form(
            "421063210",
            "https://stock.naver.com/domestic/stock/005930/discussion/421063210",
            "댓글 내용",
            "token-123",
        );
        let pairs = url::form_urlencoded::parse(form.as_bytes())
            .map(|(key, value)| (key.into_owned(), value.into_owned()))
            .collect::<BTreeMap<_, _>>();

        assert_eq!(pairs.get("objectId").map(String::as_str), Some("421063210"));
        assert_eq!(
            pairs.get("objectUrl").map(String::as_str),
            Some("https://stock.naver.com/domestic/stock/005930/discussion/421063210")
        );
        assert_eq!(pairs.get("contents").map(String::as_str), Some("댓글 내용"));
        assert_eq!(pairs.get("commentType").map(String::as_str), Some("txt"));
        assert_eq!(
            pairs.get("validateBanWords").map(String::as_str),
            Some("true")
        );
        assert_eq!(
            pairs.get("cbox_token").map(String::as_str),
            Some("token-123")
        );
    }
}
