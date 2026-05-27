use base64::{Engine, engine::general_purpose::URL_SAFE};
use num_bigint_dig::BigUint;
use rand::thread_rng;
use rsa::{RsaPublicKey, pkcs1v15::Pkcs1v15Encrypt};

#[derive(Debug, thiserror::Error)]
pub enum AuthError {
    #[error("reqwest: {0}")]
    Http(#[from] reqwest::Error),
    #[error("rsa: {0}")]
    Rsa(#[from] rsa::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("invalid jsonp")]
    InvalidJsonp,
    #[error("invalid key format")]
    InvalidKeyFormat,
    #[error("login failed: {0}")]
    LoginFailed(String),
}

struct KeyResponse {
    #[allow(dead_code)]
    session_key: String,
    #[allow(dead_code)]
    key_id: String,
    public_key: RsaPublicKey,
}

// 서버 응답 형식: "세션키,키ID,n(hex),e(hex)"
// 예: "oV83RCXf...,200022062,856d19c5...,010001"
fn extract_public_key(response: &str) -> Result<KeyResponse, AuthError> {
    let parts: Vec<&str> = response.trim().split(',').collect();
    if parts.len() < 4 {
        return Err(AuthError::InvalidKeyFormat);
    }
    let n_bytes = hex::decode(parts[2]).map_err(|_| AuthError::InvalidKeyFormat)?;
    let e_bytes = hex::decode(parts[3]).map_err(|_| AuthError::InvalidKeyFormat)?;
    let n = BigUint::from_bytes_be(&n_bytes);
    let e = BigUint::from_bytes_be(&e_bytes);
    let public_key = RsaPublicKey::new(n, e)?;
    Ok(KeyResponse {
        session_key: parts[0].to_string(),
        key_id: parts[1].to_string(),
        public_key,
    })
}

fn encrypt(public_key: &RsaPublicKey, plaintext: &str) -> Result<String, AuthError> {
    let mut rng = thread_rng();
    let encrypted = public_key.encrypt(&mut rng, Pkcs1v15Encrypt, plaintext.as_bytes())?;
    Ok(URL_SAFE.encode(&encrypted))
}

fn parse_jsonp(callback: &str, body: &str) -> Result<serde_json::Value, AuthError> {
    let prefix = format!("{}(", callback);
    let json_str = body
        .strip_prefix(&prefix)
        .and_then(|s| s.strip_suffix(')'))
        .ok_or(AuthError::InvalidJsonp)?;
    Ok(serde_json::from_str(json_str)?)
}

async fn do_login(client: &reqwest::Client, url: &str, encrypted: &str) -> Result<(), AuthError> {
    client
        .post(url)
        .form(&[("data", encrypted)])
        .send()
        .await?
        .error_for_status()?;
    Ok(())
}

pub async fn login(
    pub_key_url: &str,
    login_url: &str,
    profile_url: &str,
    id: &str,
    pw: &str,
) -> Result<serde_json::Value, AuthError> {
    let client = reqwest::Client::builder().cookie_store(true).build()?;
    let response = client.get(pub_key_url).send().await?.text().await?;
    let key = extract_public_key(&response)?;
    // TODO: 평문 형식 확정 후 수정 (session_key 포함 여부 등)
    let plaintext = format!("{}\n{}", id, pw);
    let encrypted = encrypt(&key.public_key, &plaintext)?;
    do_login(&client, login_url, &encrypted).await?;
    let callback = format!("jsonp_{}", rand::random::<u32>());
    let url = format!("{}?svc=my&callback={}", profile_url, callback);
    let body = client.get(&url).send().await?.text().await?;
    let profile = parse_jsonp(&callback, &body)?;
    match profile["rtn_cd"].as_str() {
        Some("0") => Ok(profile),
        Some(code) => Err(AuthError::LoginFailed(
            profile["rtn_msg"]
                .as_str()
                .unwrap_or(code)
                .to_string(),
        )),
        None => Err(AuthError::LoginFailed("rtn_cd missing".to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::thread_rng;
    use rsa::{RsaPrivateKey, RsaPublicKey, pkcs1v15::Pkcs1v15Encrypt};

    const SAMPLE_KEY_RESPONSE: &str =
        "oV83RCXfMVqoxxlAPkiVcx6GAPFcsSty,200022062,\
         856d19c5558c131fb0a6d44cc32fcad8b47fa7284edcc52635ca0d050e7997ce\
         8ffd2be8d6e25980ae3af36feffb6e88dbc2fd09790389146d536ba56e0074f1\
         39ecd59fafb759019e9e70394b8399586e15289011983b987cca48899180bf29\
         6dc396944ae7ff3ee72c57300046be9f6fd1c791027885b0aa6aa73d2a49117d,\
         010001";

    #[test]
    fn extract_public_key_parses_hex_n_e() {
        let key = extract_public_key(SAMPLE_KEY_RESPONSE).unwrap();
        assert_eq!(key.session_key, "oV83RCXfMVqoxxlAPkiVcx6GAPFcsSty");
        assert_eq!(key.key_id, "200022062");
    }

    #[test]
    fn extract_public_key_invalid_format_returns_err() {
        assert!(extract_public_key("only,two").is_err());
        assert!(extract_public_key("a,b,not_hex_!,010001").is_err());
    }

    #[test]
    fn encrypt_roundtrip() {
        let mut rng = thread_rng();
        let private_key = RsaPrivateKey::new(&mut rng, 2048).unwrap();
        let public_key = RsaPublicKey::from(&private_key);

        let plaintext = "testid\ntestpassword";
        let encoded = encrypt(&public_key, plaintext).unwrap();

        assert!(encoded
            .chars()
            .all(|c| c.is_alphanumeric() || c == '-' || c == '_' || c == '='));

        let decoded = URL_SAFE.decode(&encoded).unwrap();
        let decrypted = private_key.decrypt(Pkcs1v15Encrypt, &decoded).unwrap();
        assert_eq!(std::str::from_utf8(&decrypted).unwrap(), plaintext);
    }

    #[test]
    fn parse_jsonp_success_response() {
        let callback = "jsonp_99999";
        let body = r#"jsonp_99999({"rtn_cd":"0","rtn_msg":"Success","nick_name":"홍길동","image_url":""})"#;
        let value = parse_jsonp(callback, body).unwrap();
        assert_eq!(value["rtn_cd"], "0");
        assert_eq!(value["rtn_msg"], "Success");
        assert_eq!(value["nick_name"], "홍길동");
    }

    #[test]
    fn parse_jsonp_invalid_callback_returns_err() {
        let body = r#"other_callback({"rtn_cd":"0"})"#;
        assert!(parse_jsonp("jsonp_99999", body).is_err());
    }

    #[tokio::test]
    async fn do_login_sends_form_post() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("POST", "/login")
            .match_body(mockito::Matcher::Regex(r"data=.+".to_string()))
            .with_status(200)
            .create_async()
            .await;

        let client = reqwest::Client::builder()
            .cookie_store(true)
            .build()
            .unwrap();
        do_login(&client, &format!("{}/login", server.url()), "encoded_data")
            .await
            .unwrap();
        mock.assert_async().await;
    }
}
