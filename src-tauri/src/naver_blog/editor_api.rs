//! 네이버 블로그 편집기 보조 API 클라이언트(oglink·장소·스티커·업로드).
//!
//! 편집기 툴바의 삽입 블록(링크/장소/스티커/사진/파일)은 발행 전에 `platform.editor.naver.com`의
//! 보조 API로 메타데이터/업로드 참조를 먼저 얻어야 documentModel 컴포넌트를 채울 수 있다. 저장된
//! 네이버 세션 쿠키를 그대로 실어(`naver_report`·댓글 클라이언트와 동일 규약) 호출한다. 파싱은
//! 순수 함수로 분리해 wiremock 없이도 스펙 JSON과 맞춤을 검증한다.
//!
//! # 실기기 튜닝
//! 업로드(multipart) 필드명·staticmap 쿼리 파라미터 일부는 실제 편집기 트래픽으로 재검증이
//! 필요해 `// TODO(실기기 튜닝)`로 표시했다. 파싱/조립/컴파일과 GET 계열은 스펙대로 동작한다.

use serde::Serialize;
use serde_json::Value;

use super::error::BlogError;

/// 편집기 보조 API 호스트.
const EDITOR_HOST: &str = "https://platform.editor.naver.com";
/// 글쓰기 폼 옵션(se-authorization 토큰 발급) 호스트.
const BLOG_HOST: &str = "https://blog.naver.com";
/// 사진 업로드 결과가 올라가는 도메인.
const BLOGFILES_DOMAIN: &str = "https://blogfiles.pstatic.net";

/// 편집기 보조 API 인증 세션(글쓰기 페이지에서 발급). `platform.editor.naver.com` API는 쿠키만으론
/// 401("the token must not be empty")을 돌려주고, 아래 두 헤더가 있어야 통과한다.
///
/// - `se_authorization`: `PostWriteFormSeOptions.naver` 응답 `result.token`(HS256 JWT). 실측 확정.
/// - `se_app_id`: 에디터가 세션마다 만드는 `SE-<uuid>` 클라이언트 생성값. 우리도 세션당 1개를 만들어
///   그 세션의 모든 호출에서 재사용한다.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditorSession {
    /// `se-authorization` 헤더값(JWT). 자격 증명이므로 에러 메시지에 노출하지 않는다.
    pub se_authorization: String,
    /// `se-app-id` 헤더값(`SE-<uuid>`, 세션당 고정 재사용).
    pub se_app_id: String,
}

/// oglink API가 준 링크 메타데이터(컴포넌트 `oglink`용).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OglinkMeta {
    pub title: String,
    pub domain: String,
    pub description: String,
    pub thumbnail_src: String,
    pub thumbnail_width: u32,
    pub thumbnail_height: u32,
    pub oglink_sign: String,
}

/// 장소 검색 결과 1건(컴포넌트 `placesMap`의 place 원소용).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaceResult {
    pub id: String,
    pub name: String,
    pub tel: String,
    pub road_address: String,
    pub address: String,
    /// 경도(x).
    pub x: String,
    /// 위도(y).
    pub y: String,
    /// place.type(예: "s").
    pub place_type: String,
    pub thum_url: String,
}

/// staticmap 결과(지도 썸네일 URL).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StaticMapResult {
    pub src: String,
}

/// 스티커 팩 1개(목록용).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StickerPack {
    pub pack_code: String,
    pub sticker_count: u32,
    pub is_free: bool,
}

/// 파일 업로드 결과(컴포넌트 `file`용).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UploadedFile {
    pub file_id: String,
    pub file_name: String,
    pub file_size: u64,
}

/// 사진 업로드 결과(컴포넌트 `image`용).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UploadedImage {
    pub src: String,
    pub path: String,
    pub domain: String,
    pub file_size: u64,
    pub width: u32,
    pub height: u32,
    pub original_width: u32,
    pub original_height: u32,
    pub file_name: String,
}

/// 네이버 블로그 편집기 보조 API HTTP 클라이언트. base_url을 분리 보관해 실서버/wiremock을 함께 쓴다.
pub struct BlogEditorApiClient {
    base: String,
    http: reqwest::Client,
    /// 편집기 인증 세션(se-authorization/se-app-id). 없으면 401이 나므로 [`Self::with_session`]로 실어야 한다.
    session: Option<EditorSession>,
}

impl Default for BlogEditorApiClient {
    fn default() -> Self {
        Self::new()
    }
}

impl BlogEditorApiClient {
    /// 실서버 호스트를 사용하는 클라이언트를 생성한다.
    pub fn new() -> Self {
        Self::with_base_url(EDITOR_HOST)
    }

    /// 주입된 base_url을 사용하는 클라이언트를 생성한다(테스트용).
    pub fn with_base_url(base: impl Into<String>) -> Self {
        Self {
            base: base.into(),
            http: crate::naver_cafe::shared_http_client(),
            session: None,
        }
    }

    /// 편집기 인증 세션(se-authorization/se-app-id)을 실어 이후 모든 API 호출에 헤더로 붙인다.
    pub fn with_session(mut self, session: EditorSession) -> Self {
        self.session = Some(session);
        self
    }

    /// 세션이 있으면 편집기 인증 헤더(se-authorization/se-app-id + Origin)를 요청에 붙인다.
    fn apply_session_headers(&self, mut req: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        if let Some(s) = &self.session {
            req = req
                .header("se-authorization", s.se_authorization.as_str())
                .header("se-app-id", s.se_app_id.as_str())
                .header("Origin", BLOG_HOST);
        }
        req
    }

    /// 공용 GET — 위장 헤더(same-origin XHR) + 편집기 인증 헤더 + 저장 쿠키를 싣고 본문 텍스트를 돌려준다.
    async fn get_text(&self, url: &str, cookie: Option<&str>) -> Result<String, BlogError> {
        let mut req = self
            .http
            .get(url)
            .header("User-Agent", crate::naver_cafe::post::BROWSER_USER_AGENT)
            .header("Accept", "application/json, text/plain, */*")
            .header("Referer", "https://blog.naver.com/")
            .header("sec-fetch-site", "same-site")
            .header("sec-fetch-mode", "cors")
            .header("sec-fetch-dest", "empty");
        req = self.apply_session_headers(req);
        if let Some(c) = cookie {
            req = req.header("Cookie", c);
        }
        let resp = req
            .send()
            .await
            .map_err(|e| BlogError::new(format!("편집기 보조 API 요청 실패: {e}")))?;
        let status = resp.status();
        let text = resp
            .text()
            .await
            .map_err(|e| BlogError::new(format!("편집기 보조 API 응답 읽기 실패: {e}")))?;
        tracing::info!(
            "[BLOG] 편집기 보조 API 응답 — status={} body={}",
            status.as_u16(),
            snippet(&text)
        );
        Ok(text)
    }

    /// 링크(oglink) 메타데이터 조회.
    ///
    /// # 쿠키 보안
    /// `cookie`는 사용자 인증 자격 증명이며 에러/로그에 노출하지 않는다.
    pub async fn oglink(&self, url: &str, cookie: Option<&str>) -> Result<OglinkMeta, BlogError> {
        let api = format!("{}/api/blogpc001/v1/oglink", self.base);
        let text = self
            .get_text(
                &reqwest::Url::parse_with_params(&api, &[("url", url)])
                    .map(|u| u.to_string())
                    .unwrap_or(api),
                cookie,
            )
            .await?;
        parse_oglink(&text)
            .ok_or_else(|| BlogError::new(format!("링크 메타 해석 실패: {}", snippet(&text))))
    }

    /// 장소 검색(displayCount=7 고정).
    ///
    /// # 쿠키 보안
    /// `cookie`는 사용자 인증 자격 증명이며 에러/로그에 노출하지 않는다.
    pub async fn places(
        &self,
        query: &str,
        cookie: Option<&str>,
    ) -> Result<Vec<PlaceResult>, BlogError> {
        let api = format!("{}/api/blogpc001/v1/map/naver/places", self.base);
        let url = reqwest::Url::parse_with_params(
            &api,
            &[
                ("query", query),
                ("siteSort", "0"),
                ("displayCount", "7"),
                ("page", "1"),
            ],
        )
        .map(|u| u.to_string())
        .unwrap_or(api);
        let text = self.get_text(&url, cookie).await?;
        Ok(parse_places(&text))
    }

    /// 장소 좌표(위도,경도)로 정적 지도 이미지 URL을 얻는다.
    ///
    /// # 쿠키 보안
    /// `cookie`는 사용자 인증 자격 증명이며 에러/로그에 노출하지 않는다.
    pub async fn staticmap(
        &self,
        latitude: &str,
        longitude: &str,
        cookie: Option<&str>,
    ) -> Result<StaticMapResult, BlogError> {
        let api = format!("{}/api/blogpc001/v2/map/naver/staticmap", self.base);
        // TODO(실기기 튜닝): markers 외 width/height/level 등 파라미터를 실제 편집기 트래픽으로 확정.
        let url = reqwest::Url::parse_with_params(
            &api,
            &[("markers", format!("{latitude},{longitude}").as_str())],
        )
        .map(|u| u.to_string())
        .unwrap_or(api);
        let text = self.get_text(&url, cookie).await?;
        parse_staticmap(&text)
            .ok_or_else(|| BlogError::new(format!("정적 지도 해석 실패: {}", snippet(&text))))
    }

    /// 스티커 팩 목록.
    ///
    /// # 쿠키 보안
    /// `cookie`는 사용자 인증 자격 증명이며 에러/로그에 노출하지 않는다.
    pub async fn stickers(&self, cookie: Option<&str>) -> Result<Vec<StickerPack>, BlogError> {
        let url = format!("{}/api/blogpc001/v1/stickers", self.base);
        let text = self.get_text(&url, cookie).await?;
        Ok(parse_sticker_packs(&text))
    }

    /// 스티커 팩 내 seq 목록.
    ///
    /// # 쿠키 보안
    /// `cookie`는 사용자 인증 자격 증명이며 에러/로그에 노출하지 않는다.
    pub async fn sticker_pack_seqs(
        &self,
        pack_code: &str,
        cookie: Option<&str>,
    ) -> Result<Vec<u32>, BlogError> {
        let url = format!("{}/api/blogpc001/v1/stickers/{pack_code}", self.base);
        let text = self.get_text(&url, cookie).await?;
        Ok(parse_sticker_seqs(&text))
    }

    /// 사진 업로더 세션키 조회.
    ///
    /// # 쿠키 보안
    /// `cookie`는 사용자 인증 자격 증명이며 에러/로그에 노출하지 않는다.
    pub async fn photo_session_key(&self, cookie: Option<&str>) -> Result<String, BlogError> {
        let url = format!("{}/api/blogpc001/v1/photo-uploader/session-key", self.base);
        let text = self.get_text(&url, cookie).await?;
        parse_session_key(&text)
            .ok_or_else(|| BlogError::new(format!("세션키 해석 실패: {}", snippet(&text))))
    }

    /// 로컬 파일을 업로드하고 `{fileId, fileName, fileSize}`를 얻는다(파일 블록용).
    ///
    /// # 쿠키 보안
    /// `cookie`는 사용자 인증 자격 증명이며 에러/로그에 노출하지 않는다.
    pub async fn upload_file(
        &self,
        file_name: &str,
        bytes: Vec<u8>,
        cookie: Option<&str>,
    ) -> Result<UploadedFile, BlogError> {
        let url = format!("{}/api/blogpc001/v2/upload/file", self.base);
        // TODO(실기기 튜닝): multipart 파트 이름("file")과 부가 필드를 실제 편집기 트래픽으로 확정.
        let part = reqwest::multipart::Part::bytes(bytes)
            .file_name(file_name.to_owned())
            .mime_str("application/octet-stream")
            .map_err(|e| BlogError::new(format!("업로드 파트 생성 실패: {e}")))?;
        let form = reqwest::multipart::Form::new().part("file", part);
        let mut req = self
            .http
            .post(&url)
            .header("User-Agent", crate::naver_cafe::post::BROWSER_USER_AGENT)
            .header("Accept", "application/json, text/plain, */*")
            .header("Referer", "https://blog.naver.com/");
        req = self.apply_session_headers(req);
        if let Some(c) = cookie {
            req = req.header("Cookie", c);
        }
        let resp = req
            .multipart(form)
            .send()
            .await
            .map_err(|e| BlogError::new(format!("파일 업로드 요청 실패: {e}")))?;
        let status = resp.status();
        let text = resp
            .text()
            .await
            .map_err(|e| BlogError::new(format!("파일 업로드 응답 읽기 실패: {e}")))?;
        tracing::info!(
            "[BLOG] 파일 업로드 응답 — status={} body={}",
            status.as_u16(),
            snippet(&text)
        );
        parse_uploaded_file(&text)
            .ok_or_else(|| BlogError::new(format!("파일 업로드 응답 해석 실패: {}", snippet(&text))))
    }

    /// 로컬 이미지를 세션키로 업로드하고 image 컴포넌트에 필요한 값을 얻는다(사진 블록용).
    ///
    /// # 쿠키 보안
    /// `cookie`는 사용자 인증 자격 증명이며 에러/로그에 노출하지 않는다.
    pub async fn upload_photo(
        &self,
        session_key: &str,
        file_name: &str,
        bytes: Vec<u8>,
        cookie: Option<&str>,
    ) -> Result<UploadedImage, BlogError> {
        // TODO(실기기 튜닝): 사진 업로드 엔드포인트/파트 이름/응답 필드를 실제 편집기 트래픽으로 확정.
        let url = format!(
            "{}/api/blogpc001/v1/photo-uploader/upload?sessionKey={session_key}",
            self.base
        );
        let part = reqwest::multipart::Part::bytes(bytes)
            .file_name(file_name.to_owned())
            .mime_str("application/octet-stream")
            .map_err(|e| BlogError::new(format!("업로드 파트 생성 실패: {e}")))?;
        let form = reqwest::multipart::Form::new().part("image", part);
        let mut req = self
            .http
            .post(&url)
            .header("User-Agent", crate::naver_cafe::post::BROWSER_USER_AGENT)
            .header("Accept", "application/json, text/plain, */*")
            .header("Referer", "https://blog.naver.com/");
        req = self.apply_session_headers(req);
        if let Some(c) = cookie {
            req = req.header("Cookie", c);
        }
        let resp = req
            .multipart(form)
            .send()
            .await
            .map_err(|e| BlogError::new(format!("사진 업로드 요청 실패: {e}")))?;
        let status = resp.status();
        let text = resp
            .text()
            .await
            .map_err(|e| BlogError::new(format!("사진 업로드 응답 읽기 실패: {e}")))?;
        tracing::info!(
            "[BLOG] 사진 업로드 응답 — status={} body={}",
            status.as_u16(),
            snippet(&text)
        );
        parse_uploaded_image(&text)
            .ok_or_else(|| BlogError::new(format!("사진 업로드 응답 해석 실패: {}", snippet(&text))))
    }
}

/// 편집기 보조 API 인증 세션(se-authorization/se-app-id)을 발급받는다(실서버 호스트).
///
/// `GET blog.naver.com/PostWriteFormSeOptions.naver?blogId={blog_id}`(계정 쿠키)의 응답
/// `result.token`(HS256 JWT)이 `se-authorization` 헤더값이다(실측 확정). `se-app-id`는 에디터가
/// 세션마다 만드는 `SE-<uuid>` 클라이언트 생성값이라 우리도 하나 만들어 그 세션에서 재사용한다.
///
/// 토큰을 못 찾으면(블로그 없음/로그인 만료 등) 응답 원문 스니펫을 실은 `BlogError`로 알린다.
///
/// # 쿠키 보안
/// `cookie`는 사용자 인증 자격 증명이며 에러/로그에 노출하지 않는다.
pub async fn fetch_editor_session(
    blog_id: &str,
    cookie: Option<&str>,
) -> Result<EditorSession, BlogError> {
    fetch_editor_session_with_base(BLOG_HOST, blog_id, cookie).await
}

/// 주입된 `blog_base`로 [`fetch_editor_session`]을 수행한다(wiremock 테스트용).
pub async fn fetch_editor_session_with_base(
    blog_base: &str,
    blog_id: &str,
    cookie: Option<&str>,
) -> Result<EditorSession, BlogError> {
    let url = format!("{blog_base}/PostWriteFormSeOptions.naver?blogId={blog_id}");
    let http = crate::naver_cafe::shared_http_client();
    let mut req = http
        .get(&url)
        .header("User-Agent", crate::naver_cafe::post::BROWSER_USER_AGENT)
        .header("Accept", "application/json, text/plain, */*")
        .header("Referer", format!("{blog_base}/{blog_id}?Redirect=Write"))
        .header("sec-fetch-site", "same-origin")
        .header("sec-fetch-mode", "cors")
        .header("sec-fetch-dest", "empty");
    if let Some(c) = cookie {
        req = req.header("Cookie", c);
    }
    let resp = req
        .send()
        .await
        .map_err(|e| BlogError::new(format!("편집기 세션 발급 요청 실패: {e}")))?;
    let status = resp.status();
    let text = resp
        .text()
        .await
        .map_err(|e| BlogError::new(format!("편집기 세션 발급 응답 읽기 실패: {e}")))?;
    // 원문 로그(형님 지시): 토큰 위치를 사람이 확인할 수 있게 상태·본문을 필터 없이 남긴다
    // (쿠키는 본문에 없어 안전). 토큰은 우리가 실으려는 값이라 그대로 남긴다.
    tracing::info!(
        "[BLOG] PostWriteFormSeOptions 응답 — status={} body={}",
        status.as_u16(),
        text
    );
    let token = parse_se_token(&text).ok_or_else(|| {
        BlogError::new(format!(
            "편집기 세션 토큰(se-authorization)을 찾지 못했습니다(이 계정에 블로그가 없거나 로그인이 만료됐을 수 있음). status={} 응답={}",
            status.as_u16(),
            snippet(&text)
        ))
    })?;
    Ok(EditorSession {
        se_authorization: token,
        // se-app-id: SmartEditor 요소 id 생성기가 만드는 "SE-<uuid>"와 동일 형식(write_client::se_id 재사용).
        se_app_id: super::write_client::se_id(),
    })
}

/// `PostWriteFormSeOptions.naver` 응답에서 `result.token`(se-authorization JWT)을 뽑는다(순수 함수).
/// `{"isSuccess":true,"result":{"token":"<JWT>",...}}`. 빈 문자열이면 없음으로 본다.
pub fn parse_se_token(text: &str) -> Option<String> {
    let v: Value = serde_json::from_str(text).ok()?;
    v.get("result")
        .and_then(|r| r.get("token"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
}

/// oglink 응답에서 메타를 뽑는다(순수 함수). `oglink.summary.{domain,title,description,image}` + `oglinkSign`.
pub fn parse_oglink(text: &str) -> Option<OglinkMeta> {
    let v: Value = serde_json::from_str(text).ok()?;
    let oglink = v.get("oglink")?;
    let summary = oglink.get("summary")?;
    let image = summary.get("image");
    Some(OglinkMeta {
        title: str_field(summary, "title"),
        domain: str_field(summary, "domain"),
        description: str_field(summary, "description"),
        thumbnail_src: image.map(|i| str_field(i, "url")).unwrap_or_default(),
        thumbnail_width: image.and_then(|i| u32_field(i, "width")).unwrap_or(0),
        thumbnail_height: image.and_then(|i| u32_field(i, "height")).unwrap_or(0),
        oglink_sign: str_field(&v, "oglinkSign"),
    })
}

/// places 응답에서 장소 목록을 뽑는다(순수 함수). `result.place.list[]`.
pub fn parse_places(text: &str) -> Vec<PlaceResult> {
    let Ok(v) = serde_json::from_str::<Value>(text) else {
        return Vec::new();
    };
    let list = v
        .get("result")
        .and_then(|r| r.get("place"))
        .and_then(|p| p.get("list"))
        .and_then(Value::as_array);
    let Some(list) = list else {
        return Vec::new();
    };
    list.iter()
        .map(|p| PlaceResult {
            id: str_field(p, "id"),
            name: str_field(p, "name"),
            tel: str_field(p, "tel"),
            road_address: str_field(p, "roadAddress"),
            address: str_field(p, "address"),
            x: str_field(p, "x"),
            y: str_field(p, "y"),
            place_type: str_field(p, "type"),
            thum_url: str_field(p, "thumUrl"),
        })
        .collect()
}

/// staticmap 응답에서 이미지 URL을 뽑는다(순수 함수). 응답이 URL 문자열이거나 `{url|src|imageUrl}`.
pub fn parse_staticmap(text: &str) -> Option<StaticMapResult> {
    let trimmed = text.trim();
    if trimmed.starts_with("http") {
        return Some(StaticMapResult {
            src: trimmed.to_owned(),
        });
    }
    let v: Value = serde_json::from_str(trimmed).ok()?;
    for key in ["url", "src", "imageUrl"] {
        if let Some(s) = v.get(key).and_then(Value::as_str) {
            return Some(StaticMapResult { src: s.to_owned() });
        }
    }
    None
}

/// 스티커 팩 목록을 뽑는다(순수 함수). `list[]`.
pub fn parse_sticker_packs(text: &str) -> Vec<StickerPack> {
    let Ok(v) = serde_json::from_str::<Value>(text) else {
        return Vec::new();
    };
    let Some(list) = v.get("list").and_then(Value::as_array) else {
        return Vec::new();
    };
    list.iter()
        .filter_map(|p| {
            let pack_code = p.get("packCode")?.as_str()?.to_owned();
            Some(StickerPack {
                pack_code,
                sticker_count: u32_field(p, "stickerCount").unwrap_or(0),
                is_free: p.get("isFree").and_then(Value::as_bool).unwrap_or(true),
            })
        })
        .collect()
}

/// 스티커 팩 내 seq 목록을 뽑는다(순수 함수). seq 배열 또는 `{list:[{seq}]}` 모두 대응.
pub fn parse_sticker_seqs(text: &str) -> Vec<u32> {
    let Ok(v) = serde_json::from_str::<Value>(text) else {
        return Vec::new();
    };
    let arr = v
        .get("stickers")
        .or_else(|| v.get("list"))
        .and_then(Value::as_array);
    let Some(arr) = arr else {
        return Vec::new();
    };
    arr.iter()
        .filter_map(|s| {
            s.as_u64()
                .or_else(|| s.get("seq").and_then(Value::as_u64))
                .map(|n| n as u32)
        })
        .collect()
}

/// session-key 응답에서 sessionKey를 뽑는다(순수 함수). `{isSuccess, sessionKey}`.
pub fn parse_session_key(text: &str) -> Option<String> {
    let v: Value = serde_json::from_str(text).ok()?;
    v.get("sessionKey")
        .and_then(Value::as_str)
        .map(str::to_owned)
}

/// 파일 업로드 응답을 뽑는다(순수 함수). `{fileId, fileName, fileSize}`.
pub fn parse_uploaded_file(text: &str) -> Option<UploadedFile> {
    let v: Value = serde_json::from_str(text).ok()?;
    let obj = v.get("result").unwrap_or(&v);
    Some(UploadedFile {
        file_id: obj.get("fileId")?.as_str()?.to_owned(),
        file_name: str_field(obj, "fileName"),
        file_size: obj.get("fileSize").and_then(Value::as_u64).unwrap_or(0),
    })
}

/// 사진 업로드 응답을 뽑는다(순수 함수). blogfiles.pstatic.net src/path/크기.
pub fn parse_uploaded_image(text: &str) -> Option<UploadedImage> {
    let v: Value = serde_json::from_str(text).ok()?;
    let obj = v.get("result").unwrap_or(&v);
    let src = str_field(obj, "url");
    let src = if src.is_empty() {
        str_field(obj, "src")
    } else {
        src
    };
    if src.is_empty() {
        return None;
    }
    let path = obj
        .get("path")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .unwrap_or_else(|| path_from_url(&src));
    let width = u32_field(obj, "width").unwrap_or(0);
    let height = u32_field(obj, "height").unwrap_or(0);
    Some(UploadedImage {
        src,
        path,
        domain: BLOGFILES_DOMAIN.to_owned(),
        file_size: obj.get("fileSize").and_then(Value::as_u64).unwrap_or(0),
        width,
        height,
        original_width: u32_field(obj, "originalWidth").unwrap_or(width),
        original_height: u32_field(obj, "originalHeight").unwrap_or(height),
        file_name: str_field(obj, "fileName"),
    })
}

/// URL에서 도메인을 제외한 path 부분(쿼리 제거)을 뽑는다(순수 헬퍼).
fn path_from_url(url: &str) -> String {
    let no_query = url.split('?').next().unwrap_or(url);
    match no_query.split_once("//") {
        Some((_, rest)) => rest
            .find('/')
            .map(|i| rest[i..].to_owned())
            .unwrap_or_default(),
        None => String::new(),
    }
}

/// JSON 객체에서 문자열 필드를 안전하게 뽑는다(없으면 "").
fn str_field(v: &Value, key: &str) -> String {
    v.get(key)
        .and_then(Value::as_str)
        .map(str::to_owned)
        .unwrap_or_default()
}

/// JSON 객체에서 u32 필드를 안전하게 뽑는다.
fn u32_field(v: &Value, key: &str) -> Option<u32> {
    v.get(key).and_then(Value::as_u64).map(|n| n as u32)
}

/// 로그·에러용 응답 앞부분 스니펫(최대 300자).
fn snippet(text: &str) -> String {
    text.chars().take(300).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_oglink_extracts_summary_and_sign() {
        let m = parse_oglink(
            r#"{"oglink":{"summary":{"domain":"naver.com","title":"네이버","description":"검색","image":{"url":"https://img/x.png","width":300,"height":200}}},"oglinkSign":"SIGN"}"#,
        )
        .unwrap();
        assert_eq!(m.title, "네이버");
        assert_eq!(m.domain, "naver.com");
        assert_eq!(m.description, "검색");
        assert_eq!(m.thumbnail_src, "https://img/x.png");
        assert_eq!(m.thumbnail_width, 300);
        assert_eq!(m.thumbnail_height, 200);
        assert_eq!(m.oglink_sign, "SIGN");
    }

    #[test]
    fn parse_places_extracts_list() {
        let list = parse_places(
            r#"{"result":{"place":{"list":[{"id":"1621706163","name":"카페","tel":"02-1","roadAddress":"도로1","address":"지번1","x":"127.0","y":"37.5","type":"s","thumUrl":"https://t/1.png"}]}}}"#,
        );
        assert_eq!(list.len(), 1);
        let p = &list[0];
        assert_eq!(p.id, "1621706163");
        assert_eq!(p.name, "카페");
        assert_eq!(p.road_address, "도로1");
        assert_eq!(p.x, "127.0");
        assert_eq!(p.y, "37.5");
        assert_eq!(p.place_type, "s");
    }

    #[test]
    fn parse_places_empty_on_bad_json() {
        assert!(parse_places("<html>").is_empty());
        assert!(parse_places(r#"{"result":{}}"#).is_empty());
    }

    #[test]
    fn parse_staticmap_url_string_or_object() {
        assert_eq!(
            parse_staticmap("https://map/static.png").unwrap().src,
            "https://map/static.png"
        );
        assert_eq!(
            parse_staticmap(r#"{"url":"https://map/o.png"}"#).unwrap().src,
            "https://map/o.png"
        );
        assert!(parse_staticmap("nope").is_none());
    }

    #[test]
    fn parse_sticker_packs_extracts_list() {
        let packs = parse_sticker_packs(
            r#"{"list":[{"packCode":"motion2d_01","stickerCount":24,"isFree":true}]}"#,
        );
        assert_eq!(packs.len(), 1);
        assert_eq!(packs[0].pack_code, "motion2d_01");
        assert_eq!(packs[0].sticker_count, 24);
        assert!(packs[0].is_free);
    }

    #[test]
    fn parse_sticker_seqs_supports_both_shapes() {
        assert_eq!(parse_sticker_seqs(r#"{"stickers":[1,2,10]}"#), vec![1, 2, 10]);
        assert_eq!(
            parse_sticker_seqs(r#"{"list":[{"seq":3},{"seq":7}]}"#),
            vec![3, 7]
        );
    }

    #[test]
    fn parse_session_key_extracts() {
        assert_eq!(
            parse_session_key(r#"{"isSuccess":true,"sessionKey":"SK123"}"#).unwrap(),
            "SK123"
        );
        assert!(parse_session_key(r#"{"isSuccess":false}"#).is_none());
    }

    #[test]
    fn parse_uploaded_file_extracts() {
        let f = parse_uploaded_file(r#"{"fileId":"F1","fileName":"a.pdf","fileSize":2048}"#).unwrap();
        assert_eq!(f.file_id, "F1");
        assert_eq!(f.file_name, "a.pdf");
        assert_eq!(f.file_size, 2048);
        // result 래핑도 해석한다.
        let f2 =
            parse_uploaded_file(r#"{"result":{"fileId":"F2","fileName":"b","fileSize":1}}"#).unwrap();
        assert_eq!(f2.file_id, "F2");
    }

    #[test]
    fn parse_uploaded_image_extracts_and_derives_path() {
        let img = parse_uploaded_image(
            r#"{"url":"https://blogfiles.pstatic.net/a/b/x.png?type=w1","fileSize":1000,"width":600,"height":400,"originalWidth":1200,"originalHeight":800,"fileName":"x.png"}"#,
        )
        .unwrap();
        assert_eq!(img.src, "https://blogfiles.pstatic.net/a/b/x.png?type=w1");
        assert_eq!(img.path, "/a/b/x.png");
        assert_eq!(img.domain, "https://blogfiles.pstatic.net");
        assert_eq!(img.width, 600);
        assert_eq!(img.original_width, 1200);
        assert_eq!(img.file_name, "x.png");
    }

    #[test]
    fn parse_uploaded_image_none_without_src() {
        assert!(parse_uploaded_image(r#"{"fileSize":1}"#).is_none());
    }

    #[tokio::test]
    async fn oglink_calls_editor_api() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/blogpc001/v1/oglink"))
            .respond_with(ResponseTemplate::new(200).set_body_string(
                r#"{"oglink":{"summary":{"domain":"d","title":"t","description":"x","image":{"url":"u","width":1,"height":2}}},"oglinkSign":"S"}"#,
            ))
            .mount(&server)
            .await;
        let client = BlogEditorApiClient::with_base_url(server.uri());
        let m = client.oglink("https://x", Some("NID_SES=abc")).await.unwrap();
        assert_eq!(m.title, "t");
        assert_eq!(m.oglink_sign, "S");
    }

    #[test]
    fn parse_se_token_extracts_result_token() {
        let jwt = "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiJibG9ncGMwMDEifQ.sig";
        let body = format!(
            r#"{{"isSuccess":true,"result":{{"appCode":"blogpc001","token":"{jwt}","documentModel":null}}}}"#
        );
        assert_eq!(parse_se_token(&body).as_deref(), Some(jwt));
    }

    #[test]
    fn parse_se_token_none_when_missing_or_empty() {
        assert!(parse_se_token(r#"{"isSuccess":false}"#).is_none());
        assert!(parse_se_token(r#"{"result":{"token":""}}"#).is_none());
        assert!(parse_se_token("<html>bot</html>").is_none());
    }

    #[tokio::test]
    async fn fetch_editor_session_reads_token_and_generates_app_id() {
        use wiremock::matchers::{method, path, query_param};
        use wiremock::{Mock, MockServer, ResponseTemplate};
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/PostWriteFormSeOptions.naver"))
            .and(query_param("blogId", "myblog"))
            .respond_with(ResponseTemplate::new(200).set_body_string(
                r#"{"isSuccess":true,"result":{"appCode":"blogpc001","token":"JWT-TOKEN-XYZ"}}"#,
            ))
            .mount(&server)
            .await;
        let s = fetch_editor_session_with_base(&server.uri(), "myblog", Some("NID_SES=abc"))
            .await
            .unwrap();
        assert_eq!(s.se_authorization, "JWT-TOKEN-XYZ");
        assert!(s.se_app_id.starts_with("SE-"), "se-app-id는 SE-<uuid> 형식");
    }

    #[tokio::test]
    async fn fetch_editor_session_errors_when_no_token() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/PostWriteFormSeOptions.naver"))
            .respond_with(
                ResponseTemplate::new(200).set_body_string(r#"{"isSuccess":false}"#),
            )
            .mount(&server)
            .await;
        assert!(
            fetch_editor_session_with_base(&server.uri(), "noblog", None)
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn oglink_sends_session_headers() {
        use wiremock::matchers::{header, method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/blogpc001/v1/oglink"))
            .and(header("se-authorization", "JWT-XYZ"))
            .and(header("se-app-id", "SE-abc"))
            .respond_with(ResponseTemplate::new(200).set_body_string(
                r#"{"oglink":{"summary":{"domain":"d","title":"t","description":"x","image":{"url":"u","width":1,"height":2}}},"oglinkSign":"S"}"#,
            ))
            .mount(&server)
            .await;
        let client = BlogEditorApiClient::with_base_url(server.uri()).with_session(EditorSession {
            se_authorization: "JWT-XYZ".to_string(),
            se_app_id: "SE-abc".to_string(),
        });
        // 세션 헤더가 매칭돼야만 200이 온다(없으면 mock 미스로 실패).
        let m = client.oglink("https://x", Some("NID_SES=abc")).await.unwrap();
        assert_eq!(m.title, "t");
    }
}
