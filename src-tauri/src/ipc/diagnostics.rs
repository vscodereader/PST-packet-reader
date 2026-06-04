//! Environment diagnostics — reports whether the external tools the automation
//! depends on are healthy, served over Tauri IPC for the 알림 화면 status cards.
//!
//! Unlike the other ipc domains this is *not* JSON-file-backed: it probes the
//! live environment on demand (the UI's 새로고침 button re-invokes it). Both
//! probes are best-effort and never throw — a missing Chrome / unplugged ADB
//! device is reported as a status field so the whole result can render at once.
//! 보안: 쿠키 등 민감정보는 다루지 않으며, 출력은 경로/버전/연결여부뿐이다.

use std::time::Duration;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Chrome 실행 파일 설치/버전 상태.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../src/shared/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct ChromeStatus {
    /// Chrome 경로를 찾았는지 여부.
    pub installed: bool,
    /// 찾은 실행 파일 경로(설치된 경우).
    pub path: Option<String>,
    /// 설치 디렉터리에서 읽은 버전(못 읽으면 None = "버전 미상").
    pub version: Option<String>,
    /// 미설치/경로 오류 사유(설치된 경우 None).
    pub error: Option<String>,
}

/// ADB 디바이스 감지 상태.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../src/shared/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct AdbStatus {
    /// USB 디바이스가 감지되었는지 여부.
    pub connected: bool,
    /// 미연결/스캔 실패 사유(연결된 경우 None). 미연결은 정상 상태로 취급.
    pub error: Option<String>,
}

/// Chrome + ADB 진단을 한 번에 묶은 결과.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../src/shared/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct EnvironmentStatus {
    pub chrome: ChromeStatus,
    pub adb: AdbStatus,
}

/// chrome.exe 가 있는 `Application` 디렉터리에서 버전명 하위 폴더(예: `125.0.6422.142`)를
/// 찾아 버전을 얻는다. 여러 개면 가장 높은 버전을 쓴다(업데이트 직후 등).
///
/// 중요: Chrome 을 **실행하지 않는다.** Windows 의 `chrome.exe --version` 은 버전을
/// 출력하지 않고 브라우저 창을 띄우는 문제가 있어, 실행 대신 설치 디렉터리 구조만
/// 읽는다. 버전 폴더를 못 찾으면 None("버전 미상")이다.
fn chrome_version_from_install_dir(chrome_exe: &str) -> Option<String> {
    let app_dir = std::path::Path::new(chrome_exe).parent()?;
    let mut versions: Vec<String> = std::fs::read_dir(app_dir)
        .ok()?
        .flatten()
        .filter(|entry| entry.path().is_dir())
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter(|name| is_chrome_version_dir(name))
        .collect();
    versions.sort_by_key(|name| version_sort_key(name));
    versions.pop()
}

/// 버전 폴더 이름인지 — 점으로 구분된 2개 이상의 정수(예: "125.0.6422.142").
fn is_chrome_version_dir(name: &str) -> bool {
    let parts: Vec<&str> = name.split('.').collect();
    parts.len() >= 2
        && parts
            .iter()
            .all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()))
}

/// 버전 문자열을 숫자 튜플로 — 문자열 정렬("9" > "125") 대신 수치 비교를 위해.
fn version_sort_key(name: &str) -> Vec<u64> {
    name.split('.')
        .filter_map(|part| part.parse().ok())
        .collect()
}

/// 백엔드 Chrome 탐색 오류 원문을 일반 사용자용 안내 문구로 변환한다.
///
/// `chrome_path()` 의 오류는 확인한 후보 경로 목록·환경변수(`CHROME_PATH`) 지정 안내
/// 같은 개발자/관리자용 정보를 담고 있어, 그대로 노출하면 일반 사용자가 무엇을 해야
/// 할지 알기 어렵다. ADB 안내([`friendly_adb_error`])와 톤을 맞춰 원인별 조치를 짧게
/// 안내한다.
fn friendly_chrome_error(raw: &str) -> String {
    let message = if raw.contains("설정되어 있지만") {
        // 관리자가 지정한 CHROME_PATH 경로에 실제 파일이 없는 경우.
        "지정된 위치에서 Chrome을 찾을 수 없습니다. Chrome이 삭제되었거나 설치 위치가 \
         바뀌었을 수 있어요. 관리자에게 문의해 주세요."
    } else {
        // 표준 설치 위치 어디에도 없음 = 미설치로 안내.
        "Chrome 브라우저가 설치되어 있지 않습니다. Chrome을 설치한 뒤 \
         '환경 상태 새로고침'을 눌러 주세요."
    };

    message.to_string()
}

/// Chrome 상태를 조사한다. 경로를 찾으면 installed=true 이며, 버전을 못 읽어도
/// installed 는 유지된다("설치됨/버전 미상"). Chrome 을 실행하지 않는다.
/// 미설치 사유는 일반 사용자용 안내 문구로 바꿔 담는다(개발자용 원문은 노출하지 않음).
async fn probe_chrome() -> ChromeStatus {
    match crate::auth::config::chrome_path() {
        Ok(path) => {
            let version = chrome_version_from_install_dir(&path);
            ChromeStatus {
                installed: true,
                path: Some(path),
                version,
                error: None,
            }
        }
        Err(message) => ChromeStatus {
            installed: false,
            path: None,
            version: None,
            error: Some(friendly_chrome_error(&message)),
        },
    }
}

/// 백엔드 ADB 오류 원문을 일반 사용자용 안내 문구로 변환한다.
///
/// `probe_adb_connection` 이 돌려주는 원문(예: `adb: USB Error: Access denied
/// (insufficient permissions)`)은 개발자용이라 그대로 노출하면 일반 사용자가
/// 무엇을 해야 할지 알 수 없다. 원인을 오류 Display 문자열의 안정적인 키워드로
/// 분류해, 사용자가 취할 조치를 한국어로 안내한다.
fn friendly_adb_error(raw: &str) -> String {
    let lower = raw.to_lowercase();

    let message = if lower.contains("access denied")
        || lower.contains("insufficient permission")
        || lower.contains("permission")
    {
        // 권한/점유 충돌 — 오늘 겪은 그 케이스.
        "디바이스에 접근할 수 없습니다. 다른 ADB 프로그램(Android Studio·scrcpy 등)을 \
         모두 종료한 뒤, 휴대폰 화면의 'USB 디버깅 허용'을 눌러 주세요."
    } else if lower.contains("busy") {
        "다른 프로그램이 디바이스를 사용 중입니다. 휴대폰 관리 프로그램이나 다른 ADB \
         도구를 종료한 뒤 다시 시도해 주세요."
    } else if lower.contains("not found")
        || lower.contains("no device")
        || lower.contains("no such device")
    {
        "연결된 디바이스가 없습니다. 휴대폰을 USB로 연결하고 USB 디버깅을 켜 주세요."
    } else {
        "디바이스를 확인할 수 없습니다. USB 연결과 USB 디버깅 설정을 확인한 뒤 \
         다시 시도해 주세요."
    };

    message.to_string()
}

/// ADB 상태를 조사한다. 미연결/스캔 실패는 connected=false 로 정상 변환하며,
/// 사유는 일반 사용자용 안내 문구로 바꿔 담는다(개발자용 원문은 노출하지 않음).
///
/// `autodetect` 가 드라이버 문제 등으로 멈추면 UI 스피너가 영영 돌 수 있으므로
/// 상한(`ADB_PROBE_TIMEOUT_SECS`)을 두고, 초과하면 "지연" 안내로 반환한다.
/// (초과 시 백그라운드 블로킹 스레드는 계속 돌 수 있으나, 커맨드는 즉시 반환되어
///  화면이 멈추지 않는다.)
async fn probe_adb() -> AdbStatus {
    let timeout = Duration::from_secs(crate::auth::config::ADB_PROBE_TIMEOUT_SECS);

    match tokio::time::timeout(timeout, crate::auth::probe_adb_connection()).await {
        Ok(Ok(())) => AdbStatus {
            connected: true,
            error: None,
        },
        Ok(Err(err)) => AdbStatus {
            connected: false,
            error: Some(friendly_adb_error(&err.to_string())),
        },
        Err(_elapsed) => AdbStatus {
            connected: false,
            error: Some(
                "디바이스 확인이 지연되고 있습니다. USB 연결과 케이블을 확인한 뒤 \
                 다시 시도해 주세요."
                    .to_string(),
            ),
        },
    }
}

/// 현재 환경의 Chrome/ADB 상태를 반환한다. 두 검사는 독립이라 병렬 실행하며,
/// 부분 실패도 화면에 표시해야 하므로 항상 Ok 상태값을 돌려준다(throw 안 함).
#[tauri::command]
pub async fn get_environment_status() -> EnvironmentStatus {
    let (chrome, adb) = tokio::join!(probe_chrome(), probe_adb());
    EnvironmentStatus { chrome, adb }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_chrome_version_dir_matches_dotted_numbers_only() {
        assert!(is_chrome_version_dir("125.0.6422.142"));
        assert!(is_chrome_version_dir("130.0"));
        assert!(!is_chrome_version_dir("SetupMetrics")); // 버전 폴더 아님
        assert!(!is_chrome_version_dir("12345")); // 점 없음
        assert!(!is_chrome_version_dir("1.")); // 빈 파트
    }

    #[test]
    fn chrome_version_from_install_dir_picks_the_highest_version_folder() {
        let app = tempfile::tempdir().unwrap();
        std::fs::create_dir(app.path().join("125.0.6422.142")).unwrap();
        std::fs::create_dir(app.path().join("130.0.6723.58")).unwrap();
        std::fs::create_dir(app.path().join("SetupMetrics")).unwrap();
        let chrome_exe = app.path().join("chrome.exe");
        std::fs::write(&chrome_exe, b"").unwrap();

        // 문자열 정렬이면 "9..." > "130..." 으로 틀리므로 수치 비교를 검증.
        std::fs::create_dir(app.path().join("9.0.0.1")).unwrap();
        assert_eq!(
            chrome_version_from_install_dir(chrome_exe.to_str().unwrap()),
            Some("130.0.6723.58".to_string())
        );
    }

    #[test]
    fn chrome_version_from_install_dir_returns_none_without_a_version_folder() {
        let app = tempfile::tempdir().unwrap();
        let chrome_exe = app.path().join("chrome.exe");
        std::fs::write(&chrome_exe, b"").unwrap();
        assert_eq!(
            chrome_version_from_install_dir(chrome_exe.to_str().unwrap()),
            None
        );
    }

    #[test]
    fn friendly_adb_error_explains_permission_denied_without_dev_jargon() {
        // 오늘 실제로 뜬 개발자용 원문.
        let msg = friendly_adb_error("adb: USB Error: Access denied (insufficient permissions)");
        assert!(msg.contains("USB 디버깅 허용"));
        // 개발자용 원문이 사용자에게 새어 나가지 않아야 한다.
        assert!(!msg.to_lowercase().contains("access denied"));
        assert!(!msg.contains("USB Error"));
    }

    #[test]
    fn friendly_adb_error_explains_missing_device() {
        // RustADBError 의 여러 "기기 없음" Display 변형을 모두 같은 안내로.
        assert!(friendly_adb_error("adb: USB Device not found: 0 0").contains("USB로 연결"));
        assert!(friendly_adb_error("adb: Device not found: foo").contains("USB로 연결"));
    }

    #[test]
    fn friendly_adb_error_explains_busy_device() {
        let msg = friendly_adb_error("adb: Device is busy. Is ADB server running?");
        assert!(msg.contains("사용 중"));
    }

    #[test]
    fn friendly_adb_error_falls_back_for_unknown_causes() {
        let msg = friendly_adb_error("adb: some unexpected internal failure");
        assert!(msg.contains("확인"));
        assert!(!msg.contains("internal failure"));
    }

    #[test]
    fn friendly_chrome_error_explains_broken_override_path() {
        // CHROME_PATH 가 지정됐는데 그 파일이 없는 경우 (config.rs 의 실제 원문).
        let msg = friendly_chrome_error(
            "CHROME_PATH이 설정되어 있지만 파일이 없습니다: /no/such/chrome.exe",
        );
        assert!(msg.contains("관리자"));
        // 환경변수 이름·경로 같은 개발자용 정보는 노출하지 않는다.
        assert!(!msg.contains("CHROME_PATH"));
        assert!(!msg.contains("/no/such"));
    }

    #[test]
    fn friendly_chrome_error_stays_in_sync_with_config_override_message() {
        // 회귀 방지: friendly_chrome_error 의 분류 키워드("설정되어 있지만")는
        // config.rs 의 실제 오류 문구에 결합돼 있다. config.rs 문구가 바뀌면
        // 분류가 조용히 "미설치"로 빠지므로, 실제 출력을 분류해 검증한다.
        // (CHROME_PATH 등 프로세스 env 를 만지므로 ENV_LOCK 으로 직렬화한다.)
        let _guard = crate::auth::config::ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        std::env::set_var("CHROME_PATH", "/no/such/chrome-xyz.exe");

        let raw = crate::auth::config::chrome_path().unwrap_err();

        std::env::remove_var("CHROME_PATH");

        let msg = friendly_chrome_error(&raw);
        assert!(
            msg.contains("관리자"),
            "config.rs 의 CHROME_PATH 오류 문구가 바뀌어 분류가 어긋났습니다. \
             friendly_chrome_error 의 키워드를 함께 갱신하세요. 원문: {raw}"
        );
    }

    #[test]
    fn friendly_chrome_error_explains_missing_install() {
        // 표준 위치 어디에도 없을 때의 실제 원문(경로 목록 + 환경변수 안내 포함).
        let raw = "Chrome을 찾을 수 없습니다. 다음 경로를 확인했습니다:\n  \
                   - C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe\n\
                   다른 경로에 설치된 경우 환경변수 CHROME_PATH에 chrome.exe 전체 경로를 지정하세요.";
        let msg = friendly_chrome_error(raw);
        assert!(msg.contains("설치"));
        assert!(!msg.contains("CHROME_PATH"));
        assert!(!msg.contains("chrome.exe"));
    }

    #[test]
    fn environment_status_roundtrips_through_json_with_camelcase() {
        let status = EnvironmentStatus {
            chrome: ChromeStatus {
                installed: true,
                path: Some("/x/chrome".to_string()),
                version: Some("125.0.6422.142".to_string()),
                error: None,
            },
            adb: AdbStatus {
                connected: false,
                error: Some("no device".to_string()),
            },
        };
        let json = serde_json::to_string(&status).unwrap();
        assert!(json.contains("\"installed\":true"));
        assert!(json.contains("\"connected\":false"));
        let back: EnvironmentStatus = serde_json::from_str(&json).unwrap();
        assert_eq!(status, back);
    }

    #[tokio::test]
    async fn probe_chrome_keeps_installed_true_when_version_unreadable() {
        // CHROME_PATH 가 존재하면 installed=true 이지만, 옆에 버전 폴더가 없으면
        // version 은 None("버전 미상")이어야 한다. (Chrome 을 실행하지 않으므로
        // 어떤 경우에도 브라우저 창이 뜨지 않는다.)
        let _guard = crate::auth::config::ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let dir = tempfile::tempdir().unwrap();
        let chrome_exe = dir.path().join("chrome.exe");
        std::fs::write(&chrome_exe, b"").unwrap();
        std::env::set_var("CHROME_PATH", &chrome_exe);

        let status = probe_chrome().await;

        std::env::remove_var("CHROME_PATH");

        assert!(status.installed);
        assert!(status.error.is_none());
        assert!(status.version.is_none());
    }
}
