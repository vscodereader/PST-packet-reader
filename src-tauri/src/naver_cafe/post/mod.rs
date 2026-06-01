pub mod client;
pub mod error;
pub mod models;
pub mod parser;
pub mod request_builder;
pub mod service;
pub mod smart_editor;

pub use error::{PostError, PostErrorData};
pub use models::PostRequest;
pub use parser::{
    ApiFailure, ArticleRegisterResult, ArticleWriteForm, Head, WriteInfo,
};
pub use request_builder::{
    article_post_headers, article_post_path, build_article_write_body,
    build_article_write_body_with_content, ArticleWrite, ArticleWriteBody, API_HOST,
};
pub use client::{
    cookie_header_from_storage_state, CafeHttpClient, BROWSER_USER_AGENT,
    CODE_HTTP_TRANSPORT_ERROR, CODE_REGISTER_HTTP_ERROR, CODE_REGISTER_PARSE_ERROR,
    CODE_SESSION_INVALID,
};
pub use service::{
    build_post_preview, execute_post, execute_post_live, PostExecutionMode, PostOutcome,
    PostPreview, CODE_CONTENT_BUILD_FAILED, CODE_LIVE_SEND_NOT_IMPLEMENTED,
};
pub use smart_editor::{
    build_content_document, build_content_json_string, ContentJsonRoot, IdProvider,
    SequentialIdProvider,
};
