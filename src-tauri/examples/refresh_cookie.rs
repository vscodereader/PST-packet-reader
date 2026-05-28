use pstmacro_lib::orchestrator::{self, Account};

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        print_usage();
        return;
    }
    let id = value_after(&args, "--id").or_else(|| std::env::var("PSTMACRO_NAVER_ID").ok());
    let password =
        value_after(&args, "--password").or_else(|| std::env::var("PSTMACRO_NAVER_PASSWORD").ok());
    let headless = args.iter().any(|arg| arg == "--headless")
        || std::env::var("PSTMACRO_HEADLESS").as_deref() == Ok("1");

    let Some(id) = id else {
        print_usage_and_exit();
    };
    let Some(password) = password else {
        print_usage_and_exit();
    };

    let account = Account {
        label: "cargo-example".to_string(),
        id,
        password,
    };

    match orchestrator::refresh_account_cookie(account, headless).await {
        Ok(paths) => {
            println!("cookie refresh finished");
            println!("cookies dir: {}", paths.cookies_dir.display());
        }
        Err(error) => {
            eprintln!("cookie refresh failed: {error}");
            std::process::exit(1);
        }
    }
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
    eprintln!(
        "usage: cargo run --example refresh_cookie -- --id <naver-id> --password <naver-password> [--headless]"
    );
    eprintln!("env: PSTMACRO_NAVER_ID, PSTMACRO_NAVER_PASSWORD, PSTMACRO_HEADLESS=1");
}
