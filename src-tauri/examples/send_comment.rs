//! 댓글/대댓글 작성 end-to-end 진단 예제.
//!
//! `send_post.rs`(글 작성)의 댓글 버전이다. `send_post`가 `CafeHttpClient`를
//! 쓰듯, 이 예제는 라이브러리의 [`CafeCommentClient`]로 실제 전송을 수행한다.
//!
//! `--ref-comment`(부모 댓글 ID)를 주면 **대댓글**(CommentReply.json),
//! 없으면 **일반 댓글**(CommentPost.json)로 전송한다.
//! `--cafe`에는 URL / vanity(`cafe.naver.com/<slug>`) / 숫자 cafeId 모두 가능하다.
//!
//! # 사용법
//!
//! ```bash
//! # 로컬 테스트 — 쿠키 파일 직접 지정 (WSL 친화적), 일반 댓글:
//! PSTMACRO_LIVE_COOKIES_PATH=/tmp/<id>.json \
//! cargo run --example send_comment -- --cafe 31732304 --article 2 --content '안녕하세요'
//!
//! # 대댓글 — 부모 댓글 ID 지정:
//! PSTMACRO_LIVE_COOKIES_PATH=/tmp/<id>.json \
//! cargo run --example send_comment -- --cafe 31732304 --article 4 --ref-comment 62598693 --content '답글입니다'
//!
//! # production — appdata 에 저장된 계정 쿠키 사용:
//! PSTMACRO_LIVE_ACCOUNT_ID=<계정id> \
//! cargo run --example send_comment -- --cafe 'cafe.naver.com/<slug>' --article 2 --content '댓글'
//! ```
//!
//! 선택 환경변수/플래그: `--content`(기본 "테스트 댓글").
//!
//! 주의: 실제 네이버 카페에 댓글이 작성됩니다. 본인의 테스트 카페에만 사용하세요.
//! 보안: 쿠키 값/헤더는 절대 출력하지 않습니다.

use std::{env, fs};

use pstmacro_lib::auth::{read_account_cookies, read_account_cookies_unchecked};
use pstmacro_lib::naver_cafe::comment::{CafeCommentClient, CommentRequest, ReplyRequest};
use pstmacro_lib::naver_cafe::post::cookie_header_from_storage_state;
use pstmacro_lib::naver_cafe::CafeOrchestrator;
use serde_json::Value;

#[tokio::main]
async fn main() {
    let args: Vec<String> = env::args().collect();
    if args.iter().any(|a| a == "--help" || a == "-h") {
        print_usage();
        return;
    }

    let cafe_input = pick(&args, "--cafe", "PSTMACRO_LIVE_CAFE_ID").unwrap_or_else(|| {
        eprintln!(
            "카페가 필요합니다 (--cafe 또는 PSTMACRO_LIVE_CAFE_ID) — URL/vanity/숫자 모두 가능"
        );
        print_usage();
        std::process::exit(2);
    });
    let article_id = pick(&args, "--article", "PSTMACRO_LIVE_ARTICLE_ID").unwrap_or_else(|| {
        eprintln!("게시글 ID가 필요합니다 (--article 또는 PSTMACRO_LIVE_ARTICLE_ID)");
        print_usage();
        std::process::exit(2);
    });
    let content = pick(&args, "--content", "PSTMACRO_LIVE_CONTENT")
        .unwrap_or_else(|| "테스트 댓글".to_string());
    // --ref-comment 가 있으면 대댓글, 없으면 일반 댓글.
    let ref_comment_id = pick(&args, "--ref-comment", "PSTMACRO_LIVE_REF_COMMENT_ID");

    // 쿠키 확보 (파일 경로 우선, 없으면 account_id 로 조회)
    let cookies_value = match resolve_cookies(&args) {
        Ok(v) => v,
        Err(msg) => {
            eprintln!("{msg}");
            std::process::exit(1);
        }
    };
    // 보안: 쿠키 헤더 값은 절대 출력하지 않는다.
    let Some(cookie_header) = cookie_header_from_storage_state(&cookies_value) else {
        eprintln!("쿠키에서 네이버 세션 쿠키를 찾지 못했습니다. (로그인 상태를 확인하세요)");
        std::process::exit(1);
    };
    let cookie = Some(cookie_header.as_str());

    // --- 1) 카페 식별자 해석 (URL/vanity/숫자 → 숫자 cafeId) ---
    println!("=== 1) 카페 해석: {cafe_input:?} ===");
    let cafe_id = match CafeOrchestrator::new()
        .resolve_cafe_id(&cafe_input, cookie)
        .await
    {
        Ok(id) => {
            println!("해석된 cafeId: {id}");
            id.to_string()
        }
        Err(e) => {
            eprintln!("카페 해석 실패:");
            eprintln!("{}", serde_json::to_string_pretty(&e).unwrap_or_default());
            std::process::exit(1);
        }
    };
    println!();

    // --- 2) 댓글/대댓글 전송 (CafeCommentClient — send_post 의 CafeHttpClient 대응) ---
    let kind = if ref_comment_id.is_some() {
        "대댓글"
    } else {
        "일반 댓글"
    };
    println!("=== 2) {kind} 작성 ===");
    println!(
        "요청: cafeId={cafe_id} articleId={article_id} ref={:?} content={content:?}",
        ref_comment_id
    );

    let client = CafeCommentClient::new();
    let cookie = Some(cookie_header.as_str());
    let result = match &ref_comment_id {
        Some(ref_id) => {
            let req = ReplyRequest {
                cafe_id: cafe_id.clone(),
                article_id: article_id.clone(),
                content: content.clone(),
                sticker_id: None,
                ref_comment_id: ref_id.clone(),
            };
            client.post_reply(&req, cookie).await
        }
        None => {
            let req = CommentRequest {
                cafe_id: cafe_id.clone(),
                article_id: article_id.clone(),
                content: content.clone(),
                sticker_id: None,
            };
            client.post_comment(&req, cookie).await
        }
    };

    match result {
        Ok(result) => {
            println!("성공: {kind}이(가) 작성되었습니다.");
            println!("  commentId    = {}", result.comment_id);
            println!("  refCommentId = {}", result.ref_comment_id);
            println!("  is_reply     = {}", result.is_reply());
        }
        Err(comment_error) => {
            // CommentError 에는 http_status, errorCode, reason(api_error_message)이 담김. 쿠키는 없음.
            eprintln!("실패 응답 (쿠키 값은 포함되지 않음):");
            eprintln!(
                "{}",
                serde_json::to_string_pretty(&comment_error).unwrap_or_default()
            );
            std::process::exit(1);
        }
    }
}

/// 쿠키 값을 확보한다.
///
/// - [로컬 테스트] `--cookies`/`PSTMACRO_LIVE_COOKIES_PATH` 가 있으면 그 json 파일을 직접 읽는다.
/// - [production] 없으면 `--account`/`PSTMACRO_LIVE_ACCOUNT_ID` 로 cookies 폴더의
///   `<account_id>.json` 을 조회한다.
fn resolve_cookies(args: &[String]) -> Result<Value, String> {
    if let Some(path) = pick(args, "--cookies", "PSTMACRO_LIVE_COOKIES_PATH") {
        println!("쿠키 파일 직접 사용(로컬 테스트): {path}");
        let text =
            fs::read_to_string(&path).map_err(|e| format!("쿠키 파일을 읽지 못했습니다: {e}"))?;
        let value = serde_json::from_str(&text).map_err(|e| format!("쿠키 JSON 파싱 실패: {e}"))?;
        return Ok(value);
    }

    let account_id = pick(args, "--account", "PSTMACRO_LIVE_ACCOUNT_ID").ok_or_else(|| {
        "쿠키 소스가 필요합니다: --cookies <경로> / PSTMACRO_LIVE_COOKIES_PATH\n또는 --account <id> / PSTMACRO_LIVE_ACCOUNT_ID".to_string()
    })?;

    match read_account_cookies(&account_id) {
        Ok(Some(_)) => println!("쿠키 상태: 유효(valid)"),
        Ok(None) => println!("쿠키 상태: 없음/만료(invalid) — 성공하려면 다시 로그인하세요"),
        Err(e) => println!("쿠키 상태 확인 실패: {e}"),
    }

    match read_account_cookies_unchecked(&account_id) {
        Ok(Some(v)) => Ok(v),
        Ok(None) => Err(format!("계정 '{account_id}' 의 쿠키 파일이 cookies 폴더에 없습니다.")),
        Err(e) => Err(format!(
            "쿠키 읽기 실패: {e}\n(WSL이면 LOCALAPPDATA 가 Windows AppData\\Local 을 가리키도록 설정하세요.)"
        )),
    }
}

/// CLI 플래그(우선) 또는 환경변수에서 값을 읽는다.
fn pick(args: &[String], flag: &str, env_key: &str) -> Option<String> {
    args.windows(2)
        .find(|pair| pair[0] == flag)
        .map(|pair| pair[1].clone())
        .or_else(|| env::var(env_key).ok())
}

fn print_usage() {
    eprintln!("usage:");
    eprintln!("  일반 댓글: PSTMACRO_LIVE_COOKIES_PATH=/tmp/<id>.json cargo run --example send_comment -- --cafe 31732304 --article 2 --content '안녕하세요'");
    eprintln!("  대댓글:    ... cargo run --example send_comment -- --cafe 31732304 --article 4 --ref-comment 62598693 --content '답글'");
    eprintln!("  production: PSTMACRO_LIVE_ACCOUNT_ID=<id> cargo run --example send_comment -- --cafe 'cafe.naver.com/<slug>' --article 2");
    eprintln!("  플래그: --account --cookies --cafe --article --content --ref-comment");
    eprintln!();
    eprintln!("  --cafe 는 URL / vanity(cafe.naver.com/<slug>) / 숫자 cafeId 모두 가능.");
    eprintln!("  --ref-comment 가 있으면 대댓글, 없으면 일반 댓글로 전송.");
    eprintln!();
    eprintln!("주의: 실제 카페에 댓글이 작성됩니다. 본인 테스트 카페에만 사용하세요.");
}
