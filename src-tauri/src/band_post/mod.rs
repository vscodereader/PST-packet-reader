//! band.us 가입·글쓰기·댓글을 순수 HTTP로 수행하는 모듈(사수 지시: CDP 폐기, 패킷분석).
//!
//! 로그인은 기존 [`band_auth`](crate::band_auth)(CDP)로 쿠키를 확보하고, 이 모듈은
//! 그 쿠키로 `api-kr.band.us`에 직접 요청한다. 모든 요청은 `md` 서명([`signature`])이
//! 필요하며, 서명 키(`secretKey`)는 [`getkey`]로 받아온다.
//!
//! 네이버 카페 모듈([`naver_cafe`](crate::naver_cafe)) 구조를 미러한다:
//! 순수 빌더([`request_builder`]) + HTTP 클라이언트([`client`]) 분리.

pub mod getkey;
pub mod signature;
pub mod util;
