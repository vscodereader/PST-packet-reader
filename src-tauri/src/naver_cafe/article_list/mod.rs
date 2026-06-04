//! 카페 게시글 목록(최신글/인기글) 조회 모듈.
//!
//! 카페의 게시글 목록을 정렬 기준([`SortBy`])에 따라 조회한다
//! (apis.naver.com article-list API). client/models/parser/service로 분리한다.
//!
//! ⚠️ 미확인 엔드포인트: 실제 패킷 캡처가 없어 경로/파라미터/응답 봉투를
//! 형제 모듈과 공개 API 관례로 추정했다([[#96]]). 검증 후 확정할 것.

pub mod client;
pub mod models;
pub mod parser;
pub mod service;

pub use client::{article_list_path, ArticleListClient, ARTICLE_LIST_API_HOST};
pub use models::{Article, ArticleListError, ArticleListResponse, SortBy};
pub use parser::parse_article_list_body;
pub use service::fetch_article_list_for_account;
