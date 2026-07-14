//! JWT 발급·검증. 운영자 토큰(만료 8시간 + 유저버전 ver) / 기기 토큰(만료 없음). 설계 §5·§6.

use jsonwebtoken::{decode, encode, Algorithm, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};

/// 운영자 토큰 클레임: sub=loginId, ver=유저 토큰버전(회수용), exp=만료(8h). 설계 §5.
#[derive(Debug, Serialize, Deserialize)]
pub struct OperatorClaims {
    pub sub: String,
    pub ver: i64,
    pub exp: usize,
}

/// 기기 토큰 클레임: sub=device_id(UUID 문자열). 만료 없음(영구) — 회수는 기기표 줄 삭제(§6-4).
#[derive(Debug, Serialize, Deserialize)]
pub struct DeviceClaims {
    pub sub: String,
    /// 기기 토큰 표시(검증 시 운영자 토큰과 구분). 만료 클레임은 두지 않는다.
    pub kind: String,
}

/// 운영자 JWT 발급(exp = now + ttl).
pub fn issue_operator(
    secret: &[u8],
    login_id: &str,
    ver: i64,
    ttl_secs: i64,
) -> Result<String, String> {
    let exp = (chrono::Utc::now().timestamp() + ttl_secs) as usize;
    let claims = OperatorClaims {
        sub: login_id.to_string(),
        ver,
        exp,
    };
    encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(secret),
    )
    .map_err(|e| e.to_string())
}

/// 운영자 JWT 검증(서명 + 만료). ver 일치는 호출부가 DB와 대조(§5).
pub fn verify_operator(secret: &[u8], token: &str) -> Result<OperatorClaims, String> {
    let v = Validation::new(Algorithm::HS256); // 기본: exp 검증 on
    decode::<OperatorClaims>(token, &DecodingKey::from_secret(secret), &v)
        .map(|d| d.claims)
        .map_err(|e| e.to_string())
}

/// 기기 JWT 발급(만료 없음).
pub fn issue_device(secret: &[u8], device_id: &str) -> Result<String, String> {
    let claims = DeviceClaims {
        sub: device_id.to_string(),
        kind: "dev".into(),
    };
    encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(secret),
    )
    .map_err(|e| e.to_string())
}

/// 기기 JWT 검증(서명만, 만료 없음). 회수는 기기표에서 줄을 지워 처리(§6-4) — 호출부가 device_id 존재 확인.
pub fn verify_device(secret: &[u8], token: &str) -> Result<DeviceClaims, String> {
    let mut v = Validation::new(Algorithm::HS256);
    v.validate_exp = false;
    v.required_spec_claims.clear(); // jsonwebtoken 9는 기본 "exp" 필수 — 기기 토큰엔 exp 없으니 해제
    decode::<DeviceClaims>(token, &DecodingKey::from_secret(secret), &v)
        .map(|d| d.claims)
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn operator_token_roundtrip_with_version() {
        let s = b"test-secret";
        let t = issue_operator(s, "Superadmin", 3, 3600).unwrap();
        let c = verify_operator(s, &t).unwrap();
        assert_eq!(c.sub, "Superadmin");
        assert_eq!(c.ver, 3);
        // 다른 서명키로는 검증 실패.
        assert!(verify_operator(b"other", &t).is_err());
    }

    #[test]
    fn device_token_has_no_expiry() {
        let s = b"dev-secret";
        let t = issue_device(s, "11111111-1111-1111-1111-111111111111").unwrap();
        let c = verify_device(s, &t).unwrap();
        assert_eq!(c.sub, "11111111-1111-1111-1111-111111111111");
    }
}
