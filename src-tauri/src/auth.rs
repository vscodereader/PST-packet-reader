use aes::cipher::{block_padding::Pkcs7, BlockEncryptMut, KeyIvInit};
use base64::{Engine, engine::general_purpose::STANDARD};
use p256::{ecdh::EphemeralSecret, elliptic_curve::sec1::ToEncodedPoint, PublicKey};
use rand::{Rng, RngCore, rngs::OsRng};
use reqwest::header::{self, HeaderMap, HeaderValue};
use sha2::{Digest, Sha256};

type Aes256CbcEnc = cbc::Encryptor<aes::Aes256>;

const LOGIN_FORM_URL: &str =
    "https://nid.naver.com/nidlogin.login?mode=form&url=https://www.naver.com/";
const DYNAMIC_EC_KEY_BASE: &str = "https://nid.naver.com/login/dynamicEcKey/";
const LOGIN_URL: &str = "https://nid.naver.com/nidlogin.login";
const FINALIZE_URL: &str =
    "https://nid.naver.com/signin/v3/finalize?url=https%3A%2F%2Fwww.naver.com%2F&svctype=1";
const GET_PROFILE_BASE: &str = "https://static.nid.naver.com/getProfile";
const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) \
    AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36";
const CAPTCHA_TOKEN_BASE: &str = "https://ncpt.naver.com/v2/tokens";
const HTTP_TIMEOUT_SECS: u64 = 15;

#[derive(Debug, thiserror::Error)]
pub enum AuthError {
    #[error("reqwest: {0}")]
    Http(#[from] reqwest::Error),
    #[error("crypto: {0}")]
    Crypto(String),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("invalid jsonp")]
    InvalidJsonp,
    #[error("form parse: required field not found in login page html")]
    FormParse,
    #[error("invalid key format")]
    InvalidKeyFormat,
    #[error("login failed: {0}")]
    LoginFailed(String),
}

struct FormData {
    wtoken: String,
    dynamic_key: String,
    sitekey: Option<String>,
}

struct EcKeyData {
    session_key: String,
    server_public_key: PublicKey,
}

/// 로그인 폼 HTML에서 `<input name="{name}" ... value="{value}">` 값을 추출.
/// name-value 속성 순서 무관하게 동작.
fn find_input_value(html: &str, name: &str) -> Option<String> {
    let name_attr = format!("name=\"{}\"", name);
    let name_pos = html.find(&name_attr)?;
    let tag_start = html[..name_pos].rfind('<')?;
    let tag_end = tag_start + html[tag_start..].find('>')?;
    let tag = &html[tag_start..=tag_end];

    let val_start = tag.find("value=\"")? + "value=\"".len();
    let val_end = val_start + tag[val_start..].find('"')?;
    Some(tag[val_start..val_end].to_string())
}

/// ncaptcha-api.js script src에서 `ncaptcha-sitekey` 쿼리 파라미터를 추출.
fn extract_sitekey(html: &str) -> Option<String> {
    let marker = "ncaptcha-sitekey=";
    let start = html.find(marker)? + marker.len();
    let rest = &html[start..];
    let end = rest.find(|c| c == '"' || c == '&' || c == ' ').unwrap_or(rest.len());
    let key = rest[..end].trim().to_string();
    if key.is_empty() { None } else { Some(key) }
}

fn parse_form_data(html: &str) -> Result<FormData, AuthError> {
    let wtoken = find_input_value(html, "wtoken").ok_or(AuthError::FormParse)?;
    let dynamic_key = find_input_value(html, "dynamicKey").ok_or(AuthError::FormParse)?;
    let sitekey = extract_sitekey(html);
    Ok(FormData { wtoken, dynamic_key, sitekey })
}

/// dynamicEcKey 응답 형식: `{sessionKey},{04}{X_hex}{Y_hex}`
/// 예: `oV83RCXf...,042279424b...384e54d4...`
fn parse_ec_key(response: &str) -> Result<EcKeyData, AuthError> {
    let s = response.trim();
    let comma = s.find(',').ok_or(AuthError::InvalidKeyFormat)?;
    let session_key = s[..comma].to_string();
    let key_hex = &s[comma + 1..];
    let key_bytes = hex::decode(key_hex).map_err(|_| AuthError::InvalidKeyFormat)?;
    let server_public_key = PublicKey::from_sec1_bytes(&key_bytes)
        .map_err(|e| AuthError::Crypto(e.to_string()))?;
    Ok(EcKeyData { session_key, server_public_key })
}

/// ECIES: 임시 P-256 키쌍 생성 → ECDH → SHA-256 KDF → AES-256-CBC 암호화.
/// 반환 형식(eccpw): `{ct_b64},{iv_b64},{ephemeral_pubkey_hex},{session_key}`
/// 평문 형식 "id\npw\nsession_key": 64바이트 암호문 크기로 역산 (캡처 수치 근거).
fn encrypt_ecies(
    server_public_key: &PublicKey,
    session_key: &str,
    id: &str,
    pw: &str,
) -> Result<String, AuthError> {
    let ephemeral_secret = EphemeralSecret::random(&mut OsRng);
    let ephemeral_public = PublicKey::from(&ephemeral_secret);
    let shared_secret = ephemeral_secret.diffie_hellman(server_public_key);
    let aes_key = Sha256::digest(shared_secret.raw_secret_bytes());

    let mut iv = [0u8; 16];
    OsRng.fill_bytes(&mut iv);

    let plaintext = format!("{}\n{}\n{}", id, pw, session_key);
    let ciphertext = Aes256CbcEnc::new_from_slices(aes_key.as_slice(), &iv)
        .map_err(|e| AuthError::Crypto(e.to_string()))?
        .encrypt_padded_vec_mut::<Pkcs7>(plaintext.as_bytes());

    let part0 = STANDARD.encode(&ciphertext);
    let part1 = STANDARD.encode(iv);
    // 04 prefix (uncompressed) || X(32B) || Y(32B) → 65바이트 hex
    let part2 = hex::encode(ephemeral_public.to_encoded_point(false).as_bytes());

    Ok(format!("{},{},{},{}", part0, part1, part2, session_key))
}

async fn fetch_form_data(client: &reqwest::Client, url: &str) -> Result<FormData, AuthError> {
    let html = client.get(url).send().await?.text().await?;
    parse_form_data(&html)
}

async fn fetch_ec_public_key(
    client: &reqwest::Client,
    base_url: &str,
    dynamic_key: &str,
) -> Result<EcKeyData, AuthError> {
    let url = format!("{}{}", base_url, dynamic_key);
    let response = client.get(&url).send().await?.text().await?;
    parse_ec_key(&response)
}

/// 11자 영소문자+숫자 랜덤 tid 생성 (ncaptcha-api.js가 JS에서 생성하는 형식).
fn generate_tid() -> String {
    const CHARSET: &[u8] = b"abcdefghijklmnopqrstuvwxyz0123456789";
    let mut rng = rand::thread_rng();
    (0..11).map(|_| CHARSET[rng.gen_range(0..CHARSET.len())] as char).collect()
}

/// CAPTCHA 토큰을 best-effort로 발급. 실패 시 None 반환 (에러 전파 없음 — fallback 흐름).
/// cipherText는 JS 없이 재현 불가하므로 빈 값으로 시도; 서버가 bvsd를 미강제하면 tokenId 반환.
async fn fetch_captcha_token(
    client: &reqwest::Client,
    sitekey: &str,
    base_url: &str,
) -> Option<String> {
    let tid = generate_tid();
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let url = format!("{}?q={}&tid={}", base_url, timestamp, tid);

    let body = serde_json::json!({
        "cipherText": "",
        "siteKey": sitekey,
        "t": "1|3|1|251|8",
        "referer": LOGIN_FORM_URL,
        "origin": "https://nid.naver.com"
    })
    .to_string();

    let resp = client
        .post(&url)
        .header("Content-Type", "text/plain")
        .header("Referer", LOGIN_FORM_URL)
        .header("Origin", "https://nid.naver.com")
        .body(body)
        .send()
        .await
        .ok()?;

    let json: serde_json::Value = resp.json().await.ok()?;
    json.get("tokenId")?.as_str().map(|s| s.to_string())
}

/// 네이버 로그인 실패 HTML에서 오류 메시지 추출.
fn extract_login_error(html: &str) -> Option<String> {
    for marker in &["class=\"error_message\">", "class='error_message'>"] {
        if let Some(start) = html.find(marker) {
            let rest = &html[start + marker.len()..];
            if let Some(end) = rest.find('<') {
                let msg = rest[..end].trim().to_string();
                if !msg.is_empty() {
                    return Some(msg);
                }
            }
        }
    }
    None
}

async fn submit_login(
    client: &reqwest::Client,
    url: &str,
    eccpw: &str,
    dynamic_key: &str,
    wtoken: &str,
    bvsd: &str,
) -> Result<(), AuthError> {
    let resp = client
        .post(url)
        .header("Referer", LOGIN_FORM_URL)
        .header("Origin", "https://nid.naver.com")
        .form(&[
            ("dynamicKey", dynamic_key),
            ("eccpw", eccpw),
            ("enctp", "1"),
            ("wtoken", wtoken),
            ("svctype", "1"),
            ("template_type", "V2_DESKTOP_DEFAULT"),
            ("smart_LEVEL", "1"),
            ("locale", "ko_KR"),
            ("url", "https://www.naver.com/"),
            ("id", ""),
            ("pw", ""),
            ("localechange", ""),
            ("next_step", "true"),
            ("show_pk", "true"),
            ("bvsd", bvsd),
        ])
        .send()
        .await?;

    // 패킷 분석 기준: 네이버는 로그인 성공/실패 모두 HTTP 200으로 응답.
    // 실패 시 body에 error_message 클래스 또는 로그인 폼 재반환.
    let status = resp.status();
    let body = resp.text().await.unwrap_or_default();

    if std::env::var("NAVER_AUTH_DEBUG").is_ok() {
        eprintln!(
            "[auth] submit_login: HTTP {} / body {} bytes\n--- body head ---\n{}\n---",
            status,
            body.len(),
            &body[..body.len().min(800)]
        );
    }

    if !status.is_success() {
        return Err(AuthError::LoginFailed(format!("로그인 POST HTTP {}", status)));
    }
    if let Some(msg) = extract_login_error(&body) {
        return Err(AuthError::LoginFailed(msg));
    }
    // 네이버 비정상 네트워크 감지 경고 페이지 (datacenter/VPN IP 차단).
    if body.contains("class=\"warning\"") || body.contains("ico_warning") {
        return Err(AuthError::LoginFailed(
            "네트워크 차단 — 네이버가 비정상 네트워크로 감지함 (datacenter/VPN IP)".to_string(),
        ));
    }
    // 서버가 로그인 폼을 다시 반환하면 암호화 포맷 불일치 또는 CAPTCHA 차단.
    if body.contains("name=\"dynamicKey\"") || body.contains("name='dynamicKey'") {
        return Err(AuthError::LoginFailed(
            "로그인 폼 재반환 — eccpw 포맷 오류 또는 CAPTCHA 차단 가능성".to_string(),
        ));
    }
    Ok(())
}

async fn finalize_session(client: &reqwest::Client, url: &str) -> Result<(), AuthError> {
    client
        .get(url)
        .header("Referer", LOGIN_URL)
        .send()
        .await?
        .error_for_status()?;
    Ok(())
}

/// getProfile JSONP 응답을 언래핑해 JSON으로 파싱. rtn_cd가 "0"이 아니면 에러.
/// 응답 형식: `{callback}({"rtn_cd":"0","rtn_msg":"Success",...})` 또는 끝에 `;` 포함 가능.
fn parse_jsonp_profile(
    callback: &str,
    text: &str,
) -> Result<serde_json::Value, AuthError> {
    let prefix = format!("{}(", callback);
    // 표준 JSONP는 세미콜론으로 끝날 수 있음: "cb({...});"
    let trimmed = text.trim().trim_end_matches(';');
    let json_str = trimmed
        .strip_prefix(&prefix)
        .and_then(|s| s.strip_suffix(')'))
        .ok_or_else(|| {
            AuthError::LoginFailed(format!(
                "JSONP 파싱 실패: {}",
                &text[..text.len().min(200)]
            ))
        })?;

    let v: serde_json::Value = serde_json::from_str(json_str)?;

    if v.get("rtn_cd").and_then(|v| v.as_str()) != Some("0") {
        return Err(AuthError::LoginFailed(format!(
            "로그인 세션 없음 — 응답: {}",
            json_str
        )));
    }

    Ok(v)
}

/// finalize 후 getProfile JSONP로 실제 세션 유효성을 확인하고 사용자 프로필을 반환.
/// 패킷 분석 기준: GET static.nid.naver.com/getProfile?svc=my&callback=jsonp_...
async fn verify_session(
    client: &reqwest::Client,
    base_url: &str,
) -> Result<serde_json::Value, AuthError> {
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let callback = format!("jsonp_{}_{}", timestamp, rand::random::<u32>());
    let url = format!("{}?svc=my&callback={}", base_url, callback);

    // 브라우저는 www.naver.com 리다이렉트 후 getProfile을 호출하므로 Referer 필요.
    let resp = client
        .get(&url)
        .header("Referer", "https://www.naver.com/")
        .send()
        .await?;

    let status = resp.status();
    let text = resp.text().await.unwrap_or_default();

    if !status.is_success() {
        return Err(AuthError::LoginFailed(format!(
            "getProfile HTTP {} — 세션 쿠키가 발급되지 않았을 가능성 있음 (body: {})",
            status,
            &text[..text.len().min(200)]
        )));
    }

    parse_jsonp_profile(&callback, &text)
}

fn build_client() -> Result<reqwest::Client, AuthError> {
    let mut headers = HeaderMap::new();
    headers.insert(
        header::ACCEPT,
        HeaderValue::from_static(
            "text/html,application/xhtml+xml,application/xml;q=0.9,image/avif,image/webp,image/apng,*/*;q=0.8,application/signed-exchange;v=b3;q=0.7",
        ),
    );
    headers.insert(
        header::ACCEPT_LANGUAGE,
        HeaderValue::from_static("ko-KR,ko;q=0.9,en-US;q=0.8,en;q=0.7"),
    );

    Ok(reqwest::Client::builder()
        .cookie_store(true)
        .user_agent(USER_AGENT)
        .default_headers(headers)
        // 리다이렉트를 자동으로 따르지 않아 각 단계를 명시적으로 제어
        .redirect(reqwest::redirect::Policy::none())
        .timeout(std::time::Duration::from_secs(HTTP_TIMEOUT_SECS))
        .build()?)
}

pub async fn login(id: &str, pw: &str) -> Result<serde_json::Value, AuthError> {
    let client = build_client()?;

    // Step 0: www.naver.com 방문 — 브라우저가 항상 보유하는 NNB 등 초기 쿠키 수집.
    // 실패해도 계속 진행 (네트워크 오류 무시).
    client.get("https://www.naver.com/").send().await.ok();

    // Step 1: 로그인 폼 GET — 초기 쿠키 수집 + wtoken, dynamicKey 추출
    let form = fetch_form_data(&client, LOGIN_FORM_URL).await?;

    // Step 2: EC 공개키 GET
    let ec_key = fetch_ec_public_key(&client, DYNAMIC_EC_KEY_BASE, &form.dynamic_key).await?;

    // Step 3: CAPTCHA 엔드포인트 호출 (best-effort — tokenId 위치 미확인, bvsd는 항상 "")
    if let Some(sk) = form.sitekey.as_deref() {
        fetch_captcha_token(&client, sk, CAPTCHA_TOKEN_BASE).await;
    }
    let bvsd = String::new();

    // Step 4: ECIES 암호화 후 자격증명 POST
    let eccpw = encrypt_ecies(&ec_key.server_public_key, &ec_key.session_key, id, pw)?;
    submit_login(&client, LOGIN_URL, &eccpw, &form.dynamic_key, &form.wtoken, &bvsd).await?;

    // Step 5: 세션 확정 (NID_AUT, NID_SES 쿠키 발급)
    finalize_session(&client, FINALIZE_URL).await?;

    // Step 6: 실제 세션 유효성 확인 — getProfile 응답 그대로 반환
    verify_session(&client, GET_PROFILE_BASE).await
}

#[cfg(test)]
mod tests {
    use super::*;

    // P-256(secp256r1) 위에 있음이 수학적으로 검증된 실제 서버 공개키
    const SAMPLE_EC_KEY_RESPONSE: &str = concat!(
        "oV83RCXfMVqoxxlAPkiVcx6GAPFcsSty,",
        "04",
        "2279424b355ea6d07693a09e46cf52f1e49f7379833d9f8d01004c8a5a034aaa",
        "384e54d48fc0f0dcb2618efa142aba74a6539f52a3ebe677e7fb0941950da23e"
    );

    #[test]
    fn parse_ec_key_extracts_session_key_and_valid_public_key() {
        let key = parse_ec_key(SAMPLE_EC_KEY_RESPONSE).unwrap();
        assert_eq!(key.session_key, "oV83RCXfMVqoxxlAPkiVcx6GAPFcsSty");
    }

    #[test]
    fn parse_ec_key_invalid_format_returns_err() {
        assert!(parse_ec_key("no_comma_here").is_err());
        assert!(parse_ec_key("session,not_hex!!").is_err());
        // 유효한 hex지만 P-256 위의 점이 아닌 경우
        let bad_point = format!("session,04{}", "00".repeat(64));
        assert!(parse_ec_key(&bad_point).is_err());
    }

    #[test]
    fn encrypt_ecies_produces_valid_four_part_format() {
        let ec_key = parse_ec_key(SAMPLE_EC_KEY_RESPONSE).unwrap();
        let eccpw = encrypt_ecies(
            &ec_key.server_public_key,
            &ec_key.session_key,
            "testid",
            "testpw",
        )
        .unwrap();

        let parts: Vec<&str> = eccpw.splitn(4, ',').collect();
        assert_eq!(parts.len(), 4, "eccpw must have 4 comma-separated parts");

        // 파트0: AES 암호문 — 유효한 base64, 블록 크기(16) 배수
        let ct = STANDARD.decode(parts[0]).expect("part0 must be valid base64");
        assert!(ct.len() > 0 && ct.len() % 16 == 0);

        // 파트1: AES IV — 16바이트 base64
        let iv = STANDARD.decode(parts[1]).expect("part1 must be valid base64");
        assert_eq!(iv.len(), 16);

        // 파트2: 임시 P-256 공개키 — 65바이트 uncompressed hex (0x04 prefix)
        let pubkey_bytes = hex::decode(parts[2]).expect("part2 must be valid hex");
        assert_eq!(pubkey_bytes.len(), 65);
        assert_eq!(pubkey_bytes[0], 0x04);

        // 파트3: sessionKey
        assert_eq!(parts[3], ec_key.session_key);
    }

    #[test]
    fn parse_form_data_extracts_wtoken_and_dynamic_key() {
        let html = r#"
            <input type="hidden" name="wtoken" value="abc123token">
            <input type="hidden" name="dynamicKey" id="dynamicKey" value="testkey456">
        "#;
        let form = parse_form_data(html).unwrap();
        assert_eq!(form.wtoken, "abc123token");
        assert_eq!(form.dynamic_key, "testkey456");
    }

    #[test]
    fn parse_form_data_value_before_name_works() {
        // value가 name보다 먼저 나오는 경우
        let html = r#"<input type="hidden" value="tok999" name="wtoken">
            <input value="dynval" name="dynamicKey">"#;
        let form = parse_form_data(html).unwrap();
        assert_eq!(form.wtoken, "tok999");
        assert_eq!(form.dynamic_key, "dynval");
    }

    #[test]
    fn parse_form_data_missing_field_returns_err() {
        let html = r#"<input type="hidden" name="wtoken" value="abc123">"#;
        assert!(parse_form_data(html).is_err());
    }

    #[tokio::test]
    async fn submit_login_sends_required_fields() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("POST", "/nidlogin.login")
            .match_body(mockito::Matcher::AllOf(vec![
                mockito::Matcher::Regex("eccpw=".to_string()),
                mockito::Matcher::Regex("enctp=1".to_string()),
                mockito::Matcher::Regex("wtoken=test_wtoken".to_string()),
                mockito::Matcher::Regex("dynamicKey=test_dynkey".to_string()),
            ]))
            .with_status(200)
            .with_body("")
            .create_async()
            .await;

        let client = reqwest::Client::builder()
            .cookie_store(true)
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .unwrap();

        submit_login(
            &client,
            &format!("{}/nidlogin.login", server.url()),
            "ct_part,iv_part,pubkey_part,session",
            "test_dynkey",
            "test_wtoken",
            "",
        )
        .await
        .unwrap();
        mock.assert_async().await;
    }

    #[test]
    fn extract_sitekey_from_ncaptcha_script_tag() {
        let html = r#"<script src="https://ncpt.naver.com/static/ncaptcha-api.js?ncaptcha-sitekey=6e3d93f4abc&ncaptcha-other=x" defer=""></script>"#;
        assert_eq!(extract_sitekey(html).as_deref(), Some("6e3d93f4abc"));
    }

    #[test]
    fn extract_sitekey_returns_none_when_absent() {
        assert!(extract_sitekey("<script src=\"other.js\"></script>").is_none());
    }

    #[test]
    fn generate_tid_is_11_alphanumeric_chars() {
        let tid = generate_tid();
        assert_eq!(tid.len(), 11);
        assert!(tid.chars().all(|c| c.is_ascii_alphanumeric() && !c.is_uppercase()));
    }

    #[tokio::test]
    async fn fetch_captcha_token_returns_token_id_on_success() {
        let mut server = mockito::Server::new_async().await;
        let _mock = server
            .mock("POST", mockito::Matcher::Regex(r"^/v2/tokens".to_string()))
            .with_status(200)
            .with_body(r#"{"tokenId":"YTNjNWI1YTE4NDVj"}"#)
            .create_async()
            .await;

        let client = reqwest::Client::builder().cookie_store(true).build().unwrap();
        let result = fetch_captcha_token(
            &client,
            "test-sitekey",
            &format!("{}/v2/tokens", server.url()),
        )
        .await;
        assert_eq!(result.as_deref(), Some("YTNjNWI1YTE4NDVj"));
    }

    #[tokio::test]
    async fn fetch_captcha_token_returns_none_on_server_error() {
        let mut server = mockito::Server::new_async().await;
        let _mock = server
            .mock("POST", mockito::Matcher::Regex(r"^/v2/tokens".to_string()))
            .with_status(500)
            .create_async()
            .await;

        let client = reqwest::Client::builder().cookie_store(true).build().unwrap();
        let result = fetch_captcha_token(
            &client,
            "test-sitekey",
            &format!("{}/v2/tokens", server.url()),
        )
        .await;
        assert!(result.is_none());
    }

    #[test]
    fn parse_jsonp_profile_success_returns_profile() {
        let callback = "jsonp_123";
        let text = r#"jsonp_123({"rtn_cd":"0","rtn_msg":"Success","nick_name":"테스터","image_url":""})"#;
        let v = parse_jsonp_profile(callback, text).unwrap();
        assert_eq!(v["rtn_cd"].as_str(), Some("0"));
        assert_eq!(v["nick_name"].as_str(), Some("테스터"));
    }

    #[test]
    fn parse_jsonp_profile_with_trailing_semicolon_works() {
        // 실제 Naver 서버 응답: 끝에 ";" 포함
        let callback = "jsonp_123";
        let text = r#"jsonp_123({"rtn_cd":"0","rtn_msg":"Success","nick_name":"테스터","image_url":""});"#;
        let v = parse_jsonp_profile(callback, text).unwrap();
        assert_eq!(v["rtn_cd"].as_str(), Some("0"));
    }

    #[test]
    fn parse_jsonp_profile_nonzero_rtn_cd_returns_err() {
        let callback = "jsonp_456";
        let text = r#"jsonp_456({"rtn_cd":"1","rtn_msg":"Unauthorized"})"#;
        let err = parse_jsonp_profile(callback, text).unwrap_err();
        assert!(matches!(err, AuthError::LoginFailed(_)));
        assert!(err.to_string().contains("로그인 세션 없음"));
    }

    #[test]
    fn parse_jsonp_profile_malformed_returns_err() {
        let callback = "jsonp_789";
        // 콜백 이름 불일치
        let err = parse_jsonp_profile(callback, "other_cb({})").unwrap_err();
        assert!(matches!(err, AuthError::LoginFailed(_)));
        assert!(err.to_string().contains("JSONP 파싱 실패"));
    }

    #[test]
    fn extract_login_error_finds_message() {
        let html = r#"<p class="error_message">아이디 또는 비밀번호가 잘못되었습니다.</p>"#;
        assert_eq!(
            extract_login_error(html).as_deref(),
            Some("아이디 또는 비밀번호가 잘못되었습니다.")
        );
    }

    #[test]
    fn extract_login_error_returns_none_when_absent() {
        assert!(extract_login_error("<p>no error here</p>").is_none());
    }

    #[tokio::test]
    async fn submit_login_returns_err_when_body_contains_error_message() {
        let mut server = mockito::Server::new_async().await;
        let _mock = server
            .mock("POST", "/nidlogin.login")
            .with_status(200)
            .with_body(r#"<p class="error_message">잘못된 비밀번호</p>"#)
            .create_async()
            .await;

        let client = reqwest::Client::builder()
            .cookie_store(true)
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .unwrap();

        let err = submit_login(
            &client,
            &format!("{}/nidlogin.login", server.url()),
            "eccpw",
            "dynkey",
            "wtoken",
            "",
        )
        .await
        .unwrap_err();

        assert!(matches!(err, AuthError::LoginFailed(_)));
        assert!(err.to_string().contains("잘못된 비밀번호"));
    }
}
