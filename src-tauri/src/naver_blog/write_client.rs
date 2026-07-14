//! 네이버 블로그 새 글 발행(RabbitWrite) HTTP 클라이언트.
//!
//! 패킷(2026-07-13) 분석: 새 글 발행은 `POST blog.naver.com/RabbitWrite.naver`(form-urlencoded)로
//! documentModel(SmartEditor v2.10.2 JSON)·populationParams(발행 설정)·tokenId 등을 보낸다. 발행에
//! 쓰이는 `tokenId`는 서버가 주는 값이 아니라 **클라이언트가 만든 32바이트 값**이라(어떤 응답에도
//! 없고 PostWriteForm 의 token 은 빈 값), 여기서 직접 생성한다. 서버가 이 값을 ncpt 봇탐지와
//! 교차검증하면 HTTP 단독으론 거부될 수 있어(그때만 CDP), 우선 HTTP로 시도해 실측한다.
//!
//! 카페/댓글 클라이언트와 동일하게 base_url을 분리 보관해 실서버/wiremock을 함께 쓴다.

use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};

use super::error::BlogError;

/// 블로그 본문 발행 호스트.
const BLOG_HOST: &str = "https://blog.naver.com";

/// 공개 설정(공개 범위). 패킷 확정(예약+공개범위): openType 숫자값과 UI 순서 일치.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenType {
    /// 전체공개(0).
    Public,
    /// 이웃공개(1).
    Neighbor,
    /// 서로이웃공개(2).
    MutualNeighbor,
    /// 비공개(3).
    Private,
}

impl OpenType {
    /// populationParams.configuration.openType 에 들어가는 숫자값.
    pub fn code(self) -> u8 {
        match self {
            OpenType::Public => 0,
            OpenType::Neighbor => 1,
            OpenType::MutualNeighbor => 2,
            OpenType::Private => 3,
        }
    }
}

/// 발행 시간 설정. 현재 발행 또는 예약 발행(연·월·일·시·분).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PublishTime {
    /// 지금 발행(postWriteTimeType="now").
    Now,
    /// 예약 발행(postWriteTimeType="pre") — 예약 날짜/시각.
    Reserve {
        year: u32,
        month: u32,
        date: u32,
        hour: u32,
        minute: u32,
    },
}

/// 블로그 글 발행 설정(발행 UI ↔ populationParams). 형님이 UI에서 고른 값이 그대로 매핑된다.
#[derive(Debug, Clone)]
pub struct BlogPublishSettings {
    pub open_type: OpenType,
    pub comment_yn: bool,
    pub search_yn: bool,
    pub sympathy_yn: bool,
    /// 블로그/카페 공유: 0=허용 안 함, 2=링크 허용.
    pub scrap_type: u8,
    pub out_side_allow_yn: bool,
    /// 카테고리(게시판) id. 기본 1.
    pub category_id: u32,
    /// 주제 디렉토리. 0=주제 선택 안 함.
    pub directory_seq: u32,
    /// 태그: # 없이 순수 단어를 공백으로 구분("첫글 인생"). 빈 문자열이면 태그 없음.
    pub tags: String,
    /// 발행 시간(현재/예약).
    pub publish_time: PublishTime,
    /// 공지사항 등록 여부.
    pub notice_post_yn: bool,
}

impl Default for BlogPublishSettings {
    fn default() -> Self {
        Self {
            open_type: OpenType::Public,
            comment_yn: true,
            search_yn: true,
            sympathy_yn: true,
            scrap_type: 2,
            out_side_allow_yn: true,
            category_id: 1,
            directory_seq: 0,
            tags: String::new(),
            publish_time: PublishTime::Now,
            notice_post_yn: false,
        }
    }
}

/// 발행 성공 결과. 게시글 번호(logNo)와 링크를 보존한다(결과 링크·완료 로그용).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BlogWriteResult {
    /// 게시글 번호. 예약 발행은 아직 없을 수 있어 `None`.
    pub log_no: Option<String>,
    /// 게시글/리다이렉트 URL(성공 응답의 redirectUrl).
    pub redirect_url: String,
}

/// 네이버 블로그 새 글 발행 HTTP 클라이언트.
pub struct BlogWriteClient {
    blog_base: String,
    http: reqwest::Client,
}

impl Default for BlogWriteClient {
    fn default() -> Self {
        Self::new()
    }
}

impl BlogWriteClient {
    /// 실서버 호스트를 사용하는 클라이언트를 생성한다.
    pub fn new() -> Self {
        Self::with_base_url(BLOG_HOST)
    }

    /// 주입된 base_url을 사용하는 클라이언트를 생성한다(테스트용).
    pub fn with_base_url(blog_base: impl Into<String>) -> Self {
        Self {
            blog_base: blog_base.into(),
            http: crate::naver_cafe::shared_http_client(),
        }
    }

    /// 새 글을 발행한다. 저장 쿠키(`cookie`)로 로그인 세션을 싣고 RabbitWrite로 POST한다.
    ///
    /// # 쿠키 보안
    /// `cookie`는 사용자 인증 자격 증명이며 에러/로그에 노출하지 않는다.
    pub async fn publish(
        &self,
        blog_id: &str,
        title: &str,
        content: &str,
        settings: &BlogPublishSettings,
        cookie: &str,
    ) -> Result<BlogWriteResult, BlogError> {
        let document_model = build_document_model(title, content);
        self.publish_document(blog_id, document_model, settings, cookie)
            .await
    }

    /// 이미 조립된 documentModel(제목 + 본문 컴포넌트들)을 그대로 발행한다(툴바 블록 경로).
    ///
    /// 순수 텍스트 [`Self::publish`]와 발행 로직(폼·헤더·토큰·응답 파싱)을 공유하며, 차이는
    /// documentModel을 호출부가 만들어 넘긴다는 점뿐이다.
    ///
    /// # 쿠키 보안
    /// `cookie`는 사용자 인증 자격 증명이며 에러/로그에 노출하지 않는다.
    pub async fn publish_document(
        &self,
        blog_id: &str,
        document_model: Value,
        settings: &BlogPublishSettings,
        cookie: &str,
    ) -> Result<BlogWriteResult, BlogError> {
        let population_params = build_population_params(settings);
        let token_id = generate_token_id();
        let form = [
            ("blogId", blog_id.to_string()),
            ("documentModel", document_model.to_string()),
            (
                "mediaResources",
                json!({"image":[],"video":[],"file":[]}).to_string(),
            ),
            ("populationParams", population_params.to_string()),
            ("productApiVersion", "v1".to_string()),
            ("tokenId", token_id),
        ];
        let url = format!("{}/RabbitWrite.naver", self.blog_base);
        let resp = self
            .http
            .post(&url)
            .header("User-Agent", crate::naver_cafe::post::BROWSER_USER_AGENT)
            .header("Accept", "application/json")
            .header("Origin", BLOG_HOST)
            .header("Referer", format!("{BLOG_HOST}/{blog_id}?Redirect=Write"))
            .header("Content-Type", "application/x-www-form-urlencoded")
            .header("Cookie", cookie)
            .form(&form)
            .send()
            .await
            .map_err(|e| BlogError::new(format!("블로그 발행 요청 실패: {e}")))?;
        let status = resp.status();
        let text = resp
            .text()
            .await
            .map_err(|e| BlogError::new(format!("블로그 발행 응답 읽기 실패: {e}")))?;
        // 게시 API 원문 로그(형님 필수): 상태·본문을 필터 없이 남긴다(쿠키는 본문에 없어 안전).
        tracing::info!(
            "[BLOG] RabbitWrite 응답 — status={} body={}",
            status.as_u16(),
            snippet(&text)
        );
        if let Some(result) = parse_write_response(&text) {
            return Ok(result);
        }
        // 실패 응답 분류: 종토→블로그로 바꾼 계정은 네이버 블로그가 없어 `{"isSuccess":false,
        // "errorCode":"no privilege"}`가 온다(실측). 이때는 "블로그 없음(생성 필요)"으로 명확히 알려
        // 프론트가 계정별로 표시하게 한다. 그 외 실패는 봇차단 등 일반 실패로 남긴다.
        if is_no_privilege(&text) {
            return Err(BlogError::new(format!(
                "이 계정에 네이버 블로그가 없습니다(블로그 생성이 필요합니다). blogId={blog_id}"
            )));
        }
        Err(BlogError::new(format!(
            "블로그 발행 실패(성공 응답이 아님). status={} 응답={}",
            status.as_u16(),
            snippet(&text)
        )))
    }
}

/// RabbitWrite 실패 응답이 "블로그 없음"(`errorCode == "no privilege"`)인지 판정한다(순수 함수).
/// 발행 흐름은 사전에 SeOptions로 존재확인+자동생성하므로 정상적으론 안 오지만, 최종 백스톱이다.
fn is_no_privilege(text: &str) -> bool {
    serde_json::from_str::<Value>(text)
        .ok()
        .and_then(|v| {
            v.get("errorCode")
                .and_then(Value::as_str)
                .map(|c| c.trim().eq_ignore_ascii_case("no privilege"))
        })
        .unwrap_or(false)
}

/// 제목·내용을 SmartEditor v2.10.2 documentModel(JSON)로 만든다(순수 함수). 내용은 줄바꿈마다
/// 한 문단(paragraph)으로 나눈다. 각 요소 id는 클라이언트 생성값(SE-uuid / document ULID)이다.
pub fn build_document_model(title: &str, content: &str) -> Value {
    let title_component = json!({
        "id": se_id(),
        "layout": "default",
        "title": [ paragraph(title) ],
        "subTitle": null,
        "align": "left",
        "@ctype": "documentTitle"
    });
    // 내용 문단: 빈 내용이어도 최소 한 문단은 넣는다(발행 가능한 최소 문서).
    let mut paragraphs: Vec<Value> = content.split('\n').map(paragraph).collect();
    if paragraphs.is_empty() {
        paragraphs.push(paragraph(""));
    }
    let text_component = json!({
        "id": se_id(),
        "layout": "default",
        "value": paragraphs,
        "@ctype": "text"
    });
    json!({
        "documentId": "",
        "document": {
            "version": "2.10.2",
            "theme": "default",
            "language": "ko-KR",
            "id": ulid(),
            "components": [ title_component, text_component ]
        }
    })
}

/// 제목 컴포넌트 + 이미 조립된 본문 컴포넌트들로 documentModel(JSON)을 만든다(툴바 블록 경로).
///
/// [`build_document_model`]과 문서 골격(version/theme/language/id)은 동일하고, 본문만
/// 호출부(블록 → 컴포넌트 빌더)가 만든 `content_components`를 그대로 싣는다. 본문이 비어 있으면
/// 발행 가능한 최소 문서를 위해 빈 text 컴포넌트 하나를 넣는다.
pub fn build_document_model_with_components(title: &str, content_components: Vec<Value>) -> Value {
    let title_component = json!({
        "id": se_id(),
        "layout": "default",
        "title": [ paragraph(title) ],
        "subTitle": null,
        "align": "left",
        "@ctype": "documentTitle"
    });
    let mut components = vec![title_component];
    if content_components.is_empty() {
        components.push(json!({
            "id": se_id(),
            "layout": "default",
            "value": [ paragraph("") ],
            "@ctype": "text"
        }));
    } else {
        components.extend(content_components);
    }
    json!({
        "documentId": "",
        "document": {
            "version": "2.10.2",
            "theme": "default",
            "language": "ko-KR",
            "id": ulid(),
            "components": components
        }
    })
}

/// 한 줄 → SmartEditor paragraph(단순 textNode). 스타일 없는 평문.
fn paragraph(text: &str) -> Value {
    json!({
        "id": se_id(),
        "nodes": [ { "id": se_id(), "value": text, "@ctype": "textNode" } ],
        "@ctype": "paragraph"
    })
}

/// 발행 설정을 populationParams(JSON)로 만든다(순수 함수). 패킷(예약+공개범위) 필드 그대로.
pub fn build_population_params(s: &BlogPublishSettings) -> Value {
    let (time_type, (year, month, date, hour, minute)) = match s.publish_time {
        PublishTime::Now => ("now", (0, 0, 0, 0, 0)),
        PublishTime::Reserve {
            year,
            month,
            date,
            hour,
            minute,
        } => ("pre", (year, month, date, hour, minute)),
    };
    json!({
        "configuration": {
            "openType": s.open_type.code(),
            "commentYn": s.comment_yn,
            "searchYn": s.search_yn,
            "sympathyYn": s.sympathy_yn,
            "scrapType": s.scrap_type,
            "outSideAllowYn": s.out_side_allow_yn,
            "twitterPostingYn": false,
            "facebookPostingYn": false,
            "cclYn": false
        },
        "populationMeta": {
            "categoryId": s.category_id,
            "logNo": null,
            "directorySeq": s.directory_seq,
            "directoryDetail": null,
            "mrBlogTalkCode": null,
            "postWriteTimeType": time_type,
            "tags": s.tags,
            "moviePanelParticipation": false,
            "greenReviewBannerYn": false,
            "continueSaved": false,
            "noticePostYn": s.notice_post_yn,
            "autoByCategoryYn": false,
            "postLocationSupportYn": false,
            "postLocationJson": null,
            "prePostDate": date,
            "thisDayPostInfo": null,
            "scrapYn": false,
            "prePostRegistDirectly": false,
            "prePostYear": year,
            "prePostMonth": month,
            "prePostHour": hour,
            "prePostMinute": minute
        }
    })
}

/// RabbitWrite 성공 응답에서 redirectUrl·logNo를 뽑는다(순수 함수). 실패/봇차단 응답이면 `None`.
/// 성공: `{"isSuccess":true,"result":{"redirectUrl":"...PostView.naver?blogId=X&logNo=Y"}}`.
fn parse_write_response(text: &str) -> Option<BlogWriteResult> {
    let value: Value = serde_json::from_str(text).ok()?;
    if value.get("isSuccess").and_then(Value::as_bool) != Some(true) {
        return None;
    }
    let redirect_url = value
        .get("result")?
        .get("redirectUrl")?
        .as_str()?
        .to_string();
    let log_no = extract_log_no(&redirect_url);
    Some(BlogWriteResult {
        log_no,
        redirect_url,
    })
}

/// URL 쿼리스트링에서 `logNo` 값을 뽑는다(순수 함수). 없으면(예약 발행 등) `None`.
fn extract_log_no(url: &str) -> Option<String> {
    let query = url.split('?').nth(1)?;
    query.split('&').find_map(|pair| {
        let (k, v) = pair.split_once('=')?;
        (k == "logNo").then(|| v.to_string())
    })
}

/// 클라이언트 생성 tokenId(32바이트) — base64url(패딩 포함). 서버가 주지 않는 값이라 직접 만든다.
/// 블로그 자동 생성(BlogDomainRegistration)도 같은 방식의 tokenId를 쓰므로 `pub(crate)`로 공유한다.
pub(crate) fn generate_token_id() -> String {
    use base64::Engine;
    let mut bytes = [0u8; 32];
    fill_random(&mut bytes);
    base64::engine::general_purpose::URL_SAFE.encode(bytes)
}

/// SmartEditor 요소 id("SE-" + uuid v4 형식).
pub(crate) fn se_id() -> String {
    format!("SE-{}", uuid_v4())
}

/// uuid v4 형식 문자열(8-4-4-4-12 hex). 암호학적 강도는 불필요(문서 내 고유성만).
fn uuid_v4() -> String {
    let mut b = [0u8; 16];
    fill_random(&mut b);
    b[6] = (b[6] & 0x0f) | 0x40; // version 4
    b[8] = (b[8] & 0x3f) | 0x80; // variant
    let h: String = b.iter().map(|x| format!("{x:02x}")).collect();
    format!(
        "{}-{}-{}-{}-{}",
        &h[0..8],
        &h[8..12],
        &h[12..16],
        &h[16..20],
        &h[20..32]
    )
}

/// ULID(26자 Crockford base32) — documentModel.document.id 용. 시간+랜덤.
pub(crate) fn ulid() -> String {
    const ALPHABET: &[u8] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";
    let ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u128)
        .unwrap_or(0);
    let mut rand = [0u8; 10];
    fill_random(&mut rand);
    let mut value: u128 = (ms & ((1 << 48) - 1)) << 80;
    for (i, byte) in rand.iter().enumerate() {
        value |= (*byte as u128) << (8 * (9 - i));
    }
    let mut out = [0u8; 26];
    for i in (0..26).rev() {
        out[i] = ALPHABET[(value & 0x1f) as usize];
        value >>= 5;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// SystemTime(나노) 시드 splitmix64로 바이트 버퍼를 채운다. 암호학적 난수는 아니지만 문서 id·
/// 세션 토큰 후보로 충분하다(실서버가 tokenId를 강하게 검증하면 어차피 CDP 경로로 간다).
fn fill_random(buf: &mut [u8]) {
    let mut state = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0)
        ^ (buf.as_ptr() as u64);
    for chunk in buf.chunks_mut(8) {
        state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^= z >> 31;
        for (i, b) in chunk.iter_mut().enumerate() {
            *b = (z >> (8 * i)) as u8;
        }
    }
}

/// 로그·에러용 응답 앞부분 스니펫(최대 300자).
fn snippet(text: &str) -> String {
    text.chars().take(300).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[test]
    fn document_model_has_title_and_content() {
        let dm = build_document_model("제목입니다", "첫줄\n둘째줄");
        let comps = dm["document"]["components"].as_array().unwrap();
        assert_eq!(comps.len(), 2);
        assert_eq!(comps[0]["@ctype"], "documentTitle");
        assert_eq!(comps[0]["title"][0]["nodes"][0]["value"], "제목입니다");
        assert_eq!(comps[1]["@ctype"], "text");
        let paras = comps[1]["value"].as_array().unwrap();
        assert_eq!(paras.len(), 2);
        assert_eq!(paras[0]["nodes"][0]["value"], "첫줄");
        assert_eq!(paras[1]["nodes"][0]["value"], "둘째줄");
        assert_eq!(dm["document"]["version"], "2.10.2");
    }

    #[test]
    fn document_model_empty_content_has_one_paragraph() {
        let dm = build_document_model("t", "");
        let paras = dm["document"]["components"][1]["value"].as_array().unwrap();
        assert_eq!(paras.len(), 1);
    }

    #[test]
    fn population_params_now_maps_settings() {
        let s = BlogPublishSettings {
            open_type: OpenType::MutualNeighbor,
            tags: "첫글 인생".to_string(),
            ..Default::default()
        };
        let p = build_population_params(&s);
        assert_eq!(p["configuration"]["openType"], 2);
        assert_eq!(p["populationMeta"]["postWriteTimeType"], "now");
        assert_eq!(p["populationMeta"]["tags"], "첫글 인생");
        assert_eq!(p["populationMeta"]["categoryId"], 1);
        assert_eq!(p["populationMeta"]["directorySeq"], 0);
    }

    #[test]
    fn population_params_reserve_sets_pre_fields() {
        let s = BlogPublishSettings {
            open_type: OpenType::Public,
            publish_time: PublishTime::Reserve {
                year: 2026,
                month: 7,
                date: 22,
                hour: 8,
                minute: 30,
            },
            ..Default::default()
        };
        let p = build_population_params(&s);
        assert_eq!(p["configuration"]["openType"], 0);
        assert_eq!(p["populationMeta"]["postWriteTimeType"], "pre");
        assert_eq!(p["populationMeta"]["prePostYear"], 2026);
        assert_eq!(p["populationMeta"]["prePostMonth"], 7);
        assert_eq!(p["populationMeta"]["prePostDate"], 22);
        assert_eq!(p["populationMeta"]["prePostHour"], 8);
        assert_eq!(p["populationMeta"]["prePostMinute"], 30);
    }

    #[test]
    fn open_type_codes_match_ui_order() {
        assert_eq!(OpenType::Public.code(), 0);
        assert_eq!(OpenType::Neighbor.code(), 1);
        assert_eq!(OpenType::MutualNeighbor.code(), 2);
        assert_eq!(OpenType::Private.code(), 3);
    }

    #[test]
    fn parse_write_response_extracts_log_no() {
        let r = parse_write_response(
            r#"{"isSuccess":true,"result":{"redirectUrl":"https://blog.naver.com/PostView.naver?blogId=choisw0404&logNo=224311392458"}}"#,
        )
        .unwrap();
        assert_eq!(r.log_no.as_deref(), Some("224311392458"));
        assert!(r.redirect_url.contains("PostView.naver"));
    }

    #[test]
    fn parse_write_response_reserve_has_no_log_no() {
        let r = parse_write_response(
            r#"{"isSuccess":true,"result":{"redirectUrl":"https://blog.naver.com/PostList.naver?blogId=choisw0404"}}"#,
        )
        .unwrap();
        assert_eq!(r.log_no, None);
    }

    #[test]
    fn parse_write_response_none_on_failure() {
        assert!(parse_write_response(r#"{"isSuccess":false}"#).is_none());
        assert!(parse_write_response("<html>bot</html>").is_none());
    }

    #[test]
    fn is_no_privilege_detects_missing_blog() {
        assert!(is_no_privilege(
            r#"{"isSuccess":false,"errorCode":"no privilege"}"#
        ));
        assert!(is_no_privilege(r#"{"errorCode":"No Privilege"}"#));
        assert!(!is_no_privilege(r#"{"isSuccess":false,"errorCode":"bot"}"#));
        assert!(!is_no_privilege("<html>bot</html>"));
    }

    #[tokio::test]
    async fn publish_errors_no_blog_on_no_privilege() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/RabbitWrite.naver"))
            .respond_with(ResponseTemplate::new(200).set_body_string(
                r#"{"isSuccess":false,"errorCode":"no privilege"}"#,
            ))
            .mount(&server)
            .await;
        let client = BlogWriteClient::with_base_url(server.uri());
        let err = client
            .publish("b", "t", "c", &BlogPublishSettings::default(), "NID_SES=abc")
            .await
            .expect_err("no privilege는 Err여야 함");
        assert!(err.message().contains("블로그가 없습니다"));
    }

    #[test]
    fn generated_ids_are_unique_and_shaped() {
        assert_ne!(se_id(), se_id());
        assert!(se_id().starts_with("SE-"));
        assert_eq!(ulid().len(), 26);
        assert_eq!(uuid_v4().len(), 36);
        // tokenId: base64url of 32 bytes → 44 chars(패딩 포함).
        assert_eq!(generate_token_id().len(), 44);
        assert_ne!(generate_token_id(), generate_token_id());
    }

    #[tokio::test]
    async fn publish_sends_rabbitwrite_and_parses_result() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/RabbitWrite.naver"))
            .and(wiremock::matchers::header("Cookie", "NID_SES=abc"))
            .respond_with(ResponseTemplate::new(200).set_body_string(
                r#"{"isSuccess":true,"result":{"redirectUrl":"https://blog.naver.com/PostView.naver?blogId=b&logNo=999"}}"#,
            ))
            .mount(&server)
            .await;
        let client = BlogWriteClient::with_base_url(server.uri());
        let r = client
            .publish("b", "제목", "내용", &BlogPublishSettings::default(), "NID_SES=abc")
            .await
            .unwrap();
        assert_eq!(r.log_no.as_deref(), Some("999"));
    }

    #[tokio::test]
    async fn publish_errors_on_bot_response() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/RabbitWrite.naver"))
            .respond_with(ResponseTemplate::new(200).set_body_string("<html>bot</html>"))
            .mount(&server)
            .await;
        let client = BlogWriteClient::with_base_url(server.uri());
        assert!(client
            .publish("b", "t", "c", &BlogPublishSettings::default(), "NID_SES=abc")
            .await
            .is_err());
    }
}
