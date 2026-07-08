//! 시스템 Chrome을 CDP 디버그 포트로 띄우는 런처.
//!
//! `--remote-debugging-port=0`으로 띄운 뒤 user-data-dir의 `DevToolsActivePort`
//! 파일에서 실제 포트를 읽어 확정한다(이식성 높은 표준 방법). `ChromeHandle`은 `Drop`에서
//! 자식 프로세스를 종료하고 임시 디렉토리를 정리한다(매번 fresh = 시크릿 등가).

use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::thread::sleep;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use super::{
    config,
    error::OrchestratorError,
    ua::{self, UaProfile},
};

const PORT_FILE: &str = "DevToolsActivePort";
const PORT_WAIT: Duration = Duration::from_secs(20);
// 디버그 포트(DevToolsActivePort) 파일을 더 촘촘히 폴링해 Chrome 기동 인지 지연을 줄인다(#14).
const PORT_POLL_INTERVAL: Duration = Duration::from_millis(50);

/// 실행 중인 Chrome 핸들. Drop 시 프로세스 종료 + 임시 프로필 삭제.
pub(crate) struct ChromeHandle {
    child: Child,
    pub(crate) port: u16,
    user_data_dir: PathBuf,
    /// 로그인 경로면 이 실행에 쓴 UA 한 벌(Some). 게시/밴드 등 네이티브 UA 실행이면 None.
    /// login.rs 가 페이지 Client Hints 를 이 버전과 같게 맞출 때 쓴다.
    pub(crate) ua: Option<UaProfile>,
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

/// 설치된 크롬의 **실제 풀버전**("149.0.7827.201")을 최선노력으로 읽는다. UA 로테이션이
/// 설치 버전보다 높은 버전을 주장하지 않도록(기능탐지 회피) 상한으로 쓴다. 못 읽으면 None.
/// Windows: `chrome.exe --version`은 GUI 앱이라 콘솔 출력이 없을 수 있어 레지스트리 BLBeacon
/// (자동업데이트가 기록하는 현재 설치 버전)을 읽는다. 비-Windows(개발): `--version` 출력을 파싱.
fn installed_chrome_full_version(chrome_path: &str) -> Option<String> {
    fn first_version_token(text: &str) -> Option<String> {
        text.split_whitespace()
            .find(|t| {
                t.split('.').count() >= 3 && t.chars().next().is_some_and(|c| c.is_ascii_digit())
            })
            .map(ToOwned::to_owned)
    }
    #[cfg(windows)]
    {
        let _ = chrome_path;
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let mut cmd = Command::new("reg");
        cmd.args([
            "query",
            r"HKEY_CURRENT_USER\Software\Google\Chrome\BLBeacon",
            "/v",
            "version",
        ]);
        cmd.creation_flags(CREATE_NO_WINDOW);
        let out = cmd.output().ok()?;
        first_version_token(&String::from_utf8_lossy(&out.stdout))
    }
    #[cfg(not(windows))]
    {
        let out = Command::new(chrome_path).arg("--version").output().ok()?;
        first_version_token(&String::from_utf8_lossy(&out.stdout))
    }
}

/// 실험용(옵트인, 기본 OFF): `PSTMACRO_BLOCK_WASM` 이 설정되면 로그인 Chrome 이 네이버 봇탐지
/// WASM 엔진 호스트(`wtm.pstatic.net`)를 **DNS 단계에서 못 찾게** 막는다. 그러면 wtm 의
/// 로더(`3e66f2f…js`)와 엔진(`353dfc…wasm`)이 아예 로드되지 않아, ncaptcha SDK(`ncpt.naver.com`)
/// **단독 경로**로 강제된다(실측 2026-07-08: wtm SNI/DNS 0 이어도 SDK 혼자 cipherText·wtoken 을
/// 만들어 로그인 통과). "기기지문(fpHash)이 캡차 누적의 앵커인지"를 20판 버스트로 규명하기 위한
/// 실험 스위치다.
///
/// ⚠️ CDP 표면을 늘리지 않으려고 `Network.setBlockedURLs`(→ `Network.enable` 필요) 대신 Chrome
/// `--host-resolver-rules` 를 쓴다 — `Network.enable` 은 봇탐지 표면을 늘려 캡차율을 올릴 수 있어
/// (「enable_page_only」주석) 실험 결과를 오염시킨다. `~NOTFOUND` 는 해당 호스트 해석을 실패
/// (`ERR_NAME_NOT_RESOLVED`)시켜, 사용자가 수동으로 재현한 "wtm SNI/DNS 0" 조건과 동일하게 만든다.
/// `ncpt.naver.com`(SDK)·`ssl.pstatic.net`(폰트/이미지) 등 다른 호스트는 건드리지 않는다.
const WASM_HOST_BLOCK_ARG: &str = "--host-resolver-rules=MAP wtm.pstatic.net ~NOTFOUND";

/// 실험 스위치(위 상수 참고)의 옵트인 여부. 기본 OFF — 값과 무관하게 존재만 하면 켜진 것으로 본다.
fn block_wasm_enabled() -> bool {
    std::env::var("PSTMACRO_BLOCK_WASM").is_ok()
}

/// 시스템 Chrome을 띄운다(게시·밴드 공용 — UA 오버라이드 없이 **네이티브 UA 유지**).
pub(crate) fn launch(headless: bool) -> Result<ChromeHandle, OrchestratorError> {
    launch_inner(headless, None)
}

/// 로그인 전용 런처: 설치 크롬 버전 **이하**에서 최신 실존 UA 를 골라 브라우저 전역 UA 문자열을
/// 통일해 띄운다(fpHash 묶음 완화 실험, 2026-07-08). 게시/밴드(`launch`)는 이걸 쓰지 않아 UA 무영향.
/// 이 경로는 항상 blocking 스레드에서 도므로 UA 버전 조회(네트워크)도 안전하다.
pub(crate) fn launch_for_login(headless: bool) -> Result<ChromeHandle, OrchestratorError> {
    let chrome = config::chrome_path().map_err(OrchestratorError::CommandFailed)?;
    let installed = installed_chrome_full_version(&chrome);
    let ua = ua::pick_for_installed(installed.as_deref());
    tracing::info!(
        installed = %installed.as_deref().unwrap_or("(확인 실패)"),
        chosen = %ua.full_version,
        "[CHROME] 로그인 UA 로테이션 — 설치 크롬 버전 이하에서 선택"
    );
    launch_inner(headless, Some(ua))
}

/// 공통 런처 본체. `ua`가 Some(로그인)이면 `--user-agent` 로 브라우저 전역 UA 문자열을 통일한다.
/// 포트가 확정될 때까지 기다린다.
fn launch_inner(headless: bool, ua: Option<UaProfile>) -> Result<ChromeHandle, OrchestratorError> {
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
    // 로그인 경로면(ua=Some) --user-agent 로 브라우저 전역 UA 문자열을 통일한다(페이지·iframe·
    // 서비스워커·요청 헤더까지 같은 문자열). 페이지의 Client Hints(sec-ch-ua/userAgentData)는
    // login.rs 가 Emulation.setUserAgentOverride 로 같은 버전에 맞춘다 — 문자열만 바꾸면 Client
    // Hints·서비스워커 UA 와 어긋나 봇탐지(_setHasLiedBrowser/NCAPTCHA_UA_DETECTION)에 걸린다.
    let ua_arg = ua.as_ref().map(|u| format!("--user-agent={}", u.user_agent));
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
        // 봇탐지 지문 일치(패킷 대조 2026-07-07): 빈 incognito 프로필은 UI 언어가 ko-KR 1개뿐이라
        // Accept-Language 헤더가 `ko-KR,ko`(2개)로 나가는데, login_flow의 스텔스 스크립트는
        // navigator.languages 를 `ko-KR,ko,en-US,en`(4개)로 위조한다 → JS와 헤더가 불일치해
        // wtm/ncaptcha 가 봇 신호로 읽는다. 실행 언어를 맞춰 Accept-Language 헤더도 같은 4개로
        // 내보내 navigator.languages 위조본과 **일치**시킨다(수동 브라우저처럼 JS↔헤더 동일).
        "--lang=ko-KR",
        "--accept-lang=ko-KR,ko,en-US,en",
        "about:blank",
    ];
    // 로그인 UA 오버라이드가 있으면 URL(about:blank) 바로 앞에 --user-agent 를 끼운다.
    if let Some(arg) = &ua_arg {
        let url_idx = args.len() - 1;
        args.insert(url_idx, arg.as_str());
    }
    // 실험(옵트인): 로그인 경로(ua=Some)에서 PSTMACRO_BLOCK_WASM 이 켜지면 봇탐지 WASM 엔진
    // 호스트(wtm.pstatic.net)를 DNS 로 막아 SDK 단독 경로를 강제한다(WASM_HOST_BLOCK_ARG 주석 참고).
    // 게시/밴드(ua=None)엔 적용하지 않아 카페/밴드 흐름은 무영향.
    if ua.is_some() && block_wasm_enabled() {
        let url_idx = args.len() - 1;
        args.insert(url_idx, WASM_HOST_BLOCK_ARG);
        tracing::warn!(
            "[CHROME][실험] PSTMACRO_BLOCK_WASM=on — wtm.pstatic.net(봇탐지 WASM 엔진) DNS 차단. \
             ncaptcha SDK 단독 경로 강제(지문 앵커 규명 20판 버스트용). ⚠️ 실험 전용 스위치."
        );
    }
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
        ua,
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

    #[test]
    fn wasm_block_arg_targets_only_engine_host() {
        // 봇탐지 WASM 엔진 호스트(wtm)만 해석 실패시키고, SDK(ncpt)·정적자원(ssl.pstatic.net)은
        // 건드리지 않아야 SDK 단독 경로 실험이 성립한다. 와일드카드로 pstatic 전체를 막으면 폰트/
        // 이미지까지 죽어 로그인 UI가 깨지므로, 정확히 wtm.pstatic.net 만 대상이어야 한다.
        assert!(WASM_HOST_BLOCK_ARG.contains("--host-resolver-rules="));
        assert!(WASM_HOST_BLOCK_ARG.contains("wtm.pstatic.net"));
        assert!(WASM_HOST_BLOCK_ARG.contains("~NOTFOUND"));
        assert!(!WASM_HOST_BLOCK_ARG.contains("ncpt"));
        assert!(!WASM_HOST_BLOCK_ARG.contains("ssl.pstatic.net"));
        assert!(!WASM_HOST_BLOCK_ARG.contains('*'));
    }
}
