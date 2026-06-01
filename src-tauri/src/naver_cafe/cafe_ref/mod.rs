//! 카페 식별자 해석 및 CafeGateInfo 조회 모듈.
//!
//! 두 가지 주요 기능을 제공한다:
//!
//! 1. **URL/문자열 파싱** ([`parser`]): 사용자가 입력한 URL, 경로, 숫자 문자열에서
//!    카페 식별자를 추출한다. 네트워크 통신 없이 순수 문자열 파싱만 수행한다.
//!
//! 2. **CafeGateInfo 조회** ([`client`]): 숫자 cafeId로 카페 기본 정보를 조회해
//!    검증/표시에 사용한다.
//!
//! # Vanity URL 처리
//!
//! `cafe.naver.com/<name>` 형태의 vanity URL은 [`parser::CafeRef::Vanity`]로
//! 반환된다. vanity 이름 → 숫자 id 변환에 필요한 API 엔드포인트(`?cafeUrl=<name>` 등)는
//! 아직 실측 캡처되지 않았으므로 네트워크 해석을 지원하지 않는다. 향후 확인된 엔드포인트로
//! 구현해야 한다.

pub mod client;
pub mod models;
pub mod parser;

pub use client::{cafe_gate_info_path, CafeGateClient, CAFE_API_HOST};
pub use models::{CafeGateInfoResponse, CafeGateMessage, CafeGateResult, CafeInfoView, CafeRefError};
pub use parser::{parse_cafe_id, parse_cafe_ref, CafeRef};
