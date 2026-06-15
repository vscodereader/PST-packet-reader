//! band api HTTP 클라이언트(순수 HTTP, reqwest).
//!
//! getKey로 `secretKey`를 받고, 각 요청 경로를 `md`로 서명해 `api-kr.band.us`에
//! 직접 POST한다. 네이버 카페 클라이언트와 동일하게 테스트에서는
//! [`BandHttpClient::with_base_urls`]로 wiremock 서버를 주입한다.
//!
//! # 보안
//! `Cookie`/`secretKey`는 사용자 자격 증명이다. 로그·에러에 절대 노출하지 않는다.

use super::{
    error::BandPostError,
    getkey::{getkey_path_now, parse_getkey_response, BandAuthKey},
    request_builder::{
        band_api_headers, build_create_comment_body, build_create_post_body, build_join_band_body,
        CREATE_COMMENT_PATH, CREATE_POST_PATH, JOIN_BAND_PATH,
    },
    response::{parse_band_result, post_no_from_result},
    signature::{make_md, make_md_jwt, signed_path},
    util::now_millis,
};
use serde_json::Value;

/// 브라우저 위장 User-Agent(캡처값과 동일 계열).
pub const BROWSER_USER_AGENT: &str =
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/148.0.0.0 Safari/537.36";

const API_ORIGIN: &str = "https://www.band.us";

/// 글 게시 성공 결과: 생성된 게시물 번호 + 응답에서 확인된 실제 밴드 이름.
#[derive(Debug, Clone)]
pub struct CreatedPost {
    pub post_no: u64,
    /// `post.band.name`. 응답에 없으면 `None`.
    pub band_name: Option<String>,
}

/// band api HTTP 클라이언트.
pub struct BandHttpClient {
    /// `https://api-kr.band.us` (테스트는 wiremock URL).
    api_base: String,
    /// `https://auth.band.us` (테스트는 wiremock URL).
    auth_base: String,
    http: reqwest::Client,
}

impl BandHttpClient {
    /// 실서버 URL을 사용하는 클라이언트.
    pub fn new() -> Self {
        Self::with_base_urls("https://api-kr.band.us", "https://auth.band.us")
    }

    /// 주입된 base URL을 사용하는 클라이언트(테스트용).
    pub fn with_base_urls(api_base: impl Into<String>, auth_base: impl Into<String>) -> Self {
        Self {
            api_base: api_base.into(),
            auth_base: auth_base.into(),
            http: reqwest::Client::new(),
        }
    }

    /// getKey로 세션 서명 키(`secretKey`)를 발급받는다.
    ///
    /// `secretKey`는 세션 중 로테이션되므로 게시 시퀀스 직전에 호출한다.
    pub async fn fetch_secret_key(
        &self,
        cookie_header: &str,
    ) -> Result<BandAuthKey, BandPostError> {
        let url = format!("{}{}", self.auth_base, getkey_path_now());
        let resp = self
            .http
            .get(&url)
            .header("User-Agent", BROWSER_USER_AGENT)
            .header("Accept", "*/*")
            .header("Cookie", cookie_header)
            .header("Referer", "https://www.band.us/")
            .send()
            .await
            .map_err(transport)?;
        let status = resp.status();
        let text = resp.text().await.map_err(transport)?;
        tracing::info!(
            "[BAND] getKey 응답 status={} content_len={}",
            status.as_u16(),
            text.len()
        );
        parse_getkey_response(&text).ok_or_else(|| {
            // 진단: secretKey 값은 가린 채 상태/응답 앞부분을 남겨 원인을 드러낸다.
            let snippet = redact_secret_key(&text);
            tracing::warn!(
                "[BAND] getKey 파싱 실패 — secretKey 없음. status={} body앞부분={}",
                status.as_u16(),
                snippet
            );
            BandPostError::no_secret_key(format!(
                "status={} 응답앞부분={}",
                status.as_u16(),
                snippet
            ))
        })
    }

    /// 서명된 POST 요청을 보내고 `result_data`를 반환한다.
    async fn post_signed(
        &self,
        base_path: &str,
        body: String,
        key: &BandAuthKey,
        cookie_header: &str,
        referer: &str,
    ) -> Result<Value, BandPostError> {
        let ts = now_millis();
        let path = signed_path(base_path, ts);
        let md = if key.is_jwt_type {
            make_md_jwt(&key.secret_key, &path)
        } else {
            make_md(&key.secret_key, &path)
        };
        let url = format!("{}{}", self.api_base, path);

        let mut req = self.http.post(&url);
        for (name, value) in band_api_headers() {
            req = req.header(&name, &value);
        }
        req = req
            .header("md", md)
            .header("Cookie", cookie_header)
            .header("Origin", API_ORIGIN)
            .header("Referer", referer)
            .header("User-Agent", BROWSER_USER_AGENT)
            .body(body);

        let resp = req.send().await.map_err(transport)?;
        let status = resp.status();
        let text = resp.text().await.map_err(transport)?;
        tracing::info!("[BAND] POST {} → status={}", base_path, status.as_u16());
        if !status.is_success() {
            return Err(BandPostError::http(status.as_u16(), text));
        }
        Ok(parse_band_result(&text)?)
    }

    /// 밴드에 가입한다.
    pub async fn join_band(
        &self,
        band_no: &str,
        key: &BandAuthKey,
        cookie_header: &str,
    ) -> Result<(), BandPostError> {
        let referer = format!("https://www.band.us/band/{band_no}/intro");
        self.post_signed(
            JOIN_BAND_PATH,
            build_join_band_body(band_no),
            key,
            cookie_header,
            &referer,
        )
        .await?;
        Ok(())
    }

    /// 밴드에 글을 게시하고 생성된 게시물 정보(`post_no` + 실제 밴드명)를 반환한다.
    pub async fn create_post(
        &self,
        band_no: &str,
        content: &str,
        key: &BandAuthKey,
        cookie_header: &str,
    ) -> Result<CreatedPost, BandPostError> {
        let referer = format!("https://www.band.us/band/{band_no}/post");
        let data = self
            .post_signed(
                CREATE_POST_PATH,
                build_create_post_body(band_no, content),
                key,
                cookie_header,
                &referer,
            )
            .await?;
        let post_no = post_no_from_result(&data).ok_or_else(|| {
            BandPostError::from(super::response::BandApiError {
                result_code: Some(1),
                message: "글 게시는 성공했으나 post_no를 찾지 못했습니다.".to_string(),
            })
        })?;
        Ok(CreatedPost {
            post_no,
            band_name: super::response::band_name_from_result(&data),
        })
    }

    /// 게시물(`post_no`)에 댓글을 단다.
    pub async fn create_comment(
        &self,
        band_no: &str,
        post_no: u64,
        comment_body: &str,
        key: &BandAuthKey,
        cookie_header: &str,
    ) -> Result<(), BandPostError> {
        let referer = format!("https://www.band.us/band/{band_no}/post/{post_no}");
        self.post_signed(
            CREATE_COMMENT_PATH,
            build_create_comment_body(band_no, post_no, comment_body),
            key,
            cookie_header,
            &referer,
        )
        .await?;
        Ok(())
    }

    /// 링크(band_no)로 밴드 이름을 조회한다(`get_band_information`, 서명된 GET).
    ///
    /// 게시 전에 저장한 링크의 실제 밴드명을 확인하는 데 쓴다. 응답에 이름이 없으면 `None`.
    pub async fn get_band_name(
        &self,
        band_no: &str,
        key: &BandAuthKey,
        cookie_header: &str,
    ) -> Result<Option<String>, BandPostError> {
        let ts = now_millis();
        // band-web과 동일한 쿼리 순서(ts → band_no)로 경로를 만든다(서명 대상).
        let path = format!("/v2.2.0/get_band_information?ts={ts}&band_no={band_no}");
        let md = if key.is_jwt_type {
            make_md_jwt(&key.secret_key, &path)
        } else {
            make_md(&key.secret_key, &path)
        };
        let url = format!("{}{}", self.api_base, path);
        let referer = format!("https://www.band.us/band/{band_no}/intro");

        let mut req = self.http.get(&url);
        for (name, value) in band_api_headers() {
            req = req.header(&name, &value);
        }
        req = req
            .header("md", md)
            .header("Cookie", cookie_header)
            .header("Origin", API_ORIGIN)
            .header("Referer", referer)
            .header("User-Agent", BROWSER_USER_AGENT);

        let resp = req.send().await.map_err(transport)?;
        let status = resp.status();
        let text = resp.text().await.map_err(transport)?;
        if !status.is_success() {
            return Err(BandPostError::http(status.as_u16(), text));
        }
        let data = parse_band_result(&text)?;
        let name = super::response::name_from_band_info(&data);
        // 진단: 밴드명을 못 뽑으면(번호 폴백) 응답 앞부분을 로그로 남겨 원인을 본다.
        if name.is_none() {
            let snip: String = text.chars().take(300).collect();
            tracing::warn!(
                "[BAND] get_band_information 밴드명 없음 band_no={} status={} body앞부분={}",
                band_no,
                status.as_u16(),
                snip
            );
        } else {
            tracing::info!("[BAND] 밴드명 조회 성공 band_no={} → {:?}", band_no, name);
        }
        Ok(name)
    }

    /// 서명된 GET 요청을 보내고 `result_data`를 반환한다(`get_band_name`과 동일 규약:
    /// path+ts를 md로 서명). `path`는 `?ts=...`까지 포함한 전체 경로여야 한다 — 서명 대상이
    /// 곧 전송 경로라, 인기글 `feed_next_param`처럼 인코딩이 필요한 값도 일치한다.
    async fn get_signed(
        &self,
        path: &str,
        referer: &str,
        key: &BandAuthKey,
        cookie_header: &str,
    ) -> Result<Value, BandPostError> {
        let md = if key.is_jwt_type {
            make_md_jwt(&key.secret_key, path)
        } else {
            make_md(&key.secret_key, path)
        };
        let url = format!("{}{}", self.api_base, path);

        let mut req = self.http.get(&url);
        for (name, value) in band_api_headers() {
            req = req.header(&name, &value);
        }
        req = req
            .header("md", md)
            .header("Cookie", cookie_header)
            .header("Origin", API_ORIGIN)
            .header("Referer", referer)
            .header("User-Agent", BROWSER_USER_AGENT);

        let resp = req.send().await.map_err(transport)?;
        let status = resp.status();
        let text = resp.text().await.map_err(transport)?;
        let endpoint = path.split('?').next().unwrap_or(path);
        tracing::info!("[BAND] GET {} → status={}", endpoint, status.as_u16());
        if !status.is_success() {
            return Err(BandPostError::http(status.as_u16(), text));
        }
        Ok(parse_band_result(&text)?)
    }

    /// 최신글의 게시물 번호를 최대 `limit`개 조회한다(`get_posts_and_announcements`,
    /// `order_by=created_at_desc&limit=N`). 단일 호출.
    ///
    /// 밴드 서버는 `limit`을 엄격히 지키지 않고 공지/고정글을 포함한 기본 페이지를 더
    /// 많이 돌려줄 수 있어, 인기글([`get_popular_posts`])과 동일하게 클라이언트에서도
    /// `limit`으로 잘라 요청 개수를 보장한다(안 자르면 "1개 선택했는데 5글에 댓글" 회귀).
    pub async fn get_latest_posts(
        &self,
        band_no: &str,
        limit: u32,
        key: &BandAuthKey,
        cookie_header: &str,
    ) -> Result<Vec<u64>, BandPostError> {
        let ts = now_millis();
        let path = format!(
            "/v2.0.0/get_posts_and_announcements?ts={ts}&band_no={band_no}&order_by=created_at_desc&limit={limit}&resolution_type=4"
        );
        let referer = format!("https://www.band.us/band/{band_no}/post");
        let data = self.get_signed(&path, &referer, key, cookie_header).await?;
        let mut post_nos = super::response::post_nos_from_feed(&data);
        // 서버가 limit을 무시하고 더 보내도 요청 개수로 캡한다.
        post_nos.truncate(limit as usize);
        Ok(post_nos)
    }

    /// 인기글의 게시물 번호를 최대 `count`개 조회한다(`get_popular_posts`). 인기글은 `limit`이
    /// 없어 `feed_next_param.offset`을 따라 페이지를 이어 받아 누적한다. 한 페이지가 비거나
    /// 다음 토큰이 없으면(마지막) 멈춘다.
    pub async fn get_popular_posts(
        &self,
        band_no: &str,
        count: u32,
        key: &BandAuthKey,
        cookie_header: &str,
    ) -> Result<Vec<u64>, BandPostError> {
        let mut collected: Vec<u64> = Vec::new();
        let mut next_param: Option<String> = None;
        // 페이지 폭주 방지 가드(한 페이지 3~4개 기준 넉넉히).
        let mut guard = 0;
        while (collected.len() as u32) < count && guard < 20 {
            guard += 1;
            let ts = now_millis();
            let mut path = format!(
                "/v2.0.0/get_popular_posts?ts={ts}&band_no={band_no}&direction=before&resolution_type=4"
            );
            if let Some(fp) = &next_param {
                // feed_next_param(JSON)을 퍼센트 인코딩해 경로에 싣고, 그 경로 그대로 서명·전송한다.
                path.push_str(&format!("&feed_next_param={}", urlencoding::encode(fp)));
            }
            let referer = format!("https://www.band.us/band/{band_no}/post");
            let data = self.get_signed(&path, &referer, key, cookie_header).await?;
            let page = super::response::post_nos_from_feed(&data);
            if page.is_empty() {
                break;
            }
            collected.extend(page);
            match super::response::feed_next_param_from_result(&data) {
                Some(fp) => next_param = Some(fp),
                None => break, // 마지막 페이지
            }
        }
        collected.truncate(count as usize);
        Ok(collected)
    }
}

impl Default for BandHttpClient {
    fn default() -> Self {
        Self::new()
    }
}

fn transport(e: reqwest::Error) -> BandPostError {
    BandPostError::transport(e.to_string())
}

/// 진단용: 응답에서 secretKey 값을 가리고 앞부분(최대 250자)만 남긴다.
/// 자격 증명이 로그/에러로 새지 않게 하면서 실패 원인(에러 페이지/리다이렉트 등)을 보이게 한다.
fn redact_secret_key(text: &str) -> String {
    let mut out = String::with_capacity(text.len().min(260));
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        // `secretKey` 토큰을 만나면 그 뒤 따옴표 안 값을 <redacted>로 치환한다.
        if text[i..].starts_with("secretKey") {
            out.push_str("secretKey<redacted>");
            i += "secretKey".len();
            // 다음 따옴표 쌍을 건너뛴다(값 숨김).
            if let Some(q) = text[i..].find(['\'', '"']) {
                let quote = text.as_bytes()[i + q];
                if let Some(end) = text[i + q + 1..].find(quote as char) {
                    i = i + q + 1 + end + 1;
                }
            }
            continue;
        }
        let ch = text[i..].chars().next().unwrap_or(' ');
        out.push(ch);
        i += ch.len_utf8();
        if out.chars().count() >= 250 {
            out.push('…');
            break;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::super::error::BandPostErrorKind;
    use super::*;
    use wiremock::{
        matchers::{header, header_exists, method, path_regex},
        Mock, MockServer, ResponseTemplate,
    };

    fn test_key() -> BandAuthKey {
        BandAuthKey {
            secret_key: "krYc6CZR5GYpPFSld8a/nPYnYMZ/Y2YHYGo5gYHHLSs=".to_string(),
            is_jwt_type: false,
        }
    }

    const FAKE_COOKIE: &str = "BUC=FAKE_FOR_TEST";

    #[tokio::test]
    async fn fetch_secret_key_parses_getkey() {
        let auth = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path_regex(r"^/s/login/getKey"))
            .respond_with(ResponseTemplate::new(200).set_body_string(
                "authCallBack_1(new BandWebAuthModule({ secretKey: 'KEY123=', isJwtType: false }))",
            ))
            .mount(&auth)
            .await;

        let client = BandHttpClient::with_base_urls("http://unused", auth.uri());
        let key = client.fetch_secret_key(FAKE_COOKIE).await.unwrap();
        assert_eq!(key.secret_key, "KEY123=");
    }

    #[tokio::test]
    async fn join_band_sends_md_akey_cookie_and_succeeds() {
        let api = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path_regex(r"^/v2\.1\.0/join_band"))
            .and(header("akey", "bbc59b0b5f7a1c6efe950f6236ccda35"))
            .and(header_exists("md"))
            .and(header("Cookie", FAKE_COOKIE))
            .respond_with(ResponseTemplate::new(200).set_body_string(
                r#"{"result_code":1,"result_data":{"message":"밴드에 가입했습니다."}}"#,
            ))
            .mount(&api)
            .await;

        let client = BandHttpClient::with_base_urls(api.uri(), "http://unused");
        client
            .join_band("103043410", &test_key(), FAKE_COOKIE)
            .await
            .expect("가입 성공이어야 함");
    }

    #[tokio::test]
    async fn create_post_returns_post_no_and_band_name() {
        let api = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path_regex(r"^/v2\.0\.2/create_post"))
            .respond_with(ResponseTemplate::new(200).set_body_string(
                r#"{"result_code":1,"result_data":{"post":{"post_no":2,"web_url":"https://band.us/band/103043410/post/2","band":{"band_no":103043410,"name":"데일밴드"}}}}"#,
            ))
            .mount(&api)
            .await;

        let client = BandHttpClient::with_base_urls(api.uri(), "http://unused");
        let created = client
            .create_post("103043410", "제목\n내용", &test_key(), FAKE_COOKIE)
            .await
            .expect("게시 성공이어야 함");
        assert_eq!(created.post_no, 2);
        assert_eq!(created.band_name.as_deref(), Some("데일밴드"));
    }

    #[tokio::test]
    async fn create_comment_succeeds() {
        let api = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path_regex(r"^/v2\.3\.0/create_comment"))
            .respond_with(
                ResponseTemplate::new(200).set_body_string(
                    r#"{"result_code":1,"result_data":{"comment":{"comment_id":1}}}"#,
                ),
            )
            .mount(&api)
            .await;

        let client = BandHttpClient::with_base_urls(api.uri(), "http://unused");
        client
            .create_comment("103043410", 2, "댓글", &test_key(), FAKE_COOKIE)
            .await
            .expect("댓글 성공이어야 함");
    }

    #[test]
    fn redact_secret_key_hides_value_keeps_context() {
        let text =
            "authCallBack_1(new BandWebAuthModule({ secretKey: 'TOPSECRET=', isJwtType: false }))";
        let red = redact_secret_key(text);
        assert!(!red.contains("TOPSECRET"), "secretKey 값이 노출됨: {red}");
        assert!(red.contains("redacted"));
        assert!(red.contains("isJwtType"), "맥락(파싱 단서)은 남아야 함");
    }

    #[tokio::test]
    async fn get_band_name_returns_name() {
        let api = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path_regex(r"^/v2\.2\.0/get_band_information"))
            .and(header_exists("md"))
            .respond_with(ResponseTemplate::new(200).set_body_string(
                r#"{"result_code":1,"result_data":{"band_no":103043410,"name":"데일밴드"}}"#,
            ))
            .mount(&api)
            .await;

        let client = BandHttpClient::with_base_urls(api.uri(), "http://unused");
        let name = client
            .get_band_name("103043410", &test_key(), FAKE_COOKIE)
            .await
            .expect("조회 성공이어야 함");
        assert_eq!(name.as_deref(), Some("데일밴드"));
    }

    #[tokio::test]
    async fn get_latest_posts_returns_post_nos() {
        let api = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path_regex(r"^/v2\.0\.0/get_posts_and_announcements"))
            .and(header_exists("md"))
            .respond_with(ResponseTemplate::new(200).set_body_string(
                r#"{"result_code":1,"result_data":{"items":[{"post":{"post_no":8}},{"post":{"post_no":7}}]}}"#,
            ))
            .mount(&api)
            .await;

        let client = BandHttpClient::with_base_urls(api.uri(), "http://unused");
        let posts = client
            .get_latest_posts("103043410", 3, &test_key(), FAKE_COOKIE)
            .await
            .expect("최신글 조회 성공이어야 함");
        assert_eq!(posts, vec![8, 7]);
    }

    #[tokio::test]
    async fn get_latest_posts_truncates_when_server_returns_more_than_limit() {
        // 회귀 방지: 서버가 limit을 무시하고 5건을 줘도 요청 개수(2)로 잘라야 한다.
        // ("최신글 1·3개 선택했는데 5글에 댓글" 버그의 근본 원인.)
        let api = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path_regex(r"^/v2\.0\.0/get_posts_and_announcements"))
            .and(header_exists("md"))
            .respond_with(ResponseTemplate::new(200).set_body_string(
                r#"{"result_code":1,"result_data":{"items":[{"post":{"post_no":8}},{"post":{"post_no":7}},{"post":{"post_no":6}},{"post":{"post_no":5}},{"post":{"post_no":4}}]}}"#,
            ))
            .mount(&api)
            .await;

        let client = BandHttpClient::with_base_urls(api.uri(), "http://unused");
        let posts = client
            .get_latest_posts("103043410", 2, &test_key(), FAKE_COOKIE)
            .await
            .expect("최신글 조회 성공이어야 함");
        assert_eq!(posts, vec![8, 7]);
    }

    #[tokio::test]
    async fn get_popular_posts_truncates_to_count_and_stops_on_null_next() {
        let api = MockServer::start().await;
        // 한 페이지에 3건 + 다음 토큰 없음(null) → count=2면 앞 2건만, 추가 호출 없음.
        // 인기글 실제 구조: 항목 자체가 글이라 post_no가 최상위(post 래퍼 없음).
        Mock::given(method("GET"))
            .and(path_regex(r"^/v2\.0\.0/get_popular_posts"))
            .and(header_exists("md"))
            .respond_with(ResponseTemplate::new(200).set_body_string(
                r#"{"result_code":1,"result_data":{"paging":{"next_params":null},"items":[{"post_no":3709},{"post_no":3710},{"post_no":3711}]}}"#,
            ))
            .mount(&api)
            .await;

        let client = BandHttpClient::with_base_urls(api.uri(), "http://unused");
        let posts = client
            .get_popular_posts("72247938", 2, &test_key(), FAKE_COOKIE)
            .await
            .expect("인기글 조회 성공이어야 함");
        assert_eq!(posts, vec![3709, 3710]);
    }

    #[tokio::test]
    async fn api_error_result_code_is_surfaced() {
        let api = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path_regex(r"^/v2\.1\.0/join_band"))
            .respond_with(ResponseTemplate::new(200).set_body_string(
                r#"{"result_code":1004,"result_data":{"message":"가입할 수 없는 밴드입니다."}}"#,
            ))
            .mount(&api)
            .await;

        let client = BandHttpClient::with_base_urls(api.uri(), "http://unused");
        let err = client
            .join_band("103043410", &test_key(), FAKE_COOKIE)
            .await
            .expect_err("실패여야 함");
        match err.kind {
            BandPostErrorKind::Api(api_err) => {
                assert_eq!(api_err.result_code, Some(1004));
                assert_eq!(api_err.message, "가입할 수 없는 밴드입니다.");
            }
            other => panic!("Api 오류여야 함: {other:?}"),
        }
    }

    #[tokio::test]
    async fn http_500_is_http_error() {
        let api = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path_regex(r"^/v2\.0\.2/create_post"))
            .respond_with(ResponseTemplate::new(500).set_body_string("oops"))
            .mount(&api)
            .await;

        let client = BandHttpClient::with_base_urls(api.uri(), "http://unused");
        let err = client
            .create_post("1", "x", &test_key(), FAKE_COOKIE)
            .await
            .expect_err("500은 실패");
        assert!(matches!(
            err.kind,
            BandPostErrorKind::Http { status: 500, .. }
        ));
    }
}
