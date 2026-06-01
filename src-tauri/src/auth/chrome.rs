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
const PORT_POLL_INTERVAL: Duration = Duration::from_millis(100);

/// 실행 중인 Chrome 핸들. Drop 시 프로세스 종료 + 임시 프로필 삭제.
pub(crate) struct ChromeHandle {
    child: Child,
    pub(crate) port: u16,
    user_data_dir: PathBuf,
}

impl Drop for ChromeHandle {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.user_data_dir);
    }
}

/// 시스템 Chrome을 디버그 포트로 띄우고 포트가 확정될 때까지 기다린다.
pub(crate) fn launch(headless: bool) -> Result<ChromeHandle, OrchestratorError> {
    let chrome = config::chrome_path().map_err(OrchestratorError::CommandFailed)?;
    let user_data_dir = std::env::temp_dir().join(format!("pstmacro-login-{}", unique_suffix()));
    std::fs::create_dir_all(&user_data_dir)?;

    let profile_arg = format!("--user-data-dir={}", user_data_dir.display());
    let mut args = vec![
        "--remote-debugging-port=0",
        profile_arg.as_str(),
        "--no-first-run",
        "--no-default-browser-check",
        "--disable-quic",
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
