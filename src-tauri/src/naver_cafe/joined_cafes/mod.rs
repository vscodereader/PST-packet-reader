//! 가입 카페 목록 조회 모듈.
//!
//! 현재 로그인 계정이 가입한 네이버 카페 목록을 크롤링한다
//! (apis.naver.com 가입 카페 관리 API, 페이지네이션 포함).

pub mod client;
pub mod models;

pub use client::{join_cafes_path, JoinedCafesClient, JOINED_CAFES_HOST};
pub use models::{JoinedCafe, JoinedCafesError};
