//! Environment diagnostics — reports whether the external tools the automation
//! depends on are healthy, served over Tauri IPC for the 알림 화면 status cards.
//!
//! Unlike the other ipc domains this is *not* JSON-file-backed: it probes the
//! live environment on demand (the UI's 새로고침 button re-invokes it). Both
//! probes are best-effort and never throw — a missing Chrome / unplugged ADB
//! device is reported as a status field so the whole result can render at once.
//! 보안: 쿠키 등 민감정보는 다루지 않으며, 출력은 경로/버전/연결여부뿐이다.

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

/// Chrome 상태를 조사한다. 경로를 찾으면 installed=true 이며, 버전을 못 읽어도
/// installed 는 유지된다("설치됨/버전 미상"). Chrome 을 실행하지 않는다.
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
            error: Some(message),
        },
    }
}

/// ADB 상태를 조사한다. 미연결/스캔 실패는 connected=false 로 정상 변환한다.
async fn probe_adb() -> AdbStatus {
    match crate::auth::probe_adb_connection().await {
        Ok(()) => AdbStatus {
            connected: true,
            error: None,
        },
        Err(err) => AdbStatus {
            connected: false,
            error: Some(err.to_string()),
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
