//! 최신글/인기글 top-N 댓글 백엔드 흐름(end-to-end) 검증 예제.
//!
//! 이슈 [[#97]]의 UI("최신/인기글 상위 N개에 댓글")가 구동하는 **백엔드 왕복**을
//! 실서버로 검증한다:
//!
//!   1) `resolve_cafe_id`            — 카페 URL/vanity/숫자 → 숫자 cafeId 해석
//!   2) `ArticleListClient`([[#96]]) — 최신글/인기글 목록 조회 ([`SortBy`])
//!   3) top-N 선택                   — 목록 상위 N건을 댓글 대상으로 선정
//!   4) `CafeCommentClient`          — (--commit 시) 각 대상에 댓글 작성
//!
//! # 정직성 안내 (중요)
//!
//! #97의 **실제 top-N 선택 + 작업(job) 생성 로직은 프론트엔드(TypeScript)에**
//! 있다 — `src/features/posts/comment-jobs.ts` 의 `topNArticles` /
//! `buildArticleListCommentJobs`. 이 Rust 예제는 그 TS 코드를 호출하지 않고,
//! 백엔드 조각(#96 목록 조회 + 댓글 작성)을 실제 네이버에 대해 실행하기 위해
//! top-N 선택을 **Rust로 재구현**한다(`articles.iter().take(n)`, 목록이 N보다
//! 짧으면 있는 만큼만 — TS의 graceful fallback 동작을 그대로 반영).
//!
//! 따라서 이 예제가 검증하는 것은 **UI가 의존하는 백엔드 왕복**이지, TS 코드
//! 자체가 아니다. TS의 top-N/job 생성 로직은 vitest(`comment-jobs.test.ts`)와
//! 수동 UI 테스트로 커버된다.
//!
//! # 안전 (DRY-RUN 기본값)
//!
//! 기본은 **DRY-RUN**이다: 목록을 조회하고 top-N을 골라 **댓글 계획만 출력**하며
//! 아무것도 게시하지 않는다. `--commit`을 줄 때에만 실제로 댓글을 작성한다.
//!
//! # 사용법
//!
//! ```bash
//! # DRY-RUN(기본) — 최신글 상위 3개의 댓글 계획만 출력, 게시 없음:
//! PSTMACRO_LIVE_COOKIES_PATH=/tmp/<id>.json \
//! cargo run --example comment_on_articles -- --cafe 'cafe.naver.com/<slug>'
//!
//! # DRY-RUN — 인기글 상위 5개, 사용자 지정 댓글 본문:
//! PSTMACRO_LIVE_COOKIES_PATH=/tmp/<id>.json \
//! cargo run --example comment_on_articles -- --cafe 31732304 --sort popular --count 5 --content '잘 봤습니다'
//!
//! # COMMIT — 실제로 댓글을 작성(주의!), production 계정 쿠키 사용:
//! PSTMACRO_LIVE_ACCOUNT_ID=<계정id> \
//! cargo run --example comment_on_articles -- --cafe 31732304 --count 2 --commit
//! ```
//!
//! 플래그: `--account`/`--cookies`(+ 동등 env), `--cafe`(필수, URL/vanity/숫자),
//! `--sort latest|popular`(기본 latest), `--count <N>`(기본 3),
//! `--content <text>`(기본 "테스트 댓글"), `--commit`(없으면 DRY-RUN), `--help`/`-h`.
//! 로그: `PSTMACRO_LOG=debug`.
//!
//! 주의: `--commit` 은 **실제 네이버 카페에 댓글을 작성**합니다. 본인의 테스트
//! 카페에만 사용하세요.
//! 보안: 쿠키 값/헤더는 절대 출력하지 않습니다.

use std::{env, fs};

use pstmacro_lib::auth::{read_account_cookies, read_account_cookies_unchecked};
use pstmacro_lib::naver_cafe::comment::{CafeCommentClient, CommentRequest};
use pstmacro_lib::naver_cafe::post::cookie_header_from_storage_state;
use pstmacro_lib::naver_cafe::{ArticleListClient, CafeOrchestrator, SortBy};
use serde_json::Value;
use tracing_subscriber::EnvFilter;

/// top-N 기본값 — 목록 상위 몇 개를 댓글 대상으로 삼을지.
const DEFAULT_COUNT: usize = 3;

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
    // --help 는 쿠키/네트워크 이전에 가장 먼저 단락(short-circuit)한다.
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
    let sort_by = match pick(&args, "--sort", "PSTMACRO_LIVE_SORT")
        .unwrap_or_else(|| "latest".to_string())
        .to_ascii_lowercase()
        .as_str()
    {
        "latest" => SortBy::Latest,
        "popular" => SortBy::Popular,
        other => {
            eprintln!("--sort 는 latest 또는 popular 여야 합니다: {other:?}");
            print_usage();
            std::process::exit(2);
        }
    };
    let count: usize = pick(&args, "--count", "PSTMACRO_LIVE_COUNT")
        .map(|raw| {
            raw.trim().parse().unwrap_or_else(|_| {
                eprintln!("--count 는 0 이상의 정수여야 합니다: {raw:?}");
                std::process::exit(2);
            })
        })
        .unwrap_or(DEFAULT_COUNT);
    let content = pick(&args, "--content", "PSTMACRO_LIVE_CONTENT")
        .unwrap_or_else(|| "테스트 댓글".to_string());
    // --commit 가 없으면 DRY-RUN(아무것도 게시하지 않음).
    let commit = args.iter().any(|a| a == "--commit");

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

    // 모드 배너 — DRY-RUN vs COMMIT 을 시끄럽고 분명하게 표시한다.
    if commit {
        println!("################################################################");
        println!("## 모드: COMMIT — 실제 네이버 카페에 댓글이 작성됩니다! (--commit)");
        println!("## 본인의 테스트 카페에만 사용하세요.");
        println!("################################################################");
    } else {
        println!("================================================================");
        println!("== 모드: DRY-RUN — 계획만 출력하고 아무것도 게시하지 않습니다.");
        println!("== 실제로 게시하려면 --commit 을 추가하세요.");
        println!("================================================================");
    }
    println!();

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

    // --- 2) 게시글 목록 조회 (#96 ArticleListClient) ---
    let sort_label = match sort_by {
        SortBy::Latest => "최신글(latest)",
        SortBy::Popular => "인기글(popular)",
    };
    println!("=== 2) {sort_label} 목록 조회 ===");
    let list = match ArticleListClient::new()
        .fetch_article_list(&cafe_id, sort_by, cookie)
        .await
    {
        Ok(list) => list,
        Err(e) => {
            // 목록 조회 실패(예: 추정 스키마 불일치 → ARTICLE_LIST_PARSE_ERROR).
            eprintln!("게시글 목록 조회 실패 (쿠키 값은 포함되지 않음):");
            eprintln!("{}", serde_json::to_string_pretty(&e).unwrap_or_default());
            std::process::exit(1);
        }
    };
    println!("조회된 게시글 {}건", list.articles.len());
    println!();

    // --- 3) top-N 선택 (Rust 재구현; TS topNArticles 의 graceful fallback 반영) ---
    // 주의: 이것은 프론트엔드 topNArticles 의 Rust 재구현이다(정직성 안내 참조).
    // 목록이 N보다 짧으면 있는 만큼만 사용한다.
    let selected: Vec<_> = list.articles.iter().take(count).collect();
    println!(
        "=== 3) top-N 선택: 요청 count={count}, 선택됨 {}건 ===",
        selected.len()
    );
    if selected.is_empty() {
        eprintln!("선택된 게시글이 없습니다 (목록이 비어 있거나 --count 0). 작업 없이 종료합니다.");
        std::process::exit(1);
    }
    for (i, a) in selected.iter().enumerate() {
        println!(
            "  [{}] cafeId={} articleId={} subject={:?}",
            i + 1,
            cafe_id,
            a.article_id,
            a.subject
        );
        println!("      계획 댓글: {content:?}");
    }
    println!();

    // --- 4) 게시 (DRY-RUN 이면 건너뛰고, --commit 이면 각 글에 댓글 작성) ---
    if !commit {
        println!("DRY-RUN: 아무것도 게시하지 않았습니다. 실제 게시하려면 --commit 을 추가하세요.");
        return;
    }

    println!("=== 4) 댓글 작성 (COMMIT) ===");
    let client = CafeCommentClient::new();
    let mut ok_count = 0usize;
    let mut fail_count = 0usize;
    for (i, a) in selected.iter().enumerate() {
        let label = format!("[{}/{}] articleId={}", i + 1, selected.len(), a.article_id);
        let req = CommentRequest {
            cafe_id: cafe_id.clone(),
            article_id: a.article_id.to_string(),
            content: content.clone(),
            sticker_id: None,
        };
        match client.post_comment(&req, cookie).await {
            Ok(result) => {
                ok_count += 1;
                println!("  성공 {label} — commentId={}", result.comment_id);
            }
            Err(comment_error) => {
                fail_count += 1;
                // 실패 응답에는 http_status/errorCode/reason 이 담긴다. 쿠키 값은 없음.
                eprintln!("  실패 {label} (쿠키 값은 포함되지 않음):");
                eprintln!(
                    "{}",
                    serde_json::to_string_pretty(&comment_error).unwrap_or_default()
                );
            }
        }
    }
    println!();
    println!(
        "완료: 성공 {ok_count}건, 실패 {fail_count}건 (대상 {}건).",
        selected.len()
    );
    if fail_count > 0 {
        std::process::exit(1);
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
    eprintln!("  DRY-RUN(기본): PSTMACRO_LIVE_COOKIES_PATH=/tmp/<id>.json cargo run --example comment_on_articles -- --cafe 'cafe.naver.com/<slug>'");
    eprintln!("  인기글 5개:    ... cargo run --example comment_on_articles -- --cafe 31732304 --sort popular --count 5 --content '잘 봤습니다'");
    eprintln!("  COMMIT(실게시): PSTMACRO_LIVE_ACCOUNT_ID=<id> cargo run --example comment_on_articles -- --cafe 31732304 --count 2 --commit");
    eprintln!(
        "  플래그: --account --cookies --cafe --sort(latest|popular) --count --content --commit"
    );
    eprintln!("  로그: PSTMACRO_LOG=debug");
    eprintln!();
    eprintln!("  --cafe 는 URL / vanity(cafe.naver.com/<slug>) / 숫자 cafeId 모두 가능.");
    eprintln!("  --commit 가 없으면 DRY-RUN — 계획만 출력하고 아무것도 게시하지 않습니다.");
    eprintln!();
    eprintln!("정직성: top-N 선택은 프론트엔드 topNArticles(comment-jobs.ts)의 Rust 재구현입니다.");
    eprintln!("        이 예제는 백엔드 왕복(#96 목록 + 댓글 작성)을 검증하며 TS 코드 자체는 검증하지 않습니다.");
    eprintln!();
    eprintln!("주의: --commit 은 실제 카페에 댓글이 작성됩니다. 본인 테스트 카페에만 사용하세요.");
}
