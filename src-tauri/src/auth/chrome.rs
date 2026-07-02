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
        // "완전 종료"의 판단 근거를 로그로 드러낸다(사수 질문): kill 신호 전송 결과 →
        // wait()가 돌려주는 ExitStatus(= OS가 우리가 spawn한 Chrome 프로세스를 회수했다는
        // 확정 신호) → 임시 프로필 삭제 결과.
        //
        // 핵심(사수 지적): Chrome은 메인 chrome.exe 하나만이 아니라 렌더러/GPU/유틸리티/
        // crashpad 등 여러 자식 프로세스를 별도 PID로 띄운다. `self.child.kill()`은 우리가
        // spawn한 **메인 프로세스만** 종료하므로 자식 헬퍼가 고아로 남아 작업관리자에 계속
        // 쌓이고(15계정 × 병렬 종토 게시에서 누적), 임시 프로필이 잠겨 삭제도 실패했다.
        // 이 고아 누적이 캡차(봇탐지 점수 상승)의 유력 원인으로 지목됐다.
        // → Windows에서는 `taskkill /PID <pid> /T /F`로 프로세스 트리(자식 헬퍼 포함)를
        //   통째로 강제 종료한다. 그 뒤 wait()로 메인 프로세스 핸들을 회수한다.
        let pid = self.child.id();
        tracing::info!(
            pid,
            "[CHROME] 창 닫힘 — Chrome 종료 시작..."
        );

        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            let mut cmd = Command::new("taskkill");
            cmd.args(["/PID", &pid.to_string(), "/T", "/F"]);
            // 콘솔 창이 깜빡이지 않도록 창 없이 실행한다(adb.rs와 동일한 플래그).
            cmd.creation_flags(CREATE_NO_WINDOW);
            match cmd.output() {
                Ok(_) => tracing::info!(
                    pid,
                    "[CHROME] ✓ Chrome 프로세스 트리 종료(자식 헬퍼 포함) — taskkill /T /F"
                ),
                Err(error) => tracing::warn!(
                    pid,
                    %error,
                    "[CHROME] ⚠ taskkill 실행 실패 — child.kill()로 메인만 종료 시도"
                ),
            }
            // taskkill이 이미 프로세스를 죽였어도, spawn 핸들을 회수(reap)하려면 kill/wait를
            // 호출해야 한다. kill은 이미 죽었으면 에러여도 무해하다.
            let _ = self.child.kill();
        }

        #[cfg(not(windows))]
        {
            match self.child.kill() {
                Ok(()) => tracing::info!(pid, "[CHROME]   └ kill 신호 전송 성공 — 종료 대기(wait)"),
                Err(error) => tracing::info!(
                    pid,
                    %error,
                    "[CHROME]   └ kill 불필요/실패(이미 종료됐을 수 있음) — wait로 확정"
                ),
            }
        }

        // wait()는 프로세스가 종료될 때까지 블로킹하고, 회수 성공 시 ExitStatus를 돌려준다.
        match self.child.wait() {
            Ok(status) => tracing::info!(
                pid,
                exit = %status,
                "[CHROME] ✓ Chrome 메인 프로세스 회수 확인(wait 반환, 종료상태 위 표시)"
            ),
            Err(error) => tracing::warn!(
                pid,
                %error,
                "[CHROME] ⚠ Chrome 종료 확인 실패 — wait 오류(프로세스 상태 불명)"
            ),
        }
        match std::fs::remove_dir_all(&self.user_data_dir) {
            Ok(()) => tracing::info!(
                "[CHROME]   └ 임시 프로필 삭제 완료 — 세션 잔여 없음(다음 로그인은 fresh)"
            ),
            Err(error) => tracing::info!(
                %error,
                "[CHROME]   └ 임시 프로필 삭제 실패(고아 헬퍼가 파일을 잠갔을 수 있음) — 다음 로그인은 새 프로필이라 무해"
            ),
        }

        // 사용자가 작업관리자를 열지 않아도 되게, 우리 임시 프로필로 도는 잔존 Chrome 개수를
        // 최선노력(best-effort)으로 세어 로그에 남긴다. 0이 아니면 warn으로 눈에 띄게 한다.
        let leftover = running_chrome_count();
        if leftover > 0 {
            tracing::warn!("[CHROME] 잔존 pstmacro 크롬 프로세스: {leftover}개");
        } else {
            tracing::info!("[CHROME] 잔존 pstmacro 크롬 프로세스: 0개");
        }
    }
}

/// 우리가 띄운 임시 프로필(`pstmacro-login-*`)로 아직 실행 중인 Chrome 프로세스 개수를
/// 최선노력으로 센다. UI가 "실행 중 크롬 N개"를 작업관리자 없이 보여주는 데 쓴다(사수 요청).
/// 프로세스 조회가 실패하면(권한/도구 부재) 0을 돌려준다 — 진단용이라 실패해도 무해하다.
pub(crate) fn running_chrome_count() -> usize {
    // Windows: 각 프로세스의 명령줄을 뽑아, 우리 프로필 마커가 들어간 줄만 센다. Chrome의
    // 자식 헬퍼들도 같은 `--user-data-dir=...pstmacro-login-X`를 물고 있어 함께 잡힌다.
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let mut cmd = Command::new("wmic");
        cmd.args(["process", "get", "commandline"]);
        cmd.creation_flags(CREATE_NO_WINDOW);
        match cmd.output() {
            Ok(out) => count_profile_lines(&String::from_utf8_lossy(&out.stdout)),
            Err(_) => 0,
        }
    }
    // 비-Windows(개발/테스트): ps로 전체 프로세스 명령줄을 훑어 같은 마커를 센다.
    #[cfg(not(windows))]
    {
        match Command::new("ps").args(["-eo", "args="]).output() {
            Ok(out) => count_profile_lines(&String::from_utf8_lossy(&out.stdout)),
            Err(_) => 0,
        }
    }
}

/// 프로세스 목록(명령줄) 출력에서 우리 임시 프로필 마커가 포함된 줄 수를 센다(순수 함수).
fn count_profile_lines(process_listing: &str) -> usize {
    process_listing
        .lines()
        .filter(|line| line.contains(PROFILE_MARKER))
        .count()
}

/// 임시 프로필 디렉토리 이름의 공통 접두사. 프로세스 명령줄에서 우리 Chrome을 식별하는 마커.
const PROFILE_MARKER: &str = "pstmacro-login-";

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
        // 창이 가려지거나(원격 데스크톱·다른 창에 가림) 백그라운드가 되면 Chrome이 렌더러를
        // occluded/backgrounded 로 표시해 `document.visibilityState=hidden` 이 되고, 그러면 합성 키
        // 이벤트(`Input.dispatchKeyEvent`)를 렌더러로 전달하지 않고 버린다(마우스만 먹혀 포커스는
        // 잡히나 타이핑 0자). 실측 로그(2026-07-02, 사수 PC/Chrome 원격 데스크톱)에서 vis=hidden·
        // keydown=0 으로 확정됨. 아래 세 플래그로 가려진 창의 렌더러 백그라운딩/타이머 스로틀을 꺼,
        // 창이 전경이 아니어도 키 입력이 정상 전달되게 한다.
        "--disable-backgrounding-occluded-windows",
        "--disable-renderer-backgrounding",
        "--disable-background-timer-throttling",
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

    #[test]
    fn counts_only_our_profile_processes() {
        // 메인 + 헬퍼(렌더러/GPU) 3줄이 우리 프로필 마커를 물고 있고, 무관한 프로세스는 제외.
        let listing = "\
chrome.exe --user-data-dir=C:\\Temp\\pstmacro-login-123-1 --incognito
chrome.exe --type=renderer --user-data-dir=C:\\Temp\\pstmacro-login-123-1
chrome.exe --type=gpu-process --user-data-dir=C:\\Temp\\pstmacro-login-123-1
notepad.exe
chrome.exe --user-data-dir=C:\\Users\\me\\AppData\\Chrome\\Default";
        assert_eq!(count_profile_lines(listing), 3);
    }

    #[test]
    fn counts_zero_when_no_marker() {
        assert_eq!(count_profile_lines(""), 0);
        assert_eq!(count_profile_lines("chrome.exe\nnotepad.exe\n"), 0);
    }
}
