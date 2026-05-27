use aes::cipher::{block_padding::Pkcs7, BlockEncryptMut, KeyIvInit};
use base64::{Engine, engine::general_purpose::STANDARD};
use p256::{ecdh::EphemeralSecret, elliptic_curve::sec1::ToEncodedPoint, PublicKey};
use rand::{RngCore, rngs::OsRng};
use sha2::{Digest, Sha256};

type Aes256CbcEnc = cbc::Encryptor<aes::Aes256>;

const LOGIN_FORM_URL: &str =
    "https://nid.naver.com/nidlogin.login?mode=form&url=https://www.naver.com/";
const DYNAMIC_EC_KEY_BASE: &str = "https://nid.naver.com/login/dynamicEcKey/";
const LOGIN_URL: &str = "https://nid.naver.com/nidlogin.login";
const FINALIZE_URL: &str =
    "https://nid.naver.com/signin/v3/finalize?url=https%3A%2F%2Fwww.naver.com%2F&svctype=1";
const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) \
    AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36";

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

fn parse_form_data(html: &str) -> Result<FormData, AuthError> {
    let wtoken = find_input_value(html, "wtoken").ok_or(AuthError::FormParse)?;
    let dynamic_key = find_input_value(html, "dynamicKey").ok_or(AuthError::FormParse)?;
    Ok(FormData { wtoken, dynamic_key })
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

    // 평문 형식: "id\npw" (추가 패킷 캡처로 확정 필요)
    let plaintext = format!("{}\n{}", id, pw);
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

async fn submit_login(
    client: &reqwest::Client,
    url: &str,
    eccpw: &str,
    dynamic_key: &str,
    wtoken: &str,
) -> Result<(), AuthError> {
    client
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
            ("next_step", ""),
            ("show_pk", "0"),
            ("bvsd", ""),
        ])
        .send()
        .await?
        .error_for_status()?;
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

pub async fn login(id: &str, pw: &str) -> Result<serde_json::Value, AuthError> {
    let client = reqwest::Client::builder()
        .cookie_store(true)
        .user_agent(USER_AGENT)
        // 리다이렉트를 자동으로 따르지 않아 각 단계를 명시적으로 제어
        .redirect(reqwest::redirect::Policy::none())
        .build()?;

    // Step 1: 로그인 폼 GET — 초기 쿠키 수집 + wtoken, dynamicKey 추출
    let form = fetch_form_data(&client, LOGIN_FORM_URL).await?;

    // Step 2: EC 공개키 GET
    let ec_key = fetch_ec_public_key(&client, DYNAMIC_EC_KEY_BASE, &form.dynamic_key).await?;

    // Step 3: CAPTCHA 토큰 — ncaptcha-api.js 의존, 별도 구현 예정

    // Step 4: ECIES 암호화 후 자격증명 POST
    let eccpw = encrypt_ecies(&ec_key.server_public_key, &ec_key.session_key, id, pw)?;
    submit_login(&client, LOGIN_URL, &eccpw, &form.dynamic_key, &form.wtoken).await?;

    // Step 5: 세션 확정 (NID_AUT, NID_SES 쿠키 발급)
    finalize_session(&client, FINALIZE_URL).await?;

    Ok(serde_json::json!({ "success": true }))
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
        )
        .await
        .unwrap();
        mock.assert_async().await;
    }
}
