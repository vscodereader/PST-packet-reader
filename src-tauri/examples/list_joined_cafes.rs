use std::{env, fs};

use pstmacro_lib::auth::{read_account_cookies, read_account_cookies_unchecked};
use pstmacro_lib::naver_cafe::post::cookie_header_from_storage_state;
use pstmacro_lib::naver_cafe::JoinedCafesClient;
use serde_json::Value;
use tracing_subscriber::EnvFilter;

// 실행방법 (send_post.rs 와 동일한 쿠키 확보 방식)
//
//   [production] cookies 폴더에 저장된 각 계정의 쿠키 json 을 account_id 로 조회:
//     PSTMACRO_LIVE_ACCOUNT_ID='id' cargo run --example list_joined_cafes
//   (위치: %LOCALAPPDATA%\pstmacro\cookies\<account_id>.json)
//
//   [로컬 테스트] 쿠키 json 파일을 직접 가리켜 사용 (appdata 불필요):
//     PSTMACRO_LIVE_COOKIES_PATH='/tmp/hyeonjun1968.json' cargo run --example list_joined_cafes
//
//   플래그로도 지정 가능: --account <id> | --cookies <path>
//   로그 레벨: PSTMACRO_LOG=debug 로 페이지별 조회 로그까지 볼 수 있습니다.
//
// 읽기 전용입니다 — 카페에 아무것도 작성하지 않습니다.
// 보안: 쿠키 값/헤더는 절대 출력하지 않습니다.

#[tokio::main]
async fn main() {
    // 진단용 stdout 로깅(앱의 파일 로깅과 별개). PSTMACRO_LOG 로 레벨 제어.
    let _ = tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_env("PSTMACRO_LOG").unwrap_or_else(|_| EnvFilter::new("info")))
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

    let client = JoinedCafesClient::new();
    match client.fetch_joined_cafes(Some(cookie_header.as_str())).await {
        Ok(cafes) => {
            println!("\n가입 카페 {}개:\n", cafes.len());
            println!(
                "{:>10}  {:<32}  {:<20}  {:<12}  {:>4}  {:>4}",
                "cafeId", "카페명", "슬러그", "등급", "관리", "휴면"
            );
            println!("{}", "-".repeat(92));
            for c in &cafes {
                println!(
                    "{:>10}  {:<32}  {:<20}  {:<12}  {:>4}  {:>4}",
                    c.cafe_id,
                    truncate(&c.cafe_name, 32),
                    truncate(&c.cafe_url, 20),
                    truncate(&c.member_levelname, 12),
                    if c.managing_cafe { "O" } else { "-" },
                    if c.dormant_cafe { "O" } else { "-" },
                );
            }
            println!();
        }
        Err(err) => {
            eprintln!("실패 응답:");
            // 오류 봉투에는 http_status/서버 원문(api_error_message)이 담김. 쿠키는 없음.
            eprintln!("{}", serde_json::to_string_pretty(&err).unwrap());
            std::process::exit(1);
        }
    }
}

/// 쿠키 값을 확보한다(send_post.rs 와 동일한 우선순위).
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
    eprintln!("  production: PSTMACRO_LIVE_ACCOUNT_ID=id cargo run --example list_joined_cafes");
    eprintln!("  로컬 테스트: PSTMACRO_LIVE_COOKIES_PATH=/tmp/<id>.json cargo run --example list_joined_cafes");
    eprintln!("  플래그: --account <id> | --cookies <path>");
    eprintln!("  로그: PSTMACRO_LOG=debug 로 페이지별 조회 로그 표시");
    eprintln!();
    eprintln!("읽기 전용 — 카페에 아무것도 작성하지 않습니다.");
}
