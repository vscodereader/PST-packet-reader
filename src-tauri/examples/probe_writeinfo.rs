//! 네이버 카페 에디터 사전 GET 요청 진단 예제.
//!
//! 게시글 작성 POST 가 HTTP 500 / errorCode 10404 를 반환할 때,
//! 동일한 쿠키와 브라우저 헤더로 에디터 관련 GET 엔드포인트를 호출하여
//! (a) 쿠키가 GET 에서도 유효한지,
//! (b) POST 만 차단되는지,
//! (c) write-info / menus / form / heads 스키마를 확인한다.
//!
//! # 사용법
//!
//! ```bash
//! # 로컬 테스트 — 쿠키 파일 직접 지정 (WSL 친화적):
//! PSTMACRO_LIVE_COOKIES_PATH=/tmp/<id>.json \
//! PSTMACRO_LIVE_CAFE_ID=12345 \
//! PSTMACRO_LIVE_MENU_ID=1 \
//! cargo run --example probe_writeinfo
//!
//! # production — appdata 에 저장된 계정 쿠키 사용:
//! PSTMACRO_LIVE_ACCOUNT_ID=<계정id> \
//! PSTMACRO_LIVE_CAFE_ID=12345 \
//! PSTMACRO_LIVE_MENU_ID=1 \
//! cargo run --example probe_writeinfo
//!
//! # 플래그로도 지정 가능:
//! cargo run --example probe_writeinfo -- --cookies /tmp/<id>.json --cafe 12345 --menu 1
//! ```
//!
//! **주의:** 실제 네이버 서버에 인증된 GET 요청을 전송합니다.
//! 읽기 전용이지만 실제 요청이므로 본인 계정의 쿠키만 사용하세요.

use std::{env, fs};

use pstmacro_lib::auth::{read_account_cookies, read_account_cookies_unchecked};
use pstmacro_lib::naver_cafe::post::{cookie_header_from_storage_state, BROWSER_USER_AGENT};
use serde_json::Value;

/// 응답 바디를 최대 1500 자로 잘라낸다. 잘린 경우 끝에 주석을 추가한다.
fn truncate(s: &str) -> String {
    const MAX: usize = 1500;
    if s.len() <= MAX {
        s.to_string()
    } else {
        let cutoff = s
            .char_indices()
            .take_while(|(i, _)| *i < MAX)
            .last()
            .map(|(i, c)| i + c.len_utf8())
            .unwrap_or(MAX);
        format!("{} [... 1500자 초과로 잘림]", &s[..cutoff])
    }
}

#[tokio::main]
async fn main() {
    let args: Vec<String> = env::args().collect();
    if args.iter().any(|a| a == "--help" || a == "-h") {
        print_usage();
        return;
    }

    let cafe_id = pick(&args, "--cafe", "PSTMACRO_LIVE_CAFE_ID").unwrap_or_else(|| {
        eprintln!("cafeId 가 필요합니다 (--cafe 또는 PSTMACRO_LIVE_CAFE_ID)");
        print_usage();
        std::process::exit(2);
    });
    let menu_id_raw = pick(&args, "--menu", "PSTMACRO_LIVE_MENU_ID").unwrap_or_else(|| {
        eprintln!("menuId 가 필요합니다 (--menu 또는 PSTMACRO_LIVE_MENU_ID)");
        print_usage();
        std::process::exit(2);
    });
    let menu_id: u64 = menu_id_raw.trim().parse().unwrap_or_else(|_| {
        eprintln!("menuId 는 숫자여야 합니다: {menu_id_raw:?}");
        std::process::exit(2);
    });

    // 쿠키 확보: (1) 파일 경로 직접 지정 우선, (2) account_id 로 저장된 쿠키 조회
    let cookies_value = match resolve_cookies(&args) {
        Ok(v) => v,
        Err(msg) => {
            eprintln!("{msg}");
            std::process::exit(1);
        }
    };

    // Cookie 헤더 생성
    // 보안: 쿠키 헤더 값은 절대 출력하지 않는다 — 사용자의 인증 자격 증명이다.
    let Some(cookie_header) = cookie_header_from_storage_state(&cookies_value) else {
        eprintln!("쿠키에서 네이버 세션 쿠키를 찾지 못했습니다. (로그인 상태를 확인하세요)");
        std::process::exit(1);
    };

    let referer = format!(
        "https://cafe.naver.com/ca-fe/cafes/{cafe_id}/articles/write?boardType=L"
    );

    // 프로브할 엔드포인트 목록
    let endpoints: Vec<(&str, String)> = vec![
        (
            "GET (editor write-info)",
            format!(
                "https://apis.cafe.naver.com/editor/v2/cafes/{cafe_id}/editor?menuId={menu_id}&from=pc"
            ),
        ),
        (
            "GET (cafeinfo menus)",
            format!(
                "https://apis.naver.com/cafe-web/cafe-cafeinfo-api/v1.0/cafes/{cafe_id}/editor/menus"
            ),
        ),
        (
            "GET (cafeinfo menu form)",
            format!(
                "https://apis.naver.com/cafe-web/cafe-cafeinfo-api/v1.0/cafes/{cafe_id}/editor/menus/{menu_id}/form"
            ),
        ),
        (
            "GET (editor heads)",
            format!(
                "https://apis.cafe.naver.com/editor/v1.0/cafes/{cafe_id}/menus/{menu_id}/heads"
            ),
        ),
    ];

    println!("=== probe_writeinfo: 에디터 GET 엔드포인트 진단 ===");
    println!("cafeId={cafe_id}, menuId={menu_id}");
    println!("User-Agent: {BROWSER_USER_AGENT}");
    println!("쿠키 상태: 로드 완료 (값은 보안상 출력하지 않음)");
    println!();

    // raw reqwest 클라이언트 사용 — 프로덕션 클라이언트에 메서드 추가 없음
    let http = reqwest::Client::new();

    for (label, url) in &endpoints {
        println!("--- {label} ---");
        println!("URL: {url}");

        let result = http
            .get(url)
            .header("Origin", "https://cafe.naver.com")
            .header("Referer", &referer)
            .header("User-Agent", BROWSER_USER_AGENT)
            // apis.cafe.naver.com/editor/* 엔드포인트가 이 헤더를 요구한다.
            // 없으면 cafeProductHeaderType: NONE 오류가 발생한다.
            .header("x-cafe-product", "pc")
            // 보안: 쿠키 값은 절대 출력하지 않는다 — 아래 header() 호출만 허용
            .header("Cookie", &cookie_header)
            .send()
            .await;

        match result {
            Ok(resp) => {
                let status = resp.status();
                println!("HTTP 상태: {status}");
                match resp.text().await {
                    Ok(body) => {
                        let display = truncate(&body);
                        println!("응답 바디:\n{display}");
                    }
                    Err(e) => {
                        println!("바디 읽기 실패: {e}");
                    }
                }
            }
            Err(e) => {
                println!("요청 실패: {e}");
            }
        }

        println!();
    }

    println!("=== 진단 완료 ===");
}

/// 쿠키 값을 확보한다.
///
/// - `--cookies`/`PSTMACRO_LIVE_COOKIES_PATH` 가 있으면 그 json 파일을 직접 읽는다.
/// - 없으면 `--account`/`PSTMACRO_LIVE_ACCOUNT_ID` 로 appdata 쿠키를 조회한다.
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
        Ok(None) => println!("쿠키 상태: 없음/만료(invalid) — 다시 로그인하세요"),
        Err(e) => println!("쿠키 상태 확인 실패: {e}"),
    }

    match read_account_cookies_unchecked(&account_id) {
        Ok(Some(v)) => Ok(v),
        Ok(None) => Err(format!("계정 '{account_id}' 의 쿠키 파일이 없습니다.")),
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
    eprintln!(
        "  로컬 테스트: PSTMACRO_LIVE_COOKIES_PATH=/tmp/<id>.json PSTMACRO_LIVE_CAFE_ID=12345 PSTMACRO_LIVE_MENU_ID=1 cargo run --example probe_writeinfo"
    );
    eprintln!(
        "  production: PSTMACRO_LIVE_ACCOUNT_ID=<id> PSTMACRO_LIVE_CAFE_ID=12345 PSTMACRO_LIVE_MENU_ID=1 cargo run --example probe_writeinfo"
    );
    eprintln!("  플래그: --cookies --account --cafe --menu");
    eprintln!();
    eprintln!("주의: 실제 네이버 서버에 인증된 GET 요청을 전송합니다 (읽기 전용).");
}
