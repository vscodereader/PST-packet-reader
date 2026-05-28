use std::{env, fs, path::PathBuf, process::Command};

use pstmacro_lib::auth::config;
use serde::Deserialize;

// 실행방법
// 단일 계정: NAVER_ID='id' NAVER_PWD='pw' cargo run --example auto_login
// 다중 계정: cargo run --example auto_login -- --accounts accounts.json
// Optional: --headless 플래그 또는 HEADLESS=1 환경변수 설정 시 브라우저 숨김 모드

#[derive(Deserialize)]
struct AccountEntry {
    id: String,
    password: String,
}

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        print_usage();
        return;
    }

    let headless =
        args.iter().any(|arg| arg == "--headless") || env::var("HEADLESS").as_deref() == Ok("1");

    if let Some(accounts_path) = value_after(&args, "--accounts") {
        run_multi(&accounts_path, headless);
    } else {
        run_single(&args, headless);
    }
}

fn run_single(args: &[String], headless: bool) {
    let id = value_after(args, "--id").or_else(|| env::var("NAVER_ID").ok());
    let password = value_after(args, "--password").or_else(|| env::var("NAVER_PWD").ok());

    let Some(id) = id else { print_usage_and_exit() };
    let Some(password) = password else { print_usage_and_exit() };

    match run_login(&id, &password, headless) {
        Ok(cookies_json) => println!("{cookies_json}"),
        Err(e) => {
            eprintln!("[{id}] login failed: {e}");
            std::process::exit(1);
        }
    }
}

fn run_multi(accounts_path: &str, headless: bool) {
    let text = match fs::read_to_string(accounts_path) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("failed to read accounts file: {e}");
            std::process::exit(1);
        }
    };
    let accounts: Vec<AccountEntry> = match serde_json::from_str(&text) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("invalid accounts JSON: {e}");
            std::process::exit(1);
        }
    };

    let total = accounts.len();
    let mut failed = 0usize;

    for (i, account) in accounts.iter().enumerate() {
        eprintln!("[{}/{}] logging in as {}...", i + 1, total, account.id);
        match run_login(&account.id, &account.password, headless) {
            Ok(cookies_json) => {
                eprintln!("[{}/{}] {} ok", i + 1, total, account.id);
                println!("=== {} ===", account.id);
                println!("{cookies_json}");
            }
            Err(e) => {
                eprintln!("[{}/{}] {} failed: {e}", i + 1, total, account.id);
                failed += 1;
            }
        }
    }

    if failed > 0 {
        eprintln!("{failed}/{total} accounts failed");
        std::process::exit(1);
    }
}

fn run_login(id: &str, password: &str, headless: bool) -> Result<String, String> {
    let tmp = tempfile::tempdir().map_err(|e| e.to_string())?;
    let input_path = tmp.path().join("input.json");
    let cookies_path = tmp.path().join("cookies.json");

    let input = serde_json::json!({
        "accountId": id,
        "id": id,
        "password": password,
        "cookiesPath": cookies_path,
        "headless": headless,
        "chromePath": config::chrome_path(),
        "cdpPort": config::find_free_port(),
    });
    fs::write(&input_path, serde_json::to_string_pretty(&input).unwrap())
        .map_err(|e| e.to_string())?;

    let script = locate_login_script();
    let status = Command::new("node")
        .arg("--experimental-strip-types")
        .arg(script)
        .arg(&input_path)
        .status()
        .map_err(|e| e.to_string())?;

    if !status.success() {
        return Err(format!("playwright exited with status {status}"));
    }

    let json =
        fs::read_to_string(&cookies_path).map_err(|_| "cookie file not written".to_string())?;

    // tmp 디렉토리가 drop되면서 input.json, cookies.json 자동 삭제
    Ok(json)
}

fn locate_login_script() -> PathBuf {
    if let Ok(path) = env::var("PSTMACRO_LOGIN_SCRIPT") {
        return PathBuf::from(path);
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."))
        .join("src")
        .join("features")
        .join("playwright")
        .join("naver-login.ts")
}

fn value_after(args: &[String], flag: &str) -> Option<String> {
    args.windows(2)
        .find(|pair| pair[0] == flag)
        .map(|pair| pair[1].clone())
}

fn print_usage_and_exit() -> ! {
    print_usage();
    std::process::exit(2);
}

fn print_usage() {
    eprintln!("usage:");
    eprintln!("  단일 계정: NAVER_ID='id' NAVER_PWD='pw' cargo run --example auto_login [-- --headless]");
    eprintln!("  다중 계정: cargo run --example auto_login -- --accounts accounts.json [--headless]");
    eprintln!();
    eprintln!("accounts.json 형식:");
    eprintln!(r#"  [{{ "id": "id1", "password": "pw1" }}, {{ "id": "id2", "password": "pw2" }}]"#);
}
