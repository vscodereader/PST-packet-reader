// Chrome / CDP
pub const CHROME_PATH_WINDOWS: &str =
    "C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe";
pub const CHROME_PATH_WSL: &str =
    "/mnt/c/Program Files/Google/Chrome/Application/chrome.exe";
pub const CDP_PORT: u16 = 9222;

/// OS가 비어있는 포트를 자동으로 할당해 반환한다.
pub fn find_free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .expect("failed to bind to a free port")
        .local_addr()
        .expect("failed to get local address")
        .port()
}

/// 실행 환경에 맞는 Chrome 경로를 반환한다.
pub fn chrome_path() -> &'static str {
    let is_wsl = std::fs::read_to_string("/proc/version")
        .map(|v| v.to_lowercase().contains("microsoft"))
        .unwrap_or(false);
    if is_wsl {
        CHROME_PATH_WSL
    } else {
        CHROME_PATH_WINDOWS
    }
}

// scrcpy / ADB
pub const SCRCPY_URL: &str =
    "https://github.com/Genymobile/scrcpy/releases/download/v4.0/scrcpy-win64-v4.0.zip";
pub const ADB_AIRPLANE_ENABLE_SECS: u64 = 2;
pub const ADB_AIRPLANE_DISABLE_SECS: u64 = 8;

// 앱 데이터 경로
pub const APP_DATA_SUBDIR: &str = ".local";
pub const APP_NAME: &str = "pstmacro";
pub const DIR_ACCOUNTS: &str = "accounts";
pub const DIR_COOKIES: &str = "cookies";
pub const DIR_SCRCPY: &str = "scrcpy";
pub const DIR_LOGS: &str = "logs";
pub const FILE_ACCOUNTS: &str = "accounts.json";
pub const FILE_ADB: &str = "adb.exe";

// Playwright 로그인 스크립트
pub const LOGIN_SCRIPT_ENV: &str = "PSTMACRO_LOGIN_SCRIPT";
pub const LOGIN_SCRIPT_PATH: &str = "src/features/playwright/naver-login.ts";
pub const LOGIN_SCRIPT_FILENAME: &str = "naver-login.ts";
