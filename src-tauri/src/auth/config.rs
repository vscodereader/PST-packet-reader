// Chrome
pub const CHROME_PATH_WINDOWS: &str = "C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe";
pub const CHROME_PATH_WSL: &str = "/mnt/c/Program Files/Google/Chrome/Application/chrome.exe";

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

// 쿠키 저장 확인
pub const COOKIE_WRITE_POLL_INTERVAL_MS: u64 = 200;
pub const COOKIE_WRITE_TIMEOUT_SECS: u64 = 5;

// 앱 데이터 경로
pub const APP_NAME: &str = "pstmacro";
pub const DIR_ACCOUNTS: &str = "accounts";
pub const DIR_COOKIES: &str = "cookies";
pub const DIR_LOGS: &str = "logs";
pub const FILE_ACCOUNTS: &str = "accounts.json";

/// 프로세스 환경 변수(`CHROME_PATH`, `LOCALAPPDATA`)를 만지는 테스트들을
/// 직렬화하기 위한 공용 락. env는 프로세스 전역이라 병렬 테스트가 동시에
/// 쓰면 경합하므로, 해당 테스트들은 이 락을 잡고 실행한다.
#[cfg(test)]
pub(crate) static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chrome_path_prefers_existing_env_override() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let file = tempfile::NamedTempFile::new().unwrap();
        let existing = file.path().to_string_lossy().into_owned();

        // 존재하는 CHROME_PATH는 그대로 채택된다.
        std::env::set_var("CHROME_PATH", &existing);
        assert_eq!(chrome_path().unwrap(), existing);

        // 설정됐지만 파일이 없으면 명확한 오류.
        std::env::set_var("CHROME_PATH", "/no/such/chrome.exe");
        let err = chrome_path().unwrap_err();
        assert!(err.contains("파일이 없습니다"), "unexpected error: {err}");

        // 미설정이면 플랫폼 기본값으로 폴백 — 테스트 호스트(Linux)엔 없으므로 오류.
        std::env::remove_var("CHROME_PATH");
        assert!(chrome_path().is_err());
    }
}
