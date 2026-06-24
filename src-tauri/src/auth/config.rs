// Chrome — Windows 표준 설치 위치(64비트/32비트). 사용자 단위 설치는 LOCALAPPDATA 로 별도 구성.
#[cfg(target_os = "windows")]
pub const CHROME_PATH_WINDOWS: &str = "C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe";
#[cfg(target_os = "windows")]
pub const CHROME_PATH_WINDOWS_X86: &str =
    "C:\\Program Files (x86)\\Google\\Chrome\\Application\\chrome.exe";

// Linux/WSL에서 CDP 로그인이 띄울 시스템 Chrome/Chromium 후보 경로들입니다.
// (Linux 빌드는 Windows용 chrome.exe를 실행할 수 없으므로 네이티브 Linux 브라우저가 필요합니다.)
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
/// 우선순위:
///   1. `CHROME_PATH` 환경 변수 — 설정됐으면 최우선(파일이 없으면 명확한 오류).
///   2. 플랫폼별 표준 설치 후보들을 순서대로 탐색해 존재하는 첫 경로.
///      - Windows: 64비트 → 32비트 → 사용자 단위 설치(`%LOCALAPPDATA%`)
///      - Linux/WSL: 시스템 Chrome/Chromium(`/usr/bin/google-chrome` 등). CDP 로그인은
///        Linux 프로세스에서 브라우저를 구동하므로 네이티브 Linux 브라우저가 필요하다.
///
/// 어느 것도 못 찾으면, 확인한 후보 목록을 담은 `Err`를 반환한다.
pub fn chrome_path() -> Result<String, String> {
    if let Ok(env_path) = std::env::var("CHROME_PATH") {
        if std::path::Path::new(&env_path).exists() {
            return Ok(env_path);
        }
        return Err(format!(
            "CHROME_PATH이 설정되어 있지만 파일이 없습니다: {env_path}"
        ));
    }

    resolve_first_existing(&chrome_candidates())
}

/// 후보 경로 중 실제로 존재하는 첫 번째를 채택한다. 모두 없으면 확인한 목록을
/// 담은 오류를 돌려준다. (실제 파일시스템만 보므로 순수 로직 — 테스트 용이)
fn resolve_first_existing(candidates: &[String]) -> Result<String, String> {
    if let Some(found) = candidates
        .iter()
        .find(|path| std::path::Path::new(path).exists())
    {
        return Ok(found.clone());
    }

    let checked = candidates
        .iter()
        .map(|path| format!("  - {path}"))
        .collect::<Vec<_>>()
        .join("\n");
    Err(format!(
        "Chrome을 찾을 수 없습니다. 다음 경로를 확인했습니다:\n{checked}\n\
         다른 경로에 설치된 경우 환경변수 CHROME_PATH에 실행 파일 전체 경로를 지정하세요."
    ))
}

/// Windows 표준 Chrome 후보 경로들을 우선순위 순으로 모은다.
/// (64비트 → 32비트 → 사용자 단위 설치 `%LOCALAPPDATA%`)
#[cfg(target_os = "windows")]
fn chrome_candidates() -> Vec<String> {
    let mut candidates = vec![
        CHROME_PATH_WINDOWS.to_string(),
        CHROME_PATH_WINDOWS_X86.to_string(),
    ];

    // 사용자 단위 설치(관리자 권한 없이 설치하면 여기로 감):
    //   %LOCALAPPDATA%\Google\Chrome\Application\chrome.exe
    if let Some(local) = std::env::var_os("LOCALAPPDATA") {
        let user_chrome = std::path::Path::new(&local)
            .join("Google")
            .join("Chrome")
            .join("Application")
            .join("chrome.exe");
        candidates.push(user_chrome.to_string_lossy().into_owned());
    }

    candidates
}

/// Linux/WSL의 시스템 Chrome/Chromium 후보 경로들(CDP 로그인이 구동할 네이티브 브라우저).
#[cfg(not(target_os = "windows"))]
fn chrome_candidates() -> Vec<String> {
    CHROME_PATHS_LINUX.iter().map(|p| p.to_string()).collect()
}

// ADB
/// 비행기모드를 켠 뒤 끄기 전까지 대기하는 시간(초) — **IP 변경 버튼(EnsureIpChange)** 전용.
/// 너무 짧으면 단말 라디오가 실제로 끊겼다 붙을 새가 없어 IP가 그대로 유지될 수 있어, 넉넉히
/// 3초를 둔다(사수 피드백). 로그인(FastConfirm)은 이 대신 ADB 상태 확정 폴링을 쓴다(아래).
pub const ADB_AIRPLANE_ENABLE_SECS: u64 = 3;
/// 로그인(FastConfirm)에서 비행기모드 ON/OFF가 ADB로 확정될 때까지 폴링하는 간격(ms).
/// 고정 대기 대신 실제 상태를 확인해 확정 즉시 다음으로 넘어간다(사수 지시).
pub const ADB_AIRPLANE_CONFIRM_POLL_MS: u64 = 100;
/// 비행기모드 상태 확정 폴링의 상한(초). 못 확인해도 무한 대기하지 않도록 두고, 넘으면
/// 토글 명령은 이미 실행됐으므로 그대로 진행한다.
pub const ADB_AIRPLANE_CONFIRM_TIMEOUT_SECS: u64 = 10;
/// IP 회전(비행기모드 토글) 후 네트워크가 안정될 때까지 기다렸다가 Chrome을 띄운다(초).
/// ping으로 연결이 붙은 직후엔 DNS/라우팅·TLS 세션이 아직 자리잡지 않아, 너무 짧으면
/// "연결이 제대로 되지 않은 상태"로 로그인을 시도해 캡차가 100% 뜬다(사수 진단). 넉넉히
/// 3초를 둔다 — IP '폴링' 간격(아래 100ms)과는 별개의 안정화 대기다.
pub const ADB_SETTLE_AFTER_ROTATE_SECS: u64 = 3;
/// IP(외부망) 변경/복구를 확인하는 리트라이 간격(ms). 사수 지침(#267-4): 비행기모드 토글 뒤
/// IP가 바뀌었는지 빠르게 폴링하도록 100ms로 둔다(기존 1000ms → 로그인 대기 단축).
pub const ADB_INTERNET_POLL_INTERVAL_MS: u64 = 100;
pub const ADB_INTERNET_TIMEOUT_SECS: u64 = 30;
pub const ADB_INTERNET_PING_HOST: &str = "8.8.8.8";
/// 진단용 USB 디바이스 스캔(autodetect)이 드라이버 문제 등으로 멈추는 것을 막는
/// 상한. 정상이면 거의 즉시 끝나므로 넉넉히 잡는다(초과 시 "지연" 안내로 반환).
pub const ADB_PROBE_TIMEOUT_SECS: u64 = 10;
/// 로그인 시 개별 adb CLI 명령(devices/airplane/ping)의 응답 상한(초, #210). adb 서버가
/// 행이면 `cmd.output()`이 무한 블록돼 로그인 큐가 멈추므로, 명령 1건마다 이 시간으로
/// 끊어 실패로 처리하고 다음 계정으로 진행한다. 정상 명령은 거의 즉시 끝난다.
pub const ADB_STEP_TIMEOUT_SECS: u64 = 15;

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

        // 미설정이면 플랫폼 기본 후보들을 탐색해 폴백한다. 결과는 호스트에 Chrome이
        // 설치돼 있는지에 따라 달라지므로, 호스트에 의존하지 않는 불변식만 검증한다:
        // 성공하면 그 경로는 실제로 존재하고, 실패하면 탐색한 후보를 안내하는 오류를 낸다.
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

    #[test]
    fn resolve_first_existing_picks_the_first_present_candidate() {
        let file = tempfile::NamedTempFile::new().unwrap();
        let present = file.path().to_string_lossy().into_owned();

        let got = resolve_first_existing(&[
            "/no/such/a.exe".to_string(),
            present.clone(),
            "/no/such/b.exe".to_string(),
        ])
        .unwrap();
        assert_eq!(got, present);
    }

    #[test]
    fn resolve_first_existing_errors_with_the_checked_list() {
        let err =
            resolve_first_existing(&["/no/such/a.exe".to_string(), "/no/such/b.exe".to_string()])
                .unwrap_err();
        assert!(
            err.contains("Chrome을 찾을 수 없습니다"),
            "unexpected: {err}"
        );
        assert!(
            err.contains("/no/such/a.exe"),
            "missing checked list: {err}"
        );
    }

    // Windows 후보 구성은 cfg(windows)에서만 컴파일되므로, 해당 타깃 빌드에서만 검증한다.
    // (실 배포 타깃이 Windows 인스톨러라, 사용자 단위 설치 폴백이 후보에 붙는지 보장.)
    #[cfg(target_os = "windows")]
    #[test]
    fn chrome_candidates_append_user_install_when_localappdata_set() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        std::env::set_var("LOCALAPPDATA", "C:\\Users\\test\\AppData\\Local");

        let candidates = chrome_candidates();

        std::env::remove_var("LOCALAPPDATA");

        // 표준 2개(64/32비트) 뒤에 사용자 설치 후보가 마지막으로 붙는다.
        assert_eq!(candidates.len(), 3);
        let user = candidates.last().unwrap();
        assert!(user.contains("Google"));
        assert!(user.ends_with("chrome.exe"));
    }
}
