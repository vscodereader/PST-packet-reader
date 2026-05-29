// Chrome
pub const CHROME_PATH_WINDOWS: &str =
    "C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe";
pub const CHROME_PATH_WSL: &str =
    "/mnt/c/Program Files/Google/Chrome/Application/chrome.exe";

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

