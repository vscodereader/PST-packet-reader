use std::{env, fs};

use pstmacro_lib::auth::{read_account_cookies, read_account_cookies_unchecked};
use pstmacro_lib::naver_cafe::post::cookie_header_from_storage_state;
use pstmacro_lib::naver_cafe::CafeOrchestrator;
use serde_json::Value;
use tracing_subscriber::EnvFilter;

// 가입한 카페 목록 + 각 카페의 "작성 가능한 게시판"을 함께 조회한다.
// (list_joined_cafes.rs 는 카페 목록만 조회 — 이 예제는 카페마다 게시판까지 더 본다.)
//
// 실행방법 (list_joined_cafes.rs 와 동일한 쿠키 확보 방식):
//
//   [production] cookies 폴더의 account_id 쿠키 사용:
//     PSTMACRO_LIVE_ACCOUNT_ID='id' cargo run --example list_joined_cafe_boards
//
//   [로컬 테스트] 쿠키 json 파일을 직접 지정:
//     PSTMACRO_LIVE_COOKIES_PATH='/tmp/hyeonjun1968.json' cargo run --example list_joined_cafe_boards
//
//   플래그: --account <id> | --cookies <path>
//   카페 수 제한(많을 때 일부만): --limit <N> 또는 PSTMACRO_LIMIT
//   로그: PSTMACRO_LOG=debug 로 페이지/요청별 로그 표시
//
// 주의: 카페마다 게시판 API를 1회씩 호출하므로(N+1 요청) 가입 카페가 많으면
//       요청이 많아진다. 테스트 시 --limit 으로 줄여서 보는 것을 권장.
//
// 읽기 전용입니다 — 카페에 아무것도 작성하지 않습니다.
// 보안: 쿠키 값/헤더는 절대 출력하지 않습니다.

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
    if args.iter().any(|a| a == "--help" || a == "-h") {
        print_usage();
        return;
    }

    // 쿠키 확보: (1) 파일 경로 직접 지정 우선, (2) account_id 로 저장된 쿠키 조회
    let cookies_value = match resolve_cookies(&args) {
        Ok(v) => v,
        Err(msg) => {
            eprintln!("{msg}");
            std::process::exit(1);
        }
    };

    // Cookie 헤더 생성 (보안: 값은 절대 출력하지 않음)
    let Some(cookie_header) = cookie_header_from_storage_state(&cookies_value) else {
        eprintln!("쿠키에서 네이버 세션 쿠키를 찾지 못했습니다. (로그인 상태를 확인하세요)");
        std::process::exit(1);
    };
    let cookie = Some(cookie_header.as_str());

    let limit = pick(&args, "--limit", "PSTMACRO_LIMIT").and_then(|s| s.parse::<usize>().ok());

    let orchestrator = CafeOrchestrator::new();

    // 1) 가입 카페 목록
    let cafes = match orchestrator.list_joined_cafes(cookie).await {
        Ok(cafes) => cafes,
        Err(err) => {
            eprintln!("가입 카페 목록 조회 실패:");
            eprintln!("{}", serde_json::to_string_pretty(&err).unwrap());
            std::process::exit(1);
        }
    };

    let total = cafes.len();
    let shown: Vec<_> = match limit {
        Some(n) => cafes.iter().take(n).collect(),
        None => cafes.iter().collect(),
    };
    println!(
        "\n가입 카페 {total}개{}:\n",
        match limit {
            Some(n) if n < total => format!(" (그중 {n}개만 표시)"),
            _ => String::new(),
        }
    );

    // 2) 카페마다 작성 가능 게시판 조회 후 출력
    for c in shown {
        println!("■ {} (cafeId={})", c.cafe_name, c.cafe_id);
        match orchestrator.list_boards(c.cafe_id, cookie).await {
            Ok(boards) if boards.is_empty() => {
                println!("    (작성 가능한 게시판 없음)");
            }
            Ok(boards) => {
                for b in &boards {
                    println!("    - {:<28} (menuId={})", truncate(&b.menu_name, 28), b.menu_id);
                }
            }
            Err(err) => {
                // 한 카페 실패해도 나머지는 계속 본다(휴면/권한 등).
                let code = &err.code;
                println!("    ! 게시판 조회 실패 (code={code})");
            }
        }
        println!();
    }
}

/// 쿠키 값을 확보한다(list_joined_cafes.rs 와 동일한 우선순위).
fn resolve_cookies(args: &[String]) -> Result<Value, String> {
    if let Some(path) = pick(args, "--cookies", "PSTMACRO_LIVE_COOKIES_PATH") {
        println!("쿠키 파일 직접 사용(로컬 테스트): {path}");
        let text =
            fs::read_to_string(&path).map_err(|e| format!("쿠키 파일을 읽지 못했습니다: {e}"))?;
        let value = serde_json::from_str(&text).map_err(|e| format!("쿠키 JSON 파싱 실패: {e}"))?;
        return Ok(value);
    }

    let account_id = pick(args, "--account", "PSTMACRO_LIVE_ACCOUNT_ID").ok_or_else(|| {
        "account_id 가 필요합니다: --account <계정id> 또는 PSTMACRO_LIVE_ACCOUNT_ID".to_string()
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
    eprintln!("  production: PSTMACRO_LIVE_ACCOUNT_ID=id cargo run --example list_joined_cafe_boards");
    eprintln!(
        "  로컬 테스트: PSTMACRO_LIVE_COOKIES_PATH=/tmp/<id>.json cargo run --example list_joined_cafe_boards"
    );
    eprintln!("  플래그: --account <id> | --cookies <path> | --limit <N>");
    eprintln!("  로그: PSTMACRO_LOG=debug 로 페이지/요청별 로그 표시");
    eprintln!();
    eprintln!("가입 카페 + 각 카페의 작성 가능 게시판(menuId/menu명)을 조회합니다.");
    eprintln!("읽기 전용 — 카페에 아무것도 작성하지 않습니다.");
}
