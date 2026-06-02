// Chrome
#[cfg(target_os = "windows")]
pub const CHROME_PATH_WINDOWS: &str = "C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe";

// Linux/WSL에서 Playwright가 실행할 시스템 Chrome/Chromium 후보 경로들입니다.
// (Playwright는 Linux에서 Windows용 chrome.exe를 실행할 수 없으므로 Linux 브라우저가 필요합니다.)
#[cfg(not(target_os = "windows"))]
pub const CHROME_PATHS_LINUX: &[&str] = &[
    "/usr/bin/google-chrome",
    "/usr/bin/google-chrome-stable",
    "/opt/google/chrome/chrome",
    "/usr/bin/chromium",
    "/usr/bin/chromium-browser",
];

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

    // Windows 빌드는 Windows Chrome을, Linux/WSL 빌드는 Linux Chrome을 찾습니다.
    #[cfg(target_os = "windows")]
    let candidates: &[&str] = &[CHROME_PATH_WINDOWS];
    #[cfg(not(target_os = "windows"))]
    let candidates: &[&str] = CHROME_PATHS_LINUX;

    candidates
        .iter()
        .find(|path| std::path::Path::new(path).exists())
        .map(|path| path.to_string())
        .ok_or_else(|| {
            format!(
                "Chrome을 찾을 수 없습니다(확인한 경로: {}).\n\
                 다른 경로에 설치된 경우 환경변수 CHROME_PATH에 실행 파일 전체 경로를 지정하세요.",
                candidates.join(", ")
            )
        })
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

        // 미설정이면 플랫폼 기본 후보들을 탐색해 폴백한다. CDP 로그인 전환 이후
        // Linux/WSL 빌드는 시스템 Chrome(`/usr/bin/google-chrome` 등)을 찾으므로,
        // 결과는 호스트에 Chrome이 설치돼 있는지에 따라 달라진다. 따라서 호스트에
        // 의존하지 않는 불변식만 검증한다: 성공하면 그 경로는 실제로 존재하고,
        // 실패하면 탐색한 후보들을 안내하는 오류 메시지를 낸다.
        std::env::remove_var("CHROME_PATH");
        match chrome_path() {
            Ok(path) => assert!(
                std::path::Path::new(&path).exists(),
                "폴백 경로가 존재하지 않습니다: {path}"
            ),
            Err(err) => assert!(
                err.contains("Chrome을 찾을 수 없습니다"),
                "unexpected error: {err}"
            ),
        }
    }
}
