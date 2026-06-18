//! 카페 게시글 목록(최신글/인기글) 조회 — 실 네이버 검증용 진단 예제.
//!
//! 이슈 #96에서 추가한 게시글 목록 모듈([`pstmacro_lib::naver_cafe::article_list`])을
//! 실제 네이버 카페 API에 대고 실행해, 사람이 직접 결과를 눈으로 확인할 수 있게 한다.
//!
//! #96의 엔드포인트/응답 스키마는 실패킷(2026-06-05)으로 확정했다(최신글=
//! boardlist-api, 인기글=주간 인기글 V3). 이 예제로 실제 네이버 응답을 계속
//! 회귀 점검할 수 있다 — 성공 표가 나오면 정상이고,
//! `ARTICLE_LIST_PARSE_ERROR` / `ARTICLE_LIST_API_ERROR` 등이 나오면 네이버가
//! 스키마를 바꿨다는 신호다.
//!
//! `send_comment`가 `CafeCommentClient`로 실제 전송을 하듯, 이 예제는
//! [`ArticleListClient`]로 실제 조회만 수행한다(읽기 전용).
//! `--cafe`에는 URL / vanity(`cafe.naver.com/<slug>`) / 숫자 cafeId 모두 가능하다.
//!
//! # 사용법
//!
//! ```bash
//! # 로컬 테스트 — 쿠키 파일 직접 지정 (WSL 친화적), 최신글:
//! PSTMACRO_LIVE_COOKIES_PATH=/tmp/<id>.json \
//! cargo run --example list_articles -- --cafe 31732304
//!
//! # 인기글 정렬:
//! PSTMACRO_LIVE_COOKIES_PATH=/tmp/<id>.json \
//! cargo run --example list_articles -- --cafe 31732304 --sort popular
//!
//! # production — appdata 에 저장된 계정 쿠키 사용, vanity 카페:
//! PSTMACRO_LIVE_ACCOUNT_ID=<계정id> \
//! cargo run --example list_articles -- --cafe 'cafe.naver.com/<slug>' --sort latest
//! ```
//!
//! 선택 환경변수/플래그: `--sort latest|popular`(기본 `latest`).
//! 로그: `PSTMACRO_LOG=debug` 로 요청/페이지별 로그 표시.
//!
//! 읽기 전용입니다 — 게시글/댓글 등 어떤 쓰기 API도 호출하지 않습니다.
//! 보안: 쿠키 값/헤더는 절대 출력하지 않습니다.

use std::{env, fs};

use pstmacro_lib::auth::{read_account_cookies, read_account_cookies_unchecked};
use pstmacro_lib::naver_cafe::article_list::{ArticleListClient, SortBy};
use pstmacro_lib::naver_cafe::post::cookie_header_from_storage_state;
use pstmacro_lib::naver_cafe::CafeOrchestrator;
use serde_json::Value;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() {
    // 진단용 stdout 로깅(앱의 파일 로깅과 별개). PSTMACRO_LOG 로 레벨 제어.
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_env("PSTMACRO_LOG").unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .with_target(false)
        .try_init();

    let args: Vec<String> = env::args().collect();
    // --help 는 쿠키/네트워크 해석 전에 즉시 종료한다.
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

    // --sort latest|popular (기본 latest)
    let sort_by = match pick(&args, "--sort", "PSTMACRO_LIVE_SORT")
        .unwrap_or_else(|| "latest".to_string())
        .to_ascii_lowercase()
        .as_str()
    {
        "latest" => SortBy::Latest,
        "popular" => SortBy::Popular,
        other => {
            eprintln!("알 수 없는 정렬 기준: {other:?} (latest | popular 중 하나)");
            print_usage();
            std::process::exit(2);
        }
    };

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

    // --- 2) 게시글 목록 조회 (ArticleListClient — 읽기 전용) ---
    let sort_label = match sort_by {
        SortBy::Latest => "최신글(latest)",
        SortBy::Popular => "인기글(popular)",
    };
    println!("=== 2) 게시글 목록 조회 — {sort_label} ===");
    println!("요청: cafeId={cafe_id} ({sort_label})\n");

    let client = ArticleListClient::new();
    match client
        .fetch_article_list(&cafe_id, sort_by, 1, cookie)
        .await
    {
        Ok(response) => {
            print_articles(&response);
        }
        Err(err) => {
            // ErrorEnvelope: code(ARTICLE_LIST_PARSE_ERROR 등) + message + error_data.
            // 쿠키/세션 값은 포함되지 않는다. pretty JSON 으로 보여 스키마 진단을 돕는다.
            eprintln!("실패 응답 (쿠키 값은 포함되지 않음):");
            eprintln!("{}", serde_json::to_string_pretty(&err).unwrap_or_default());
            eprintln!();
            eprintln!("↑ code 가 ARTICLE_LIST_PARSE_ERROR / ARTICLE_LIST_API_ERROR 라면 네이버가");
            eprintln!("  엔드포인트/응답 스키마를 바꿨다는 신호입니다 — 모듈을 수정하세요.");
            std::process::exit(1);
        }
    }
}

/// 조회 결과를 읽기 좋은 표로 출력한다.
fn print_articles(response: &pstmacro_lib::naver_cafe::article_list::ArticleListResponse) {
    let total = response.articles.len();
    if total == 0 {
        println!("게시글이 없습니다.");
        return;
    }

    println!("게시글 {total}건:\n");
    println!(
        "{:>10}  {:<40}  {:<16}  {:<16}  {:>4}/{:>5}/{:>4}",
        "articleId", "subject", "writer", "menu", "댓글", "조회", "좋아요"
    );
    println!("{}", "-".repeat(110));
    for a in &response.articles {
        println!(
            "{:>10}  {:<40}  {:<16}  {:<16}  {:>4}/{:>5}/{:>4}",
            a.article_id,
            truncate(&a.subject, 40),
            truncate(&a.writer_nickname, 16),
            truncate(&a.menu_name, 16),
            a.comment_count,
            a.read_count,
            a.like_count,
        );
    }
    println!();
    println!("총 {total}건");
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

/// 표 정렬용 — 길면 잘라낸다(표시용일 뿐, 데이터는 온전함).
fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let cut: String = s.chars().take(max.saturating_sub(1)).collect();
        format!("{cut}…")
    }
}

fn print_usage() {
    eprintln!("usage:");
    eprintln!("  로컬 테스트: PSTMACRO_LIVE_COOKIES_PATH=/tmp/<id>.json cargo run --example list_articles -- --cafe 31732304");
    eprintln!(
        "  인기글:      ... cargo run --example list_articles -- --cafe 31732304 --sort popular"
    );
    eprintln!("  production: PSTMACRO_LIVE_ACCOUNT_ID=<id> cargo run --example list_articles -- --cafe 'cafe.naver.com/<slug>'");
    eprintln!("  플래그: --account <id> | --cookies <path> | --cafe <cafeId|url|vanity> | --sort <latest|popular>");
    eprintln!("  로그: PSTMACRO_LOG=debug 로 요청/페이지별 로그 표시");
    eprintln!();
    eprintln!("  --cafe 는 URL / vanity(cafe.naver.com/<slug>) / 숫자 cafeId 모두 가능.");
    eprintln!("  --sort 는 latest(최신글, 기본) 또는 popular(인기글).");
    eprintln!();
    eprintln!("카페의 게시글 목록(최신글/인기글)을 조회해 표로 출력합니다.");
    eprintln!("읽기 전용 — 게시글/댓글 등 어떤 쓰기 API도 호출하지 않습니다.");
}
