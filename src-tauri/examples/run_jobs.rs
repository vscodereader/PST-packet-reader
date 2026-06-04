//! 오케스트레이터 end-to-end 진단 예제.
//!
//! `send_post.rs`(저수준 단건)와 달리, 이 예제는 [`CafeOrchestrator`]를 통해
//! 새로 추가된 전체 흐름을 실서버로 검증한다:
//!
//!   1) `resolve_cafe_id`  — 카페 URL/vanity/숫자 → 숫자 cafeId 해석 (① 기능)
//!   2) `list_boards`      — 일반 게시판(글쓰기 가능) 목록 조회 (③ 디스커버리)
//!   3) `post_one`         — 선택한 게시판에 실제 글 작성 (②③ 실행)
//!
//! `--cafe`에 **URL이나 vanity 슬러그**(`cafe.naver.com/<name>`)를 그대로 넣어도
//! 1)에서 숫자 cafeId로 해석된다. `--menu`를 생략하면 2)에서 조회된 **첫 번째
//! 일반 게시판**을 자동 선택한다(board_type도 그 게시판 값을 사용).
//!
//! # 사용법
//!
//! ```bash
//! # 로컬 테스트 — 쿠키 파일 직접 지정 (WSL 친화적):
//! PSTMACRO_LIVE_COOKIES_PATH=/tmp/<id>.json \
//! cargo run --example run_jobs -- --cafe 'cafe.naver.com/bluegrayoc3uc'
//!
//! # production — appdata 에 저장된 계정 쿠키 사용:
//! PSTMACRO_LIVE_ACCOUNT_ID=<계정id> \
//! cargo run --example run_jobs -- --cafe 31732304 --menu 1
//!
//! # 게시판을 직접 지정 (menu_id):
//! cargo run --example run_jobs -- --cookies /tmp/<id>.json --cafe 31732304 --menu 1
//! ```
//!
//! 선택 환경변수/플래그: `--subject`(기본 "테스트 제목"), `--body`(기본 "테스트 본문").
//!
//! 주의: 실제 네이버 카페에 글이 작성됩니다. 본인의 테스트 카페에만 사용하세요.
//! 보안: 쿠키 값/헤더는 절대 출력하지 않습니다.

use std::{env, fs};

use pstmacro_lib::auth::{read_account_cookies, read_account_cookies_unchecked};
use pstmacro_lib::naver_cafe::post::cookie_header_from_storage_state;
use pstmacro_lib::naver_cafe::{CafeOrchestrator, PostJob};
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
    // menu_id는 선택 — 생략 시 일반 게시판 목록의 첫 번째를 자동 선택한다.
    let explicit_menu_id: Option<u64> = pick(&args, "--menu", "PSTMACRO_LIVE_MENU_ID").map(|raw| {
        raw.trim().parse().unwrap_or_else(|_| {
            eprintln!("menuId 는 숫자여야 합니다: {raw:?}");
            std::process::exit(2);
        })
    });
    let subject = pick(&args, "--subject", "PSTMACRO_LIVE_SUBJECT")
        .unwrap_or_else(|| "테스트 제목".to_string());
    let body =
        pick(&args, "--body", "PSTMACRO_LIVE_BODY").unwrap_or_else(|| "테스트 본문".to_string());
    let account_label = pick(&args, "--account", "PSTMACRO_LIVE_ACCOUNT_ID")
        .unwrap_or_else(|| "(file)".to_string());

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

    let orchestrator = CafeOrchestrator::new();

    // --- 1) 카페 식별자 해석 (URL/vanity/숫자 → 숫자 cafeId) ---
    println!("=== 1) 카페 해석: {cafe_input:?} ===");
    let cafe_id = match orchestrator.resolve_cafe_id(&cafe_input, cookie).await {
        Ok(id) => {
            println!("해석된 cafeId: {id}");
            id
        }
        Err(e) => {
            eprintln!("카페 해석 실패:");
            eprintln!("{}", serde_json::to_string_pretty(&e).unwrap_or_default());
            std::process::exit(1);
        }
    };
    println!();

    // --- 2) 일반 게시판 목록 조회 ---
    println!("=== 2) 게시판 목록 ===");
    let boards = match orchestrator.list_boards(cafe_id, cookie).await {
        Ok(b) => b,
        Err(e) => {
            eprintln!("게시판 목록 조회 실패:");
            eprintln!("{}", serde_json::to_string_pretty(&e).unwrap_or_default());
            std::process::exit(1);
        }
    };
    if boards.is_empty() {
        eprintln!("글쓰기 가능한 일반 게시판이 없습니다.");
        std::process::exit(1);
    }
    for b in &boards {
        println!(
            "  - menuId={} | boardType={} | {}",
            b.menu_id, b.board_type, b.menu_name
        );
    }
    println!();

    // --- 게시판 선택: --menu 가 있으면 그 게시판, 없으면 첫 번째 ---
    let chosen = match explicit_menu_id {
        Some(menu_id) => boards
            .iter()
            .find(|b| b.menu_id == menu_id)
            .unwrap_or_else(|| {
                eprintln!(
                    "menuId={menu_id} 는 일반 게시판 목록에 없습니다. 위 목록에서 선택하세요."
                );
                std::process::exit(1);
            }),
        None => &boards[0],
    };
    println!(
        "선택된 게시판: menuId={} | boardType={} | {}",
        chosen.menu_id, chosen.board_type, chosen.menu_name
    );
    println!();

    // --- 3) 글 작성 (post_one) ---
    // 참고: 다계정 production 진입점은 run_post_jobs(&[PostJob]) 이며, appdata 의
    // 계정 쿠키를 읽어 N건을 순차 실행한다. 이 예제는 로컬 테스트 호환을 위해
    // 쿠키를 직접 주입하는 post_one 을 사용한다.
    let job = PostJob {
        account_id: account_label,
        cafe: cafe_input.clone(),
        menu_id: chosen.menu_id,
        board_type: chosen.board_type.clone(),
        subject,
        body_text: body,
        tag_list: vec![],
    };

    println!("=== 3) 글 작성 ===");
    println!(
        "요청: cafeId={} menuId={} boardType={} subject={:?}",
        cafe_id, job.menu_id, job.board_type, job.subject
    );
    match orchestrator.post_one(&job, cookie).await {
        Ok(result) => {
            println!("성공: 글이 작성되었습니다.");
            println!("{}", serde_json::to_string_pretty(&result).unwrap());
        }
        Err(post_error) => {
            eprintln!("실패 응답 (쿠키 값은 포함되지 않음):");
            eprintln!("{}", serde_json::to_string_pretty(&post_error).unwrap());
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
    eprintln!("  로컬 테스트: PSTMACRO_LIVE_COOKIES_PATH=/tmp/<id>.json cargo run --example run_jobs -- --cafe 'cafe.naver.com/<slug>'");
    eprintln!("  production: PSTMACRO_LIVE_ACCOUNT_ID=<id> cargo run --example run_jobs -- --cafe 31732304 --menu 1");
    eprintln!("  플래그: --account --cookies --cafe --menu --subject --body");
    eprintln!();
    eprintln!("  --cafe 는 URL / vanity(cafe.naver.com/<slug>) / 숫자 cafeId 모두 가능.");
    eprintln!("  --menu 생략 시 일반 게시판 목록의 첫 번째를 자동 선택.");
    eprintln!();
    eprintln!("주의: 실제 카페에 글이 작성됩니다. 본인 테스트 카페에만 사용하세요.");
}
