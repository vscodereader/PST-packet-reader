// Chrome
pub const CHROME_PATH_WINDOWS: &str =
    "C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe";
pub const CHROME_PATH_WSL: &str =
    "/mnt/c/Program Files/Google/Chrome/Application/chrome.exe";

/// 실행 환경에 맞는 Chrome 경로를 반환한다.
///
/// 우선순위: `CHROME_PATH` 환경 변수 → 플랫폼 기본값.
/// 경로가 존재하지 않으면 `Err`를 반환해 호출 지점에서 명확한 오류를 낼 수 있다.
pub fn chrome_path() -> Result<String, String> {
    if let Ok(env_path) = std::env::var("CHROME_PATH") {
        if std::path::Path::new(&env_path).exists() {
            return Ok(env_path);
        }
        return Err(format!(
            "CHROME_PATH이 설정되어 있지만 파일이 없습니다: {env_path}"
        ));
    }

    let is_wsl = std::fs::read_to_string("/proc/version")
        .map(|v| v.to_lowercase().contains("microsoft"))
        .unwrap_or(false);
    let default_path = if is_wsl {
        CHROME_PATH_WSL
    } else {
        CHROME_PATH_WINDOWS
    };

    if std::path::Path::new(default_path).exists() {
        Ok(default_path.to_string())
    } else {
        Err(format!(
            "Chrome을 찾을 수 없습니다: {default_path}\n\
             다른 경로에 설치된 경우 환경변수 CHROME_PATH에 chrome.exe 전체 경로를 지정하세요."
        ))
    }
}

// ADB
pub const ADB_AIRPLANE_ENABLE_SECS: u64 = 2;
pub const ADB_INTERNET_POLL_INTERVAL_MS: u64 = 1_000;
pub const ADB_INTERNET_TIMEOUT_SECS: u64 = 30;
pub const ADB_INTERNET_PING_HOST: &str = "8.8.8.8";

// 앱 데이터 경로
pub const APP_DATA_SUBDIR: &str = ".local";
pub const APP_NAME: &str = "pstmacro";
pub const DIR_ACCOUNTS: &str = "accounts";
pub const DIR_COOKIES: &str = "cookies";
pub const DIR_LOGS: &str = "logs";
pub const FILE_ACCOUNTS: &str = "accounts.json";
