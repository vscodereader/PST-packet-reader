//! 최신글/인기글 top-N × **다계정 무작위 분배** 댓글 백엔드 흐름(end-to-end) 검증 예제.
//!
//! 이슈 [[#97]](top-N 댓글)과 [[#98]](다계정 무작위 분배)의 UI가 구동하는
//! **백엔드 왕복**을 실서버로 검증한다:
//!
//!   1) `resolve_cafe_id`            — 카페 URL/vanity/숫자 → 숫자 cafeId 해석
//!   2) `ArticleListClient`([[#96]]) — 최신글/인기글 목록 조회 ([`SortBy`])
//!   3) top-N 선택                   — 목록 상위 N건을 댓글 대상으로 선정
//!   4) (계정 × 글) 타깃 구성         — 각 계정이 각 글에 댓글을 단다
//!   5) `distribute_comments`([[#98]]) — 댓글 풀을 셔플해 타깃마다 1개씩 배정
//!   6) `CafeCommentClient`          — (--commit 시) 각 타깃에 배정 댓글 작성
//!
//! # 정직성 안내 (중요)
//!
//! #97의 top-N 선택과 #98의 분배 책임은 각각 프론트엔드 TS / 백엔드 Rust에
//! 있다. top-N 선택은 TS `topNArticles`([`comment-jobs.ts`])의 graceful
//! fallback(`articles.iter().take(n)`)을 Rust로 재현하고, 분배는 핸들러와
//! **동일한** [`distribute_comments`](naver_cafe::distribute)를 그대로 호출한다
//! (알고리즘 재구현 없음). 게시는 핸들러가 쓰는 것과 동일한
//! `CafeCommentClient`로 도배 방지 간격까지 동일하게 재현한다.
//!
//! # 쿠키 소스 — 계정마다 따로 지정 가능
//!
//! `--accounts` 항목은 두 형식을 섞어 쓸 수 있다(쉼표 구분):
//!
//!   - `id`          — appdata cookies 폴더의 `<id>.json`을 조회(production)
//!   - `id=경로.json` — 그 json 파일을 직접 읽음(WSL/로컬 테스트 친화적)
//!
//! `--accounts`가 없으면 단일 계정 하위호환 경로(`--account`/`--cookies` +
//! 동등 env)로 폴백한다.
//!
//! # 안전 (DRY-RUN 기본값)
//!
//! 기본은 **DRY-RUN**이다: 목록을 조회하고 top-N을 골라 타깃별 **배정 댓글
//! 계획만 출력**하며 아무것도 게시하지 않는다. `--commit`을 줄 때에만 실제로
//! 댓글을 작성한다(타깃 간 2초 간격).
//!
//! # 사용법
//!
//! ```bash
//! # 다계정 DRY-RUN(기본) — 최신글 상위 3개 × 두 계정, 댓글 풀에서 무작위 1개씩:
//! cargo run --example comment_on_articles -- \
//!   --cafe 31732304 --sort latest --count 3 \
//!   --accounts 'money_lab=/tmp/money_lab.json,invest_king7=/tmp/king.json' \
//!   --content '좋네요|관심종목 추가요|잘 봤습니다' --seed 7
//!
//! # 단일 계정 하위호환 — 쿠키 파일 직접 지정, 최신글 상위 3개:
//! PSTMACRO_LIVE_COOKIES_PATH=/tmp/<id>.json \
//! cargo run --example comment_on_articles -- --cafe 'cafe.naver.com/<slug>'
//!
//! # COMMIT — 실제로 댓글을 작성(주의!), production 계정 폴더 조회:
//! cargo run --example comment_on_articles -- \
//!   --cafe 31732304 --count 2 --accounts money_lab,invest_king7 \
//!   --content '좋네요|동의합니다' --commit
//! ```
//!
//! 플래그: `--accounts 'id=경로,id'`(다계정) 또는 `--account`/`--cookies`(단일,
//!   동등 env 포함), `--cafe`(필수, URL/vanity/숫자), `--sort latest|popular`(기본
//!   latest), `--count <N>`(기본 3), `--content <text>`('|' 구분 댓글 풀, 기본
//!   "테스트 댓글"), `--seed <u32>`(고정 시 결정적), `--commit`(없으면 DRY-RUN),
//!   `--help`/`-h`. 로그: `PSTMACRO_LOG=debug`.
//!
//! 주의: `--commit` 은 **실제 네이버 카페에 댓글을 작성**합니다. 본인의 테스트
//! 카페에만 사용하세요.
//! 보안: 쿠키 값/헤더는 절대 출력하지 않습니다.

use std::collections::HashMap;
use std::time::Duration;
use std::{env, fs};

use pstmacro_lib::auth::{read_account_cookies, read_account_cookies_unchecked};
use pstmacro_lib::naver_cafe::comment::{CafeCommentClient, CommentRequest};
use pstmacro_lib::naver_cafe::distribute::{distribute_comments, mulberry32, seed_from_clock};
use pstmacro_lib::naver_cafe::post::cookie_header_from_storage_state;
use pstmacro_lib::naver_cafe::{ArticleListClient, CafeOrchestrator, SortBy};
use serde_json::Value;
use tokio::time::sleep;
use tracing_subscriber::EnvFilter;

/// top-N 기본값 — 목록 상위 몇 개를 댓글 대상으로 삼을지.
const DEFAULT_COUNT: usize = 3;

/// 타깃 간 간격 — orchestrator `run_comment_jobs`(COMMENT_JOB_DELAY)와 동일하게
/// 같은 글에 연속으로 댓글을 달 때 도배 차단으로 거부되는 것을 피한다.
const COMMENT_JOB_DELAY: Duration = Duration::from_millis(2000);

/// 한 댓글이 달릴 (계정 × 글) 타깃. 분배·출력·게시가 모두 이 목록을 공유한다.
#[derive(Debug, Clone, PartialEq)]
struct Target {
    /// 계정 라벨(겸 폴더 조회 키).
    account_id: String,
    /// 계정별 쿠키 파일 경로(없으면 폴더 조회).
    cookies_path: Option<String>,
    /// 댓글 대상 게시글 숫자 ID.
    article_id: u64,
    /// 출력용 게시글 제목.
    subject: String,
}

/// 한 타깃에 배정된 댓글까지 묶은 계획 항목.
#[derive(Debug, Clone, PartialEq)]
struct Plan {
    target: Target,
    content: String,
}

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
    // 댓글 풀은 '|' 로 구분(댓글 본문에 쉼표가 흔해서 쉼표 구분은 부적절).
    // 하위호환: 구분자가 없으면 단일 댓글 풀이 되어 모든 타깃이 같은 댓글을 받는다.
    let comments: Vec<String> = pick(&args, "--content", "PSTMACRO_LIVE_CONTENT")
        .map(|s| split_nonempty(&s, '|'))
        .unwrap_or_else(|| vec!["테스트 댓글".to_string()]);
    if comments.is_empty() {
        eprintln!("댓글 풀이 비어 있습니다 (--content '댓글1|댓글2')");
        print_usage();
        std::process::exit(2);
    }
    let seed = pick(&args, "--seed", "PSTMACRO_LIVE_SEED").and_then(|s| s.parse::<u32>().ok());
    // --commit 가 없으면 DRY-RUN(아무것도 게시하지 않음).
    let commit = args.iter().any(|a| a == "--commit");

    // 계정 목록: --accounts(다계정) 우선, 없으면 단일 계정 하위호환 경로로 폴백.
    let accounts = match resolve_accounts(&args) {
        Ok(v) => v,
        Err(msg) => {
            eprintln!("{msg}");
            print_usage();
            std::process::exit(2);
        }
    };

    // 목록 조회·식별자 해석은 첫 계정의 쿠키로 수행한다(같은 카페를 공유).
    let (lead_id, lead_path) = &accounts[0];
    let lead_cookies = match load_cookies(lead_id, lead_path.as_deref()) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("첫 계정 '{lead_id}' 쿠키 확보 실패: {e}");
            std::process::exit(1);
        }
    };
    // 보안: 쿠키 헤더 값은 절대 출력하지 않는다.
    let Some(lead_header) = cookie_header_from_storage_state(&lead_cookies) else {
        eprintln!(
            "첫 계정 쿠키에서 네이버 세션 쿠키를 찾지 못했습니다. (로그인 상태를 확인하세요)"
        );
        std::process::exit(1);
    };
    let lead_cookie = Some(lead_header.as_str());

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
        .resolve_cafe_id(&cafe_input, lead_cookie)
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
        .fetch_article_list(&cafe_id, sort_by, 1, lead_cookie)
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
    // 목록이 N보다 짧으면 있는 만큼만 사용한다.
    let selected: Vec<(u64, String)> = list
        .articles
        .iter()
        .take(count)
        .map(|a| (a.article_id, a.subject.clone()))
        .collect();
    println!(
        "=== 3) top-N 선택: 요청 count={count}, 선택됨 {}건 ===",
        selected.len()
    );
    if selected.is_empty() {
        eprintln!("선택된 게시글이 없습니다 (목록이 비어 있거나 --count 0). 작업 없이 종료합니다.");
        std::process::exit(1);
    }

    // --- 4) (계정 × 글) 타깃 구성 + 5) 분배: 타깃마다 댓글 풀에서 1개씩 배정 ---
    let targets = build_targets(&accounts, &selected);
    let used_seed = seed.unwrap_or_else(seed_from_clock);
    let mut rng = mulberry32(used_seed);
    let plans = plan_comments(targets, &comments, &mut rng);

    println!(
        "=== 4/5) 타깃 {} (계정 {} × 글 {}) — 댓글 풀 {}개에서 무작위 1개씩 배정 ===",
        plans.len(),
        accounts.len(),
        selected.len(),
        comments.len()
    );
    println!(
        "시드: {used_seed}{}",
        if seed.is_some() {
            " (고정)"
        } else {
            " (wall-clock)"
        }
    );
    for (i, p) in plans.iter().enumerate() {
        let src_label = match &p.target.cookies_path {
            Some(path) => format!("파일 {path}"),
            None => "폴더 조회".to_string(),
        };
        println!(
            "  [{}] account={:<16} ({src_label}) → cafeId={cafe_id} articleId={} subject={:?}",
            i + 1,
            p.target.account_id,
            p.target.article_id,
            p.target.subject
        );
        println!("      배정 댓글: {:?}", p.content);
    }
    println!();

    // --- 6) 게시 (DRY-RUN 이면 건너뛰고, --commit 이면 각 타깃에 배정 댓글 작성) ---
    if !commit {
        println!("DRY-RUN: 아무것도 게시하지 않았습니다. 실제 게시하려면 --commit 을 추가하세요.");
        return;
    }

    println!("=== 6) 댓글 작성 (COMMIT) — 실제 네이버 전송 ===");
    let client = CafeCommentClient::new();
    let total = plans.len();
    let mut ok_count = 0usize;
    let mut fail_count = 0usize;
    // 계정별 세션 헤더를 한 번만 계산해 재사용한다(같은 계정이 여러 타깃을 가질 때
    // 쿠키 파일 중복 I/O 방지). 실패도 캐시해 재시도하지 않는다. 보안: 값은 출력 안 함.
    let mut header_cache: HashMap<String, Result<String, String>> = HashMap::new();
    for (i, p) in plans.iter().enumerate() {
        // 첫 건 이후에는 도배 차단을 피해 간격을 둔다(orchestrator와 동일).
        if i > 0 {
            sleep(COMMENT_JOB_DELAY).await;
        }
        let label = format!(
            "[{}/{}] account={:<16} articleId={}",
            i + 1,
            total,
            p.target.account_id,
            p.target.article_id
        );
        // 계정별 쿠키→세션 헤더(파일 경로 우선, 없으면 폴더 조회)를 캐시에서 가져오거나
        // 최초 1회 계산한다. 보안: 값은 출력하지 않는다.
        let header = match header_cache
            .entry(p.target.account_id.clone())
            .or_insert_with(|| {
                load_cookies(&p.target.account_id, p.target.cookies_path.as_deref())
                    .map_err(|e| format!("NO_COOKIES: {e}"))
                    .and_then(|cookies| {
                        cookie_header_from_storage_state(&cookies)
                            .ok_or_else(|| "NO_COOKIES: 네이버 세션 쿠키를 찾지 못함".to_string())
                    })
            }) {
            Ok(h) => h.clone(),
            Err(e) => {
                fail_count += 1;
                eprintln!("  실패 {label} ❌ {e}");
                continue;
            }
        };
        let req = CommentRequest {
            cafe_id: cafe_id.clone(),
            article_id: p.target.article_id.to_string(),
            content: p.content.clone(),
            sticker_id: None,
        };
        match client.post_comment(&req, Some(header.as_str())).await {
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
    println!("완료: 성공 {ok_count}건, 실패 {fail_count}건 (타깃 {total}건).");
    if fail_count > 0 {
        std::process::exit(1);
    }
}

/// (계정 × 글) 데카르트 곱으로 타깃 목록을 만든다. 계정 우선 순회라 같은 계정의
/// 타깃이 인접한다(출력 가독성). `selected`는 (articleId, subject) top-N.
fn build_targets(accounts: &[(String, Option<String>)], selected: &[(u64, String)]) -> Vec<Target> {
    accounts
        .iter()
        .flat_map(|(id, src)| {
            selected.iter().map(move |(article_id, subject)| Target {
                account_id: id.clone(),
                cookies_path: src.clone(),
                article_id: *article_id,
                subject: subject.clone(),
            })
        })
        .collect()
}

/// 타깃마다 댓글 풀에서 1개씩 배정한다. 분배는 핸들러와 동일한
/// `distribute_comments`를 그대로 쓴다(알고리즘 재구현 없음). 주입된 `rng`만으로
/// 결과가 결정되므로 같은 시드면 같은 배정이다.
fn plan_comments(
    targets: Vec<Target>,
    comments: &[String],
    rng: &mut impl FnMut() -> f64,
) -> Vec<Plan> {
    let contents = distribute_comments(targets.len(), comments, rng);
    targets
        .into_iter()
        .zip(contents)
        .map(|(target, content)| Plan { target, content })
        .collect()
}

/// 계정 목록을 확보한다. `--accounts`(다계정)가 있으면 그것을, 없으면 단일 계정
/// 하위호환 경로(`--account`/`--cookies` + 동등 env)를 `[(id, path?)]` 하나로
/// 변환한다. 어느 쪽도 없으면 에러.
fn resolve_accounts(args: &[String]) -> Result<Vec<(String, Option<String>)>, String> {
    if let Some(s) = pick(args, "--accounts", "PSTMACRO_LIVE_ACCOUNTS") {
        let accounts = parse_accounts(&s);
        if accounts.is_empty() {
            return Err(
                "계정이 필요합니다 (--accounts 'a=/tmp/a.json,b' — id 또는 id=경로)".into(),
            );
        }
        return Ok(accounts);
    }

    // 단일 계정 하위호환: --cookies(파일 직접) 우선, 없으면 --account(폴더 조회).
    if let Some(path) = pick(args, "--cookies", "PSTMACRO_LIVE_COOKIES_PATH") {
        println!("쿠키 파일 직접 사용(단일 계정, 로컬 테스트): {path}");
        return Ok(vec![("local".to_string(), Some(path))]);
    }
    if let Some(id) = pick(args, "--account", "PSTMACRO_LIVE_ACCOUNT_ID") {
        // 폴더 조회 단일 계정 — 쿠키 상태를 한 번 점검해 라벨로 출력한다.
        match read_account_cookies(&id) {
            Ok(Some(_)) => println!("쿠키 상태: 유효(valid)"),
            Ok(None) => println!("쿠키 상태: 없음/만료(invalid) — 성공하려면 다시 로그인하세요"),
            Err(e) => println!("쿠키 상태 확인 실패: {e}"),
        }
        return Ok(vec![(id, None)]);
    }

    Err("계정/쿠키 소스가 필요합니다: --accounts 'a=/tmp/a.json,b'\n또는 단일 계정 --cookies <경로> / --account <id> (+ 동등 env)".into())
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

fn print_usage() {
    eprintln!("usage:");
    eprintln!("  다계정 DRY-RUN: cargo run --example comment_on_articles -- --cafe 31732304 --count 3 --accounts 'a=/tmp/a.json,b=/tmp/b.json' --content '댓글1|댓글2|댓글3'");
    eprintln!("  폴더 조회:      ... --accounts a,b   (appdata cookies 폴더의 <id>.json)");
    eprintln!("  시드 고정:      ... --seed 7   (같은 시드면 같은 분배)");
    eprintln!("  단일 계정:      PSTMACRO_LIVE_COOKIES_PATH=/tmp/<id>.json cargo run --example comment_on_articles -- --cafe 'cafe.naver.com/<slug>'");
    eprintln!("  COMMIT(실게시): ... --commit");
    eprintln!();
    eprintln!("  플래그: --accounts('id=경로,id') 또는 --account/--cookies(단일) --cafe --sort(latest|popular) --count --content('|' 구분 풀) --seed --commit");
    eprintln!("  로그: PSTMACRO_LOG=debug");
    eprintln!();
    eprintln!("  --cafe 는 URL / vanity(cafe.naver.com/<slug>) / 숫자 cafeId 모두 가능.");
    eprintln!("  --commit 가 없으면 DRY-RUN — 타깃별 배정 댓글 계획만 출력하고 아무것도 게시하지 않습니다.");
    eprintln!();
    eprintln!("정직성: top-N 선택은 프론트엔드 topNArticles(comment-jobs.ts)의 Rust 재현,");
    eprintln!("        분배는 핸들러와 동일한 distribute_comments(#98)를 그대로 호출합니다.");
    eprintln!();
    eprintln!("주의: --commit 은 실제 카페에 댓글이 작성됩니다. 본인 테스트 카페에만 사용하세요.");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn accts(items: &[(&str, Option<&str>)]) -> Vec<(String, Option<String>)> {
        items
            .iter()
            .map(|(id, p)| (id.to_string(), p.map(str::to_string)))
            .collect()
    }

    fn arts(items: &[(u64, &str)]) -> Vec<(u64, String)> {
        items.iter().map(|(id, s)| (*id, s.to_string())).collect()
    }

    fn pool(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn build_targets_is_account_then_article_cartesian() {
        let accounts = accts(&[("a", Some("/tmp/a.json")), ("b", None)]);
        let selected = arts(&[(10, "first"), (20, "second")]);
        let targets = build_targets(&accounts, &selected);
        // 계정 × 글 = 2 × 2 = 4, 계정 우선 순회.
        assert_eq!(
            targets,
            vec![
                Target {
                    account_id: "a".into(),
                    cookies_path: Some("/tmp/a.json".into()),
                    article_id: 10,
                    subject: "first".into(),
                },
                Target {
                    account_id: "a".into(),
                    cookies_path: Some("/tmp/a.json".into()),
                    article_id: 20,
                    subject: "second".into(),
                },
                Target {
                    account_id: "b".into(),
                    cookies_path: None,
                    article_id: 10,
                    subject: "first".into(),
                },
                Target {
                    account_id: "b".into(),
                    cookies_path: None,
                    article_id: 20,
                    subject: "second".into(),
                },
            ]
        );
    }

    #[test]
    fn plan_comments_assigns_one_per_target_and_is_seed_deterministic() {
        let accounts = accts(&[("a", None), ("b", None)]);
        let selected = arts(&[(10, "x"), (20, "y")]);
        let comments = pool(&["c1", "c2", "c3", "c4"]);

        let first = plan_comments(
            build_targets(&accounts, &selected),
            &comments,
            &mut mulberry32(42),
        );
        let second = plan_comments(
            build_targets(&accounts, &selected),
            &comments,
            &mut mulberry32(42),
        );

        // 타깃 4개 각각에 댓글 1개, 같은 시드면 완전히 동일한 배정.
        assert_eq!(first.len(), 4);
        assert_eq!(first, second);
        assert!(first.iter().all(|p| comments.contains(&p.content)));
    }

    #[test]
    fn plan_comments_routes_through_the_shuffle() {
        // 단일 타깃 + rng=[0]: 2원소 Fisher–Yates가 풀을 뒤집어 ["x","y"]→["y","x"],
        // 따라서 첫(유일) 타깃은 셔플된 "y"를 받아야 한다(입력 순서 "x"가 아님).
        let accounts = accts(&[("a", None)]);
        let selected = arts(&[(10, "only")]);
        let comments = pool(&["x", "y"]);
        let mut seq = {
            let mut i = 0usize;
            let vals = vec![0.0f64];
            move || {
                let v = vals[i % vals.len()];
                i += 1;
                v
            }
        };
        let plans = plan_comments(build_targets(&accounts, &selected), &comments, &mut seq);
        assert_eq!(plans.len(), 1);
        assert_eq!(plans[0].content, "y");
    }
}
