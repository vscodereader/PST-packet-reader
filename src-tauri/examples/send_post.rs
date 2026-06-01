use std::{env, fs};

use pstmacro_lib::auth::{read_account_cookies, read_account_cookies_unchecked};
use pstmacro_lib::naver_cafe::post::{
    build_article_write_body_with_content, cookie_header_from_storage_state, CafeHttpClient,
    PostRequest, SequentialIdProvider,
};
use serde_json::Value;

// 실행방법 (auto_login.rs 와 동일하게 cargo run --example 로 실행)
//
//   [production] cookies 폴더에 저장된 각 계정의 쿠키 json 을 account_id 로 조회:
//     PSTMACRO_LIVE_ACCOUNT_ID='id' \
//     PSTMACRO_LIVE_CAFE_ID='12345' \
//     PSTMACRO_LIVE_MENU_ID='1' \
//     cargo run --example send_post
//   (위치: %LOCALAPPDATA%\pstmacro\cookies\<account_id>.json — appdata 경로는 배포 후 검증)
//
//   [로컬 테스트] 쿠키 json 파일을 직접 가리켜 사용 (appdata 불필요):
//     PSTMACRO_LIVE_COOKIES_PATH='/tmp/hyeonjun1968.json' \
//     PSTMACRO_LIVE_CAFE_ID='12345' \
//     PSTMACRO_LIVE_MENU_ID='1' \
//     cargo run --example send_post
//
//   플래그로도 지정 가능: --account --cookies --cafe --menu --subject --body
//   선택: PSTMACRO_LIVE_SUBJECT(기본 "테스트 제목"), PSTMACRO_LIVE_BODY(기본 "테스트 본문")
//
// 주의: 실제 네이버 카페에 글이 작성됩니다. 본인의 테스트 카페에만 사용하세요.
// 보안: 쿠키 값/헤더는 절대 출력하지 않습니다.

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
    let subject =
        pick(&args, "--subject", "PSTMACRO_LIVE_SUBJECT").unwrap_or_else(|| "테스트 제목".to_string());
    let body =
        pick(&args, "--body", "PSTMACRO_LIVE_BODY").unwrap_or_else(|| "테스트 본문".to_string());

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

    println!("요청 대상: cafeId={cafe_id}, menuId={menu_id}, subject={subject:?}");

    let request = PostRequest {
        cafe_id: cafe_id.clone(),
        menu_id,
        subject,
        body_text: body,
        tag_list: vec![],
        open: None,
        naver_open: None,
        external_open: None,
        enable_comment: None,
        enable_scrap: None,
        enable_copy: None,
    };

    let body = match build_article_write_body_with_content(&request, &mut SequentialIdProvider::default())
    {
        Ok(b) => b,
        Err(e) => {
            eprintln!("요청 본문 생성 실패: {e}");
            std::process::exit(1);
        }
    };

    let client = CafeHttpClient::new();
    match client
        .post_article(&cafe_id, menu_id, &body, Some(cookie_header.as_str()))
        .await
    {
        Ok(result) => {
            println!("성공: 글이 작성되었습니다.");
            println!("{}", serde_json::to_string_pretty(&result).unwrap());
        }
        Err(post_error) => {
            eprintln!("실패 응답:");
            // PostError 에는 http_status 와 서버 원문 바디(api_error_message)가 담김. 쿠키는 없음.
            eprintln!("{}", serde_json::to_string_pretty(&post_error).unwrap());
            std::process::exit(1);
        }
    }
}

/// 쿠키 값을 확보한다.
///
/// - [로컬 테스트] `--cookies`/`PSTMACRO_LIVE_COOKIES_PATH` 가 있으면 그 json 파일을 직접 읽는다
///   (appdata 경로는 배포 후에야 검증 가능하므로 로컬 테스트용 우회 경로).
/// - [production] 없으면 `--account`/`PSTMACRO_LIVE_ACCOUNT_ID` 로 cookies 폴더의
///   `<account_id>.json` 을 (만료 검증 없이) 조회한다.
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

    // 유효성 상태를 정보용으로 출력 (성공 케이스는 valid 여야 함)
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
    eprintln!("  production: PSTMACRO_LIVE_ACCOUNT_ID=id PSTMACRO_LIVE_CAFE_ID=12345 PSTMACRO_LIVE_MENU_ID=1 cargo run --example send_post");
    eprintln!("  로컬 테스트: PSTMACRO_LIVE_COOKIES_PATH=/tmp/<id>.json PSTMACRO_LIVE_CAFE_ID=12345 PSTMACRO_LIVE_MENU_ID=1 cargo run --example send_post");
    eprintln!("  플래그: --account --cookies --cafe --menu --subject --body");
    eprintln!();
    eprintln!("주의: 실제 카페에 글이 작성됩니다. 본인 테스트 카페에만 사용하세요.");
}
