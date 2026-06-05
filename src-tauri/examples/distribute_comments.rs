//! 계정별 무작위 댓글 분배 end-to-end 검증 예제 — 이슈 #98.
//!
//! IPC `run_comment_jobs`(`src/ipc/cafes.rs`)가 하는 일을 재현한다:
//!
//!   1) `distribute_comments` — 댓글 풀을 셔플해 계정(타깃)마다 1개씩 배정
//!   2) (--commit 시) 각 계정 쿠키로 `CafeCommentClient`로 실제 댓글 게시
//!
//! # 정직성 안내
//!
//! IPC 핸들러의 분배·조립 코드는 Rust 단위 테스트(`naver_cafe::distribute`)와
//! 코드 리뷰로 커버된다. 이 예제는 그 **백엔드 왕복**(분배 → 실제 게시)을
//! 실서버로 한 번 돌려보기 위한 진단 도구다. 분배는 핸들러와 **동일한**
//! `distribute_comments`를 쓰고, 게시는 핸들러 내부 `run_comment_jobs`가 쓰는
//! 것과 **동일한 전송 클라이언트** `CafeCommentClient`를 직접 호출한다(잡 간
//! 도배 방지 간격도 동일하게 재현). 차이는 DRY-RUN 분기·시드 고정(`--seed`),
//! 그리고 **계정별 쿠키 파일 경로 지정**을 지원한다는 점이다.
//!
//! # 쿠키 소스 — 계정마다 따로 지정 가능
//!
//! `--accounts` 항목은 두 형식을 섞어 쓸 수 있다:
//!
//!   - `id`          — appdata cookies 폴더의 `<id>.json`을 조회(production)
//!   - `id=경로.json` — 그 json 파일을 직접 읽음(WSL/로컬 테스트 친화적)
//!
//! `id`는 출력 라벨 겸 폴더 조회 키로 쓰인다(파일 경로를 줘도 라벨로 필요).
//!
//! # 안전 (DRY-RUN 기본값)
//!
//! 기본은 **DRY-RUN**: 분배 결과("어느 계정 → 어느 댓글")만 출력하고 아무것도
//! 게시하지 않는다(네트워크도 타지 않는다). `--commit`일 때만 실제로 작성한다.
//! `--cafe`/`--article`은 숫자 ID를 받는다(URL 해석은 `send_comment`가 검증).
//!
//! # 사용법
//!
//! ```bash
//! # DRY-RUN(기본) — 계정별 쿠키 파일 경로를 직접 지정, 게시 없음:
//! cargo run --example distribute_comments -- \
//!   --cafe 31732304 --article 9 \
//!   --accounts 'money_lab=/tmp/money_lab.json,invest_king7=/tmp/king.json' \
//!   --content '좋네요|관심종목 추가요'
//!
//! # 폴더 조회 방식(쿠키가 appdata cookies 폴더에 있을 때):
//! cargo run --example distribute_comments -- \
//!   --cafe 31732304 --article 9 --accounts money_lab,invest_king7 --content '댓글1|댓글2'
//!
//! # 시드 고정 — 같은 시드면 같은 분배(재현):
//! ... --accounts a=/tmp/a.json,b=/tmp/b.json --content '댓글1|댓글2' --seed 7
//!
//! # COMMIT — 실제로 댓글 작성(주의!):
//! ... --accounts 'money_lab=/tmp/money_lab.json,invest_king7=/tmp/king.json' --content '좋네요|동의합니다' --commit
//! ```
//!
//! 주의: `--commit`은 실제 네이버 카페에 댓글을 작성합니다. 본인 테스트 카페에만
//! 사용하세요. 보안: 쿠키 값/헤더는 절대 출력하지 않습니다.

use std::time::Duration;
use std::{env, fs};

use pstmacro_lib::auth::read_account_cookies_unchecked;
use pstmacro_lib::naver_cafe::comment::{CafeCommentClient, CommentRequest};
use pstmacro_lib::naver_cafe::distribute::{distribute_comments, mulberry32, seed_from_clock};
use pstmacro_lib::naver_cafe::post::cookie_header_from_storage_state;
use serde_json::Value;
use tokio::time::sleep;

/// 잡 간 간격 — orchestrator `run_comment_jobs`(COMMENT_JOB_DELAY)와 동일하게
/// 같은 글에 연속으로 댓글을 달 때 도배 차단으로 거부되는 것을 피한다.
const COMMENT_JOB_DELAY: Duration = Duration::from_millis(2000);

#[tokio::main]
async fn main() {
    let args: Vec<String> = env::args().collect();
    if args.iter().any(|a| a == "--help" || a == "-h") {
        print_usage();
        return;
    }

    let cafe_id = require_numeric(&args, "--cafe", "PSTMACRO_LIVE_CAFE_ID", "카페 숫자 ID");
    let article_id = require_numeric(
        &args,
        "--article",
        "PSTMACRO_LIVE_ARTICLE_ID",
        "게시글 숫자 ID",
    );

    // 각 항목: "id" 또는 "id=쿠키경로.json" (쉼표 구분).
    let accounts: Vec<(String, Option<String>)> =
        pick(&args, "--accounts", "PSTMACRO_LIVE_ACCOUNTS")
            .map(|s| parse_accounts(&s))
            .unwrap_or_default();
    if accounts.is_empty() {
        eprintln!("계정이 필요합니다 (--accounts 'a=/tmp/a.json,b' — id 또는 id=경로)");
        print_usage();
        std::process::exit(2);
    }

    // 댓글 풀은 '|' 로 구분(댓글 본문에 쉼표가 흔해서 쉼표 구분은 부적절).
    let comments: Vec<String> = pick(&args, "--content", "PSTMACRO_LIVE_CONTENT")
        .map(|s| split_nonempty(&s, '|'))
        .unwrap_or_default();
    if comments.is_empty() {
        eprintln!("댓글 풀이 필요합니다 (--content '댓글1|댓글2')");
        print_usage();
        std::process::exit(2);
    }

    let seed = pick(&args, "--seed", "PSTMACRO_LIVE_SEED").and_then(|s| s.parse::<u32>().ok());
    let commit = args.iter().any(|a| a == "--commit");

    // --- 1) 분배: 핸들러와 동일하게 댓글 풀을 셔플해 계정마다 1개씩 배정 ---
    let used_seed = seed.unwrap_or_else(seed_from_clock);
    let mut rng = mulberry32(used_seed);
    let contents = distribute_comments(accounts.len(), &comments, &mut rng);
    // (id, 쿠키소스, 배정된 댓글) 한 묶음으로 — 출력과 게시가 같은 분배를 쓴다.
    let jobs: Vec<(String, Option<String>, String)> = accounts
        .into_iter()
        .zip(contents)
        .map(|((id, src), content)| (id, src, content))
        .collect();

    println!(
        "=== 댓글 분배 (이슈 #98) — {} ===",
        if commit { "COMMIT" } else { "DRY-RUN" }
    );
    println!(
        "시드: {used_seed}{}",
        if seed.is_some() {
            " (고정)"
        } else {
            " (wall-clock)"
        }
    );
    println!(
        "타깃 {} 계정 × 댓글 풀 {}개 → 카페 {cafe_id} / 글 {article_id} 에 계정마다 1개 배정:",
        jobs.len(),
        comments.len()
    );
    for (i, (id, src, content)) in jobs.iter().enumerate() {
        let src_label = match src {
            Some(path) => format!("파일 {path}"),
            None => "폴더 조회".to_string(),
        };
        println!("  [{}] account={id:<16} ({src_label}) → {content:?}", i + 1);
    }
    println!();

    if !commit {
        println!(
            "DRY-RUN: 아무것도 게시하지 않았습니다. 실제로 작성하려면 --commit 을 추가하세요."
        );
        println!("(주의: --commit 은 실제 네이버 카페에 댓글을 작성합니다)");
        return;
    }

    // --- 2) 게시: 핸들러 내부와 동일한 CafeCommentClient. 계정별 쿠키를 직접 공급 ---
    println!("=== 게시 (--commit) — 실제 네이버 전송 ===");
    let client = CafeCommentClient::new();
    let total = jobs.len();
    let mut ok = 0usize;
    for (i, (id, src, content)) in jobs.iter().enumerate() {
        // 첫 건 이후에는 도배 차단을 피해 간격을 둔다(orchestrator와 동일).
        if i > 0 {
            sleep(COMMENT_JOB_DELAY).await;
        }
        // 쿠키 확보(파일 경로 우선, 없으면 폴더 조회). 보안: 값은 출력하지 않는다.
        let cookies = match load_cookies(id, src.as_deref()) {
            Ok(v) => v,
            Err(e) => {
                println!("  [{}] account={id:<16} ❌ NO_COOKIES: {e}", i + 1);
                continue;
            }
        };
        let Some(header) = cookie_header_from_storage_state(&cookies) else {
            println!(
                "  [{}] account={id:<16} ❌ NO_COOKIES: 네이버 세션 쿠키를 찾지 못함",
                i + 1
            );
            continue;
        };
        let req = CommentRequest {
            cafe_id: cafe_id.clone(),
            article_id: article_id.clone(),
            content: content.clone(),
            sticker_id: None,
        };
        match client.post_comment(&req, Some(header.as_str())).await {
            Ok(result) => {
                ok += 1;
                println!(
                    "  [{}] account={id:<16} ✅ commentId={}",
                    i + 1,
                    result.comment_id
                );
            }
            // CommentError 에는 code/message 만 — 쿠키 값은 절대 포함되지 않는다.
            Err(e) => println!(
                "  [{}] account={id:<16} ❌ {}: {}",
                i + 1,
                e.code,
                e.message
            ),
        }
    }
    println!();
    println!("요약: 성공 {ok}/{total}건");
    if ok < total {
        std::process::exit(1);
    }
}

/// `--accounts` 항목을 (계정 id, 쿠키 파일 경로?) 목록으로 파싱한다.
/// `id=경로` 면 파일 직접 읽기, `id` 만이면 폴더 조회.
fn parse_accounts(s: &str) -> Vec<(String, Option<String>)> {
    s.split(',')
        .map(|p| p.trim())
        .filter(|p| !p.is_empty())
        .map(|item| match item.split_once('=') {
            Some((id, path)) => (id.trim().to_string(), Some(path.trim().to_string())),
            None => (item.to_string(), None),
        })
        .collect()
}

/// 쿠키 JSON 값을 확보한다. 경로가 있으면 그 파일을, 없으면 appdata cookies
/// 폴더의 `<account_id>.json`을 읽는다(만료 검증 없이 원본 그대로).
fn load_cookies(account_id: &str, path: Option<&str>) -> Result<Value, String> {
    match path {
        Some(p) => {
            let text =
                fs::read_to_string(p).map_err(|e| format!("쿠키 파일을 읽지 못함({p}): {e}"))?;
            serde_json::from_str(&text).map_err(|e| format!("쿠키 JSON 파싱 실패({p}): {e}"))
        }
        None => match read_account_cookies_unchecked(account_id) {
            Ok(Some(v)) => Ok(v),
            Ok(None) => Err(format!("cookies 폴더에 '{account_id}.json' 이 없음")),
            Err(e) => Err(format!("쿠키 읽기 실패: {e}")),
        },
    }
}

/// CLI 플래그(우선) 또는 환경변수에서 값을 읽는다.
fn pick(args: &[String], flag: &str, env_key: &str) -> Option<String> {
    args.windows(2)
        .find(|pair| pair[0] == flag)
        .map(|pair| pair[1].clone())
        .or_else(|| env::var(env_key).ok())
}

/// 구분자로 나눠 공백을 제거하고 빈 항목을 버린다.
fn split_nonempty(s: &str, sep: char) -> Vec<String> {
    s.split(sep)
        .map(|p| p.trim())
        .filter(|p| !p.is_empty())
        .map(|p| p.to_string())
        .collect()
}

/// 필수 숫자 인자를 읽어 문자열로 돌려준다(`CommentRequest`는 숫자 문자열 사용).
/// 없거나 숫자가 아니면 사용법을 찍고 종료한다.
fn require_numeric(args: &[String], flag: &str, env_key: &str, label: &str) -> String {
    let raw = pick(args, flag, env_key).unwrap_or_else(|| {
        eprintln!("{label}가 필요합니다 ({flag} 또는 {env_key})");
        print_usage();
        std::process::exit(2);
    });
    if raw.parse::<u64>().is_err() {
        eprintln!("{label}는 숫자여야 합니다: {raw:?}");
        std::process::exit(2);
    }
    raw
}

fn print_usage() {
    eprintln!("usage:");
    eprintln!("  DRY-RUN:  cargo run --example distribute_comments -- --cafe 31732304 --article 9 --accounts 'a=/tmp/a.json,b=/tmp/b.json' --content '댓글1|댓글2'");
    eprintln!("  폴더조회: ... --accounts a,b   (appdata cookies 폴더의 <id>.json)");
    eprintln!("  시드고정: ... --seed 7");
    eprintln!("  COMMIT:   ... --commit   (실제 게시!)");
    eprintln!();
    eprintln!(
        "  --accounts 항목: 'id'(폴더 조회) 또는 'id=쿠키경로.json'(파일 직접) 혼용, 쉼표 구분."
    );
    eprintln!("  --cafe/--article 는 숫자 ID. --content 는 '|' 구분.");
    eprintln!();
    eprintln!("주의: --commit 은 실제 카페에 댓글을 작성합니다. 본인 테스트 카페에만 사용하세요.");
}
