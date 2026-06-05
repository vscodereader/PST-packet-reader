//! 카페 게시글 목록(최신글/인기글) 조회 모듈.
//!
//! 카페의 게시글 목록을 정렬 기준([`SortBy`])에 따라 조회한다(apis.naver.com).
//! 최신글과 인기글은 호출 API가 달라(최신글=`cafe-boardlist-api`, 인기글=주간
//! 인기글 `cafe2/WeeklyPopularArticleListV3`) 내부 역직렬화 타입도 분리한다.
//! 스키마는 실패킷(2026-06-05)으로 확정했다. client/models/parser/service로 나눈다.

pub mod client;
pub mod models;
pub mod parser;
pub mod service;

pub use client::{ArticleListClient, ARTICLE_LIST_API_HOST};
pub use models::{Article, ArticleListError, ArticleListResponse, SortBy};
pub use service::fetch_article_list_for_account;
