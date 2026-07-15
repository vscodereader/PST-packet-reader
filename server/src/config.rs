//! 서버 설정. 환경변수로 주입한다(설계 §13 B: 키는 DB와 분리해 환경변수로).
//!
//! ⚠️ 배포 전 결정(사수, PR #324) — 아래 주소/포트/TLS 값은 **배포 환경이 정해지면**
//! 채운다. 코드에는 자리만 두고 기본값(개발용)만 둔다.

use std::env;

pub struct Config {
    /// 바인드 주소·포트. **배포 전 결정**: 운영 시 회사 고정 IP·열어줄 포트로 교체.
    /// (개발 기본값 0.0.0.0:8080 — 모든 인터페이스에서 수신.)
    pub bind_addr: String,
    /// JWT 서명 비밀(HS256). 운영은 반드시 환경변수로 강한 값 주입.
    pub jwt_secret: Vec<u8>,
    /// 계정 ID/PW at-rest 암호화 키(AES-256-GCM, 32바이트). DB와 분리해 환경변수로(§13 B).
    /// 운영은 반드시 환경변수 `PSTMACRO_ENC_KEY`(32바이트 hex/base64 또는 32자) 주입.
    pub enc_key: [u8; 32],
    /// PostgreSQL 접속 URL(사수 확정 DB). 없으면 in-memory 저장소로 동작(개발/오프라인 미리보기).
    pub database_url: Option<String>,
    /// 운영자 JWT 만료(초). 확정 = 8시간(§5).
    pub operator_token_ttl_secs: i64,
    /// 기기코드 만료(초). 설계 = 10분(§6).
    pub device_code_ttl_secs: i64,
    /// 하트비트 타임아웃(초). 초과 시 offline 전환(§4-1).
    pub heartbeat_timeout_secs: i64,
    /// 서버가 자기 공인 주소를 Admin UI에 표시할 때 쓰는 값(§6-1).
    /// **배포 전 결정**: 도메인/고정 IP로 교체. 비면 UI가 안내 문구를 띄운다.
    pub public_server_url: Option<String>,
    /// TLS 인증서/키 경로. **배포 전 결정**: 운영 시 채운다(현재 비움 → 평문 HTTP, 배포 시 앞단/리버스프록시 TLS 권장).
    pub tls_cert_path: Option<String>,
    pub tls_key_path: Option<String>,
    /// 개발용 기본 비밀이 그대로 쓰이는지(운영 모드에서 fail-closed 판단용).
    pub jwt_is_default: bool,
    pub enc_is_default: bool,
}

const DEV_JWT_SECRET: &str = "dev-only-insecure-jwt-secret-change-me";
const DEV_ENC_KEY: &[u8; 32] = b"pstmacro-dev-only-enc-key-32byte";

/// 바인드 주소를 결정한다(순수). 우선순위: **`PORT`**(Cloud Run이 주입, 그 포트로 listen 필수) >
/// `PSTMACRO_BIND`(회사 고정 배포) > 기본 `0.0.0.0:8080`. Cloud Run은 PORT 숫자만 주므로 0.0.0.0에 붙인다.
fn resolve_bind_addr(port: Option<String>, bind: Option<String>) -> String {
    if let Some(p) = port.map(|p| p.trim().to_owned()).filter(|p| !p.is_empty()) {
        return format!("0.0.0.0:{p}");
    }
    bind.map(|b| b.trim().to_owned())
        .filter(|b| !b.is_empty())
        .unwrap_or_else(|| "0.0.0.0:8080".to_owned())
}

impl Config {
    pub fn from_env() -> Self {
        let jwt_env = env::var("PSTMACRO_JWT_SECRET").ok();
        let jwt_is_default = jwt_env.as_deref().map_or(true, |s| s == DEV_JWT_SECRET);
        let (enc_key, enc_is_default) = load_enc_key();
        Config {
            // Cloud Run 호환: PORT 우선(그 포트로 listen 필수), 없으면 PSTMACRO_BIND/기본.
            bind_addr: resolve_bind_addr(
                env::var("PORT").ok(),
                env::var("PSTMACRO_BIND").ok(),
            ),
            jwt_secret: jwt_env
                .unwrap_or_else(|| DEV_JWT_SECRET.into())
                .into_bytes(),
            enc_key,
            jwt_is_default,
            enc_is_default,
            database_url: env::var("DATABASE_URL").ok(),
            operator_token_ttl_secs: 8 * 60 * 60, // 8시간 확정(§5)
            device_code_ttl_secs: 10 * 60,        // 10분(§6)
            heartbeat_timeout_secs: 90,
            public_server_url: env::var("PSTMACRO_PUBLIC_URL").ok(), // 배포 전 결정
            tls_cert_path: env::var("PSTMACRO_TLS_CERT").ok(),       // 배포 전 결정
            tls_key_path: env::var("PSTMACRO_TLS_KEY").ok(),         // 배포 전 결정
        }
    }
}

/// 암호화 키 로드: 환경변수 `PSTMACRO_ENC_KEY`의 **앞 32바이트(raw)** 를 키로 쓴다. 없거나
/// 32바이트 미만이면 개발용 고정 키로 대체하고 `is_default=true`를 돌려준다(운영 모드에서 거부 판단용).
/// 반환: (key, is_default).
fn load_enc_key() -> ([u8; 32], bool) {
    if let Ok(s) = env::var("PSTMACRO_ENC_KEY") {
        let bytes = s.into_bytes();
        if bytes.len() >= 32 {
            let mut k = [0u8; 32];
            k.copy_from_slice(&bytes[..32]);
            return (k, false);
        }
        tracing::warn!("PSTMACRO_ENC_KEY가 32바이트 미만 — 개발용 키로 대체(운영 금지)");
    }
    // 개발 전용 고정 키. 운영에서는 반드시 환경변수 주입(없으면 기동 시 거부, main.rs).
    (*DEV_ENC_KEY, true)
}

#[cfg(test)]
mod tests {
    use super::resolve_bind_addr;

    #[test]
    fn port_env_wins_and_binds_all_interfaces() {
        // Cloud Run: PORT 주입 → 그 포트로 0.0.0.0 바인드(PSTMACRO_BIND 있어도 PORT 우선).
        assert_eq!(
            resolve_bind_addr(Some("8080".into()), Some("127.0.0.1:9999".into())),
            "0.0.0.0:8080"
        );
        assert_eq!(
            resolve_bind_addr(Some(" 3000 ".into()), None),
            "0.0.0.0:3000"
        );
    }

    #[test]
    fn falls_back_to_bind_then_default() {
        // PORT 없으면 PSTMACRO_BIND, 그것도 없으면 기본.
        assert_eq!(
            resolve_bind_addr(None, Some("0.0.0.0:8081".into())),
            "0.0.0.0:8081"
        );
        assert_eq!(resolve_bind_addr(None, None), "0.0.0.0:8080");
        assert_eq!(resolve_bind_addr(Some("".into()), Some("".into())), "0.0.0.0:8080");
    }
}
