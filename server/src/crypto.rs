//! 암호화 부품: argon2(운영자 비번 해시·단방향), AES-256-GCM(계정 ID/PW at-rest·양방향),
//! 난수(기기코드). 설계 §5·§7.

use aes_gcm::aead::{Aead, KeyInit, OsRng};
use aes_gcm::{Aes256Gcm, Nonce};
use argon2::password_hash::{rand_core::OsRng as PwOsRng, PasswordHash, SaltString};
use argon2::{Argon2, PasswordHasher, PasswordVerifier};
use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;
use rand::Rng;

/// 운영자 비밀번호 해시(argon2id, 단방향). 복원 불가 — 검증만.
pub fn hash_password(pw: &str) -> Result<String, String> {
    let salt = SaltString::generate(&mut PwOsRng);
    Argon2::default()
        .hash_password(pw.as_bytes(), &salt)
        .map(|h| h.to_string())
        .map_err(|e| e.to_string())
}

/// 비밀번호 검증(저장된 PHC 해시와 대조).
pub fn verify_password(pw: &str, phc_hash: &str) -> bool {
    match PasswordHash::new(phc_hash) {
        Ok(parsed) => Argon2::default()
            .verify_password(pw.as_bytes(), &parsed)
            .is_ok(),
        Err(_) => false,
    }
}

/// 계정 ID/PW를 AES-256-GCM으로 암호화 → base64(nonce(12) ‖ ciphertext). at-rest 저장용(§7).
pub fn encrypt(key: &[u8; 32], plaintext: &str) -> Result<String, String> {
    let cipher = Aes256Gcm::new_from_slice(key).map_err(|e| e.to_string())?;
    let mut nonce_bytes = [0u8; 12];
    rand::thread_rng().fill(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);
    let ct = cipher
        .encrypt(nonce, plaintext.as_bytes())
        .map_err(|_| "암호화 실패".to_string())?;
    let mut out = Vec::with_capacity(12 + ct.len());
    out.extend_from_slice(&nonce_bytes);
    out.extend_from_slice(&ct);
    Ok(B64.encode(out))
}

/// 복호화: base64(nonce ‖ ciphertext) → 평문. 분배 시 하위로 보내기 직전 서버 안에서만 복원.
pub fn decrypt(key: &[u8; 32], encoded: &str) -> Result<String, String> {
    let raw = B64.decode(encoded).map_err(|e| e.to_string())?;
    if raw.len() < 12 {
        return Err("암호문 길이 오류".into());
    }
    let (nonce_bytes, ct) = raw.split_at(12);
    let cipher = Aes256Gcm::new_from_slice(key).map_err(|e| e.to_string())?;
    let pt = cipher
        .decrypt(Nonce::from_slice(nonce_bytes), ct)
        .map_err(|_| "복호화 실패(키 불일치/변조)".to_string())?;
    String::from_utf8(pt).map_err(|e| e.to_string())
}

/// 기기코드: 사람이 눈으로 읽고 입력하는 6자리 숫자(§6, 1회용·10분).
pub fn gen_device_code() -> String {
    let n: u32 = rand::thread_rng().gen_range(0..1_000_000);
    format!("{n:06}")
}

/// 사용 안 함 경고 방지용 — AeadOsRng 임포트 유지(키 생성 등 확장 대비).
#[allow(dead_code)]
fn _aead_osrng_marker() -> OsRng {
    OsRng
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn password_hash_roundtrip() {
        let h = hash_password("secret123!").unwrap();
        assert!(verify_password("secret123!", &h));
        assert!(!verify_password("wrong", &h));
    }

    #[test]
    fn aes_roundtrip() {
        let key = [7u8; 32];
        let enc = encrypt(&key, "naverPW@2026").unwrap();
        assert_eq!(decrypt(&key, &enc).unwrap(), "naverPW@2026");
        // 다른 키로는 복호화 실패.
        assert!(decrypt(&[8u8; 32], &enc).is_err());
    }

    #[test]
    fn aes_nonce_is_random_each_time() {
        let key = [1u8; 32];
        assert_ne!(encrypt(&key, "x").unwrap(), encrypt(&key, "x").unwrap());
    }

    #[test]
    fn device_code_is_6_digits() {
        let c = gen_device_code();
        assert_eq!(c.len(), 6);
        assert!(c.chars().all(|ch| ch.is_ascii_digit()));
    }
}
