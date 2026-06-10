//! band api 요청(가입·글쓰기·댓글)의 경로·폼바디·공통 헤더 빌더(순수 함수).
//!
//! 실제 HTTP 전송은 [`client`](crate::band_post::client)가 담당한다. 이 모듈은
//! 패킷 캡처로 확인된 요청 구조만 생성한다. `md` 서명과 쿠키는 클라이언트가 주입한다.
//!
//! 바디는 모두 `application/x-www-form-urlencoded`이며, 캡처된 실제 바디 문자열을
//! 그대로 재현하도록 필드 순서/기본값을 맞췄다.

use serde::Serialize;

use super::signature::APP_KEY;

/// API 호스트.
pub const API_HOST: &str = "api-kr.band.us";

/// 가입 엔드포인트 경로(ts 쿼리 제외).
pub const JOIN_BAND_PATH: &str = "/v2.1.0/join_band";
/// 글쓰기 엔드포인트 경로(ts 쿼리 제외).
pub const CREATE_POST_PATH: &str = "/v2.0.2/create_post";
/// 댓글 엔드포인트 경로(ts 쿼리 제외).
pub const CREATE_COMMENT_PATH: &str = "/v2.3.0/create_comment";

/// 폼바디 Content-Type(캡처값).
pub const FORM_CONTENT_TYPE: &str = "application/x-www-form-urlencoded; charset=UTF-8";

// ---------------------------------------------------------------------------
// 바디 모델 (serde_urlencoded 직렬화 — 필드 순서 = 캡처 순서)
// ---------------------------------------------------------------------------

/// 밴드 가입 요청 바디. 캡처: `join_type=band_no&join_value=<no>&profile_id=1`.
#[derive(Debug, Clone, Serialize, PartialEq)]
struct JoinBandBody {
    join_type: String,
    join_value: String,
    profile_id: u32,
}

/// 글쓰기 요청 바디.
/// 캡처: `band_no=&content=&set_band_notice=false&set_major_band_notice=false&
/// set_linked_band_notice=&copiable_state=&should_disable_comment=false&
/// band_notice_unset_at=&purpose=create`.
#[derive(Debug, Clone, Serialize, PartialEq)]
struct CreatePostBody {
    band_no: String,
    content: String,
    set_band_notice: bool,
    set_major_band_notice: bool,
    set_linked_band_notice: String,
    copiable_state: String,
    should_disable_comment: bool,
    band_notice_unset_at: String,
    purpose: String,
}

/// 댓글 요청 바디.
/// 캡처: `band_no=&member_type=membership&body=&photos=&file=&video=&
/// content_key={json}&is_secret=false&resolution_type=4`.
#[derive(Debug, Clone, Serialize, PartialEq)]
struct CreateCommentBody {
    band_no: String,
    member_type: String,
    body: String,
    photos: String,
    file: String,
    video: String,
    content_key: String,
    is_secret: bool,
    resolution_type: u32,
}

// ---------------------------------------------------------------------------
// 바디 빌더
// ---------------------------------------------------------------------------

/// 밴드 번호로 가입 요청 바디를 만든다.
pub fn build_join_band_body(band_no: &str) -> String {
    let body = JoinBandBody {
        join_type: "band_no".to_string(),
        join_value: band_no.to_string(),
        profile_id: 1,
    };
    serde_urlencoded::to_string(&body).expect("폼 직렬화는 실패하지 않음")
}

/// 글쓰기 요청 바디를 만든다. `content`는 제목/내용을 합친 게시 본문이다.
pub fn build_create_post_body(band_no: &str, content: &str) -> String {
    let body = CreatePostBody {
        band_no: band_no.to_string(),
        content: content.to_string(),
        set_band_notice: false,
        set_major_band_notice: false,
        set_linked_band_notice: String::new(),
        copiable_state: String::new(),
        should_disable_comment: false,
        band_notice_unset_at: String::new(),
        purpose: "create".to_string(),
    };
    serde_urlencoded::to_string(&body).expect("폼 직렬화는 실패하지 않음")
}

/// 게시물(`post_no`)에 대한 댓글 요청 바디를 만든다.
pub fn build_create_comment_body(band_no: &str, post_no: u64, comment_body: &str) -> String {
    let body = CreateCommentBody {
        band_no: band_no.to_string(),
        member_type: "membership".to_string(),
        body: comment_body.to_string(),
        photos: String::new(),
        file: String::new(),
        video: String::new(),
        content_key: content_key_for_post(post_no),
        is_secret: false,
        resolution_type: 4,
    };
    serde_urlencoded::to_string(&body).expect("폼 직렬화는 실패하지 않음")
}

/// 댓글의 `content_key` JSON 문자열을 만든다.
///
/// 캡처: `{"content_type":"post","post_no":2}` (폼값으로 percent-encode되어 전송).
pub fn content_key_for_post(post_no: u64) -> String {
    format!(r#"{{"content_type":"post","post_no":{post_no}}}"#)
}

// ---------------------------------------------------------------------------
// 공통 헤더
// ---------------------------------------------------------------------------

/// band api 공통 헤더(정적 부분)를 반환한다.
///
/// `md`(요청별 서명)와 `Cookie`, `Referer`, `User-Agent`는 클라이언트가 주입하므로
/// 여기 포함하지 않는다. `akey`는 고정 상수라 포함한다.
pub fn band_api_headers() -> Vec<(String, String)> {
    vec![
        ("akey".to_string(), APP_KEY.to_string()),
        ("content-type".to_string(), FORM_CONTENT_TYPE.to_string()),
        ("device-time-zone-id".to_string(), "Asia/Seoul".to_string()),
        (
            "device-time-zone-ms-offset".to_string(),
            "32400000".to_string(),
        ),
        ("accept".to_string(), "application/json".to_string()),
        ("origin".to_string(), "https://www.band.us".to_string()),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn header<'a>(h: &'a [(String, String)], name: &str) -> Option<&'a str> {
        h.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
    }

    // ------------------------------------------------------------------
    // 바디: 캡처 실측 문자열과 일치
    // ------------------------------------------------------------------

    #[test]
    fn join_band_body_matches_capture() {
        // 캡처: join_type=band_no&join_value=103043410&profile_id=1
        assert_eq!(
            build_join_band_body("103043410"),
            "join_type=band_no&join_value=103043410&profile_id=1"
        );
    }

    #[test]
    fn create_post_body_matches_capture() {
        // 캡처 본문: "test\npstmacro test" → content=test%0Apstmacro+test
        let body = build_create_post_body("103043410", "test\npstmacro test");
        assert_eq!(
            body,
            "band_no=103043410&content=test%0Apstmacro+test\
             &set_band_notice=false&set_major_band_notice=false\
             &set_linked_band_notice=&copiable_state=&should_disable_comment=false\
             &band_notice_unset_at=&purpose=create"
        );
    }

    #[test]
    fn create_comment_body_matches_capture() {
        // 캡처: body=pstmacro 댓글작성, content_key={"content_type":"post","post_no":2}
        let body = build_create_comment_body("103043410", 2, "pstmacro 댓글작성");
        assert_eq!(
            body,
            "band_no=103043410&member_type=membership\
             &body=pstmacro+%EB%8C%93%EA%B8%80%EC%9E%91%EC%84%B1\
             &photos=&file=&video=\
             &content_key=%7B%22content_type%22%3A%22post%22%2C%22post_no%22%3A2%7D\
             &is_secret=false&resolution_type=4"
        );
    }

    #[test]
    fn content_key_for_post_is_compact_json() {
        assert_eq!(
            content_key_for_post(2),
            r#"{"content_type":"post","post_no":2}"#
        );
    }

    #[test]
    fn create_post_body_percent_encodes_newline_and_space() {
        let body = build_create_post_body("1", "제목\n내용 입니다");
        assert!(body.contains("content=%EC%A0%9C%EB%AA%A9%0A"));
        assert!(body.contains('+'), "공백은 +로 인코딩되어야 함");
    }

    // ------------------------------------------------------------------
    // 헤더
    // ------------------------------------------------------------------

    #[test]
    fn headers_include_fixed_akey() {
        let h = band_api_headers();
        assert_eq!(header(&h, "akey"), Some("bbc59b0b5f7a1c6efe950f6236ccda35"));
    }

    #[test]
    fn headers_content_type_is_form_urlencoded() {
        let h = band_api_headers();
        assert_eq!(
            header(&h, "content-type"),
            Some("application/x-www-form-urlencoded; charset=UTF-8")
        );
    }

    #[test]
    fn headers_include_device_time_zone() {
        let h = band_api_headers();
        assert_eq!(header(&h, "device-time-zone-id"), Some("Asia/Seoul"));
        assert_eq!(header(&h, "device-time-zone-ms-offset"), Some("32400000"));
    }

    #[test]
    fn headers_do_not_include_md_or_cookie() {
        // md/Cookie는 클라이언트가 주입하므로 정적 헤더에 없어야 함.
        let h = band_api_headers();
        assert!(header(&h, "md").is_none());
        assert!(header(&h, "Cookie").is_none());
        assert!(header(&h, "cookie").is_none());
    }
}
