//! 시스템 Chrome을 CDP 디버그 포트로 띄우는 런처.
//!
//! `--remote-debugging-port=0`으로 띄운 뒤 user-data-dir의 `DevToolsActivePort`
//! 파일에서 실제 포트를 읽어 확정한다(이식성 높은 표준 방법). `ChromeHandle`은 `Drop`에서
//! 자식 프로세스를 종료하고 임시 디렉토리를 정리한다(매번 fresh = 시크릿 등가).

use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::thread::sleep;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use super::{config, error::OrchestratorError};

const PORT_FILE: &str = "DevToolsActivePort";
const PORT_WAIT: Duration = Duration::from_secs(20);
// 디버그 포트(DevToolsActivePort) 파일을 더 촘촘히 폴링해 Chrome 기동 인지 지연을 줄인다(#14).
const PORT_POLL_INTERVAL: Duration = Duration::from_millis(50);

/// 실행 중인 Chrome 핸들. Drop 시 프로세스 종료 + 임시 프로필 삭제.
pub(crate) struct ChromeHandle {
    child: Child,
    pub(crate) port: u16,
    user_data_dir: PathBuf,
}

impl Drop for ChromeHandle {
    fn drop(&mut self) {
        tracing::info!("[CHROME] 창 닫힘 — Chrome 종료 시작...");
        let _ = self.child.kill();
        let _ = self.child.wait(); // 프로세스가 완전히 종료될 때까지 블로킹한다.
        tracing::info!("[CHROME] ✓ Chrome 프로세스 완전 종료 확인");
        let _ = std::fs::remove_dir_all(&self.user_data_dir);
    }
}

/// 시스템 Chrome을 디버그 포트로 띄우고 포트가 확정될 때까지 기다린다.
pub(crate) fn launch(headless: bool) -> Result<ChromeHandle, OrchestratorError> {
    let chrome = config::chrome_path().map_err(OrchestratorError::CommandFailed)?;
    let user_data_dir = std::env::temp_dir().join(format!("pstmacro-login-{}", unique_suffix()));
    std::fs::create_dir_all(&user_data_dir)?;
    // 로그인 성공 시 인증된 네이버 세션(NID_AUT/NID_SES)이 이 프로필에 남으므로,
    // 멀티유저 호스트에서 타 사용자가 읽지 못하도록 소유자 전용(0700)으로 제한한다.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&user_data_dir, std::fs::Permissions::from_mode(0o700))?;
    }

    let profile_arg = format!("--user-data-dir={}", user_data_dir.display());
    let mut args = vec![
        "--remote-debugging-port=0",
        profile_arg.as_str(),
        // 시크릿(incognito) 창으로 띄운다 — 계정마다 깨끗한 세션으로 로그인.
        "--incognito",
        "--no-first-run",
        "--no-default-browser-check",
        "--disable-quic",
        // 봇탐지(ncaptcha) 완화: CDP 제어 시 Chrome이 navigator.webdriver=true 와
        // "Chrome이 자동화 소프트웨어의 제어를 받고 있습니다" 신호를 노출하는 것을 끈다.
        // 실제 키 이벤트(login_flow)만으로는 점수형 캡차를 못 피하므로 자동화 지문도 함께 낮춘다.
        "--disable-blink-features=AutomationControlled",
        "about:blank",
    ];
    if headless {
        args.insert(0, "--headless=new");
    }

    let child = match Command::new(&chrome).args(&args).spawn() {
        Ok(child) => child,
        Err(error) => {
            let _ = std::fs::remove_dir_all(&user_data_dir);
            return Err(OrchestratorError::CommandFailed(format!(
                "Chrome 실행 실패({chrome}): {error}"
            )));
        }
    };

    let mut handle = ChromeHandle {
        child,
        port: 0,
        user_data_dir: user_data_dir.clone(),
    };

    match wait_for_port(&user_data_dir) {
        Ok(port) => {
            handle.port = port;
            tracing::info!("[CHROME] ✓ Chrome 실행 완료 — 디버그 포트 {port} (완전 로딩됨)");
            Ok(handle)
        }
        // handle이 Drop되며 프로세스/임시 디렉토리를 정리한다.
        Err(error) => Err(error),
    }
}

// DevToolsActivePort 파일이 생길 때까지 폴링해서 실제 포트를 읽는다.
fn wait_for_port(user_data_dir: &Path) -> Result<u16, OrchestratorError> {
    let path = user_data_dir.join(PORT_FILE);
    let deadline = Instant::now() + PORT_WAIT;

    loop {
        if let Ok(content) = std::fs::read_to_string(&path) {
            if let Some(port) = parse_devtools_active_port(&content) {
                return Ok(port);
            }
        }

        if Instant::now() >= deadline {
            return Err(OrchestratorError::CommandFailed(
                "Chrome 디버그 포트(DevToolsActivePort)를 확인하지 못했습니다. \
                 headed 모드는 디스플레이(WSLg)가 필요할 수 있습니다."
                    .to_owned(),
            ));
        }

        sleep(PORT_POLL_INTERVAL);
    }
}

/// `DevToolsActivePort` 파일 내용의 첫 줄에서 포트 번호를 읽는다(순수 함수).
pub(crate) fn parse_devtools_active_port(content: &str) -> Option<u16> {
    content.lines().next()?.trim().parse::<u16>().ok()
}

// 임시 프로필 디렉토리 이름에 쓸 충돌 적은 접미사(PID + 나노초).
fn unique_suffix() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or_default();
    format!("{}-{}", std::process::id(), nanos)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_port_from_first_line() {
        assert_eq!(
            parse_devtools_active_port("54321\n/devtools/browser/abc"),
            Some(54321)
        );
        assert_eq!(parse_devtools_active_port("9222"), Some(9222));
    }

    #[test]
    fn rejects_non_numeric_or_empty() {
        assert_eq!(parse_devtools_active_port(""), None);
        assert_eq!(parse_devtools_active_port("\n9222"), None);
        assert_eq!(parse_devtools_active_port("not-a-port"), None);
    }
}
