use std::process::Command;
use std::time::Duration;

use tokio::time::{sleep, Instant};

use super::{config, error::OrchestratorError};

/// 표준 adb CLI 실행 파일. winget `Google.PlatformTools` 설치 시 PATH에 등록된다.
/// raw-USB(adb_client) 대신 표준 adb 서버를 거치므로, 제조사 ADB 드라이버(삼성 등)
/// 그대로 동작하며 Windows에서 WinUSB(Zadig) 교체 없이 IP 로테이션이 된다.
/// (커밋 2075d4e 가 표준 adb→adb_client raw-USB 로 바꾸면서 삼성 폰에서 Access denied 가
///  났던 것을, 그 이전의 표준 adb CLI 방식으로 되돌린 것이다.)
const ADB_BIN: &str = "adb";

/// ADB 디바이스가 연결되어 있고 인증되었는지 확인한다.
pub async fn assert_adb_device() -> Result<(), OrchestratorError> {
    if !has_authorized_device(&run_adb_timed(vec!["devices".to_string()]).await?) {
        return Err(OrchestratorError::CommandFailed(
            "ADB 디바이스가 연결되지 않았거나 인증되지 않았습니다 (adb devices에 'device' 없음)"
                .to_string(),
        ));
    }
    Ok(())
}

/// `run_adb`를 spawn_blocking으로 실행하고 명령 1건마다 타임아웃을 건다(#210). adb 서버가
/// 행(hang)이면 `cmd.output()`이 무한 블록되는데, async 안에서 직접 호출하면 런타임 스레드를
/// 막아 로그인 큐가 멈춘다. blocking 풀로 분리하고 timeout으로 상한을 둔다(초과 시 블로킹
/// 스레드는 남을 수 있으나 호출부는 에러로 진행 — 로그인 워커가 다음 계정으로 넘어간다).
async fn run_adb_timed(args: Vec<String>) -> Result<String, OrchestratorError> {
    let label = args.join(" ");
    let task = tokio::task::spawn_blocking(move || {
        let refs: Vec<&str> = args.iter().map(String::as_str).collect();
        run_adb(&refs)
    });
    match tokio::time::timeout(Duration::from_secs(config::ADB_STEP_TIMEOUT_SECS), task).await {
        Ok(joined) => joined
            .map_err(|e| OrchestratorError::CommandFailed(format!("adb 작업 스레드 오류: {e}")))?,
        Err(_) => Err(OrchestratorError::CommandFailed(format!(
            "adb {label} 응답 시간 초과({}초)",
            config::ADB_STEP_TIMEOUT_SECS
        ))),
    }
}

/// 진단용: ADB 디바이스가 연결되어 있는지 부작용 없이 확인한다.
///
/// `adb devices` 는 블로킹 프로세스 호출이라 `spawn_blocking` 으로 감싼다.
/// 연결만 확인하고 shell 명령(비행기 모드 등)은 호출하지 않는다 — 상태 조회 전용이라
/// 부작용이 없어야 한다.
pub async fn probe_adb_connection() -> Result<(), OrchestratorError> {
    tokio::task::spawn_blocking(|| {
        if has_authorized_device(&run_adb(&["devices"])?) {
            Ok(())
        } else {
            Err(OrchestratorError::CommandFailed(
                "ADB 디바이스가 연결되지 않았거나 인증되지 않았습니다".to_string(),
            ))
        }
    })
    .await
    .map_err(|e| OrchestratorError::CommandFailed(format!("adb probe join error: {e}")))?
}

/// 비행기모드 토글(IP 회전) 결과. 프론트가 토스트·알림에 "원래/바뀐 IP·변경 여부"를
/// 표시하는 데 쓴다(#247 후속). before/after가 `(`로 시작하면 IP 확인 실패라 changed=false.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct IpRotation {
    pub before: String,
    pub after: String,
    pub changed: bool,
}

/// 비행기 모드를 켰다 꺼 IP 변경을 유도한다(로그인·밴드·"IP 변경" 버튼 공용).
///
/// 고정 대기 없이 **실제 상태**로 진행한다 — ON 후 폰 인터넷이 실제로 끊긴 걸(라디오 드롭)
/// `ping 8.8.8.8` fail로 확인할 때까지 기다렸다가 OFF하고, 그 뒤 외부 IP가 `before`와 달라질
/// 때까지 폴링해 **IP가 바뀐 순간 즉시** 반환한다. 토글 전후 IP를 로그로 남겨 콘솔에서 회전
/// 여부를 눈으로 확인할 수 있다. (Samsung One UI는 `cmd connectivity airplane-mode`로 토글해도
/// 상단 버튼에 불이 안 들어올 수 있으나, IP가 바뀌면 라디오는 실제로 순환한 것.)
pub async fn toggle_airplane_mode() -> Result<IpRotation, OrchestratorError> {
    let before = fetch_external_ip().await;
    tracing::info!("[ADB] ✈ 비행기모드 ON");
    run_adb_timed(
        airplane_mode_args(true)
            .iter()
            .map(|s| s.to_string())
            .collect(),
    )
    .await?;
    // ON 동안 라디오가 실제로 끊긴 걸(폰 인터넷 차단) 확인할 때까지 기다린다(고정 대기 없음).
    wait_until_phone_offline().await;
    tracing::info!("[ADB] ✈ 비행기모드 OFF — 인터넷 복구 대기");
    run_adb_timed(
        airplane_mode_args(false)
            .iter()
            .map(|s| s.to_string())
            .collect(),
    )
    .await?;
    // OFF 후: 인터넷 복구를 기다렸다가, 외부 IP가 실제로 바뀔 때까지 폴링해 바뀌면 즉시 반환한다.
    wait_for_internet_connection().await?;
    let after = wait_for_ip_change(&before).await;

    tracing::info!("[ADB] ─────────── IP 회전 결과 ───────────");
    tracing::info!("[ADB]   기존 IP: {before}");
    tracing::info!("[ADB]   바뀐 IP: {after}");
    let changed = !before.starts_with('(') && !after.starts_with('(') && before != after;
    if before.starts_with('(') || after.starts_with('(') {
        tracing::info!("[ADB]   (IP 확인 실패 — PC 인터넷/테더링 확인)");
    } else if !changed {
        tracing::info!(
            "[ADB]   ⚠ IP가 그대로 — USB 테더링이 PC 기본 경로인지 / 통신사 CGNAT인지 확인 필요"
        );
    } else {
        tracing::info!("[ADB]   ✓ IP 변경됨!");
    }
    tracing::info!("[ADB] ────────────────────────────────────");
    Ok(IpRotation {
        before,
        after,
        changed,
    })
}

/// `adb devices` 출력에 인증된(`device`) 디바이스가 하나라도 있는지 판별한다.
/// (`unauthorized`/`offline`/빈 목록은 false)
fn has_authorized_device(devices_output: &str) -> bool {
    devices_output
        .lines()
        .skip(1)
        .any(|line| line.split_whitespace().nth(1) == Some("device"))
}

/// 비행기모드 토글용 adb 인자. enable/disable만 다르다.
fn airplane_mode_args(enable: bool) -> [&'static str; 5] {
    [
        "shell",
        "cmd",
        "connectivity",
        "airplane-mode",
        if enable { "enable" } else { "disable" },
    ]
}

/// 비행기모드 ON 후, 폰 인터넷이 **실제로 끊길 때까지**(라디오 드롭) ADB ping 프로브로 폴링하고
/// 끊긴 걸 확인하면 즉시 반환한다(고정 대기 없음). 설정 플래그가 아니라
/// 실제 연결 상태를 보므로, 라디오가 끊겨 IP가 바뀔 토대를 보장한다. 끝내 못 끊으면(WiFi 유지 등)
/// 상한(ADB_INTERNET_TIMEOUT_SECS)을 넘겨 그대로 진행한다.
async fn wait_until_phone_offline() {
    let deadline = Instant::now() + Duration::from_secs(config::ADB_INTERNET_TIMEOUT_SECS);
    loop {
        // 프로브 실패(인터넷 끊김) = 라디오가 끊긴 것. 조회 에러는 아직 온라인으로 보고 계속 기다린다.
        if !has_internet_connection().await.unwrap_or(true) {
            tracing::info!("[ADB]   └ 라디오 끊김 확인(폰 인터넷 차단) — OFF로 진행");
            return;
        }
        if Instant::now() >= deadline {
            tracing::info!("[ADB]   └ ⚠ 라디오 끊김 미확인(상한 초과) — 그대로 진행");
            return;
        }
        sleep(Duration::from_millis(config::ADB_INTERNET_POLL_INTERVAL_MS)).await;
    }
}

/// 외부 IP가 `before`와 **실제로 달라질 때까지** api.ipify.org를 폴링하고, 바뀌면 즉시 그 IP를
/// 반환한다. 비정상 응답("(확인 실패)" 등)은 아직 안 바뀐 것으로 보고
/// 재시도한다. 상한(ADB_INTERNET_TIMEOUT_SECS)을 넘으면 마지막으로 읽은 값을 반환한다(같으면
/// 호출부가 "그대로"로 로깅 — 보통 CGNAT/테더링 경로 문제).
async fn wait_for_ip_change(before: &str) -> String {
    let deadline = Instant::now() + Duration::from_secs(config::ADB_INTERNET_TIMEOUT_SECS);
    loop {
        let ip = fetch_external_ip().await;
        if is_valid_changed_ip(before, &ip) {
            tracing::info!("[ADB]   └ IP 변경 확인({ip}) — 즉시 진행");
            return ip;
        }
        if Instant::now() >= deadline {
            // 상한 초과 — 안 바뀐 마지막 IP를 반환(호출부가 "그대로"로 로깅).
            return ip;
        }
        sleep(Duration::from_millis(config::ADB_INTERNET_POLL_INTERVAL_MS)).await;
    }
}

/// `candidate`가 `before`와 다른 '유효한' 외부 IP인지(순수 함수). "(확인 실패)"처럼 괄호로
/// 시작하는 비정상 응답이나 빈 값, before와 같은 값은 false.
fn is_valid_changed_ip(before: &str, candidate: &str) -> bool {
    !candidate.is_empty() && !candidate.starts_with('(') && candidate != before
}

/// 표준 adb CLI를 실행하고 stdout을 반환한다. 실행 실패/비-0 종료는 에러로 변환한다.
fn run_adb(args: &[&str]) -> Result<String, OrchestratorError> {
    let mut cmd = Command::new(ADB_BIN);
    cmd.args(args);
    // 윈도우에서 adb(콘솔 앱)를 실행할 때 검은 콘솔 창이 깜빡이는 것을 막는다.
    // 알림 화면 진입 시 환경 진단이 `adb devices` 를 호출하는데, 이 플래그가 없으면
    // 매번 콘솔 창이 잠깐 떴다 사라진다(CREATE_NO_WINDOW). 다른 OS엔 영향 없음.
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let output = cmd.output().map_err(|e| {
        OrchestratorError::CommandFailed(format!(
            "adb 실행 실패: {e} — PATH에 adb가 있는지 확인 (winget install Google.PlatformTools)"
        ))
    })?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let detail = if stderr.is_empty() {
            "(stderr 없음)".to_string()
        } else {
            stderr
        };
        return Err(OrchestratorError::CommandFailed(format!(
            "adb {} 실패: {detail}",
            args.join(" ")
        )));
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

/// PC의 현재 외부 IP를 조회한다(USB 테더링이면 = 폰 모바일 IP). best-effort.
/// blocking reqwest를 async 런타임에서 직접 호출하면 패닉하므로 spawn_blocking으로 감싼다.
/// 실패 시 `(확인 실패)`로 시작하는 문자열을 돌려준다 — 호출부의 IP 비교는 이를 fail-open
/// 으로 본다(`ip_matches`).
pub(crate) async fn fetch_external_ip() -> String {
    tokio::task::spawn_blocking(|| {
        reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(5))
            .build()
            .ok()
            .and_then(|client| client.get("https://api.ipify.org").send().ok())
            .and_then(|response| response.text().ok())
            .map(|text| text.trim().to_owned())
            .filter(|text| !text.is_empty())
            .unwrap_or_else(|| "(확인 실패)".to_owned())
    })
    .await
    .unwrap_or_else(|_| "(확인 실패)".to_owned())
}

async fn wait_for_internet_connection() -> Result<(), OrchestratorError> {
    let timeout = Duration::from_secs(config::ADB_INTERNET_TIMEOUT_SECS);
    let interval = Duration::from_millis(config::ADB_INTERNET_POLL_INTERVAL_MS);
    let deadline = Instant::now() + timeout;

    loop {
        // 비행기모드 해제 직후엔 단말 라디오가 순환 중이라 `adb shell`(ping 프로브)이 일시적으로
        // 실패(device offline)하거나 ping이 fail로 나올 수 있다. 이를 하드 에러로 올리면 로그인
        // 전체가 'command failed: adb shell ... ping'으로 죽으므로(#210 E2E 간헐 실패), 일시
        // 실패는 "아직 연결 안 됨"으로 보고 데드라인까지 재시도한다. 끝내 안 되면 아래에서
        // 명확한 "복구 시간 초과" 에러로 마무리한다.
        if has_internet_connection().await.unwrap_or(false) {
            return Ok(());
        }

        if Instant::now() >= deadline {
            return Err(OrchestratorError::CommandFailed(format!(
                "internet connection was not restored within {} seconds",
                config::ADB_INTERNET_TIMEOUT_SECS
            )));
        }

        sleep(interval).await;
    }
}

async fn has_internet_connection() -> Result<bool, OrchestratorError> {
    let probe = internet_probe_command();
    let output = run_adb_timed(vec!["shell".to_string(), probe]).await?;
    Ok(output.lines().any(|line| line.trim() == "ok"))
}

fn internet_probe_command() -> String {
    format!(
        "sh -c 'ping -c 1 -W 2 {} >/dev/null 2>&1 && echo ok || echo fail'",
        config::ADB_INTERNET_PING_HOST
    )
}

#[cfg(test)]
mod tests {
    use super::{
        airplane_mode_args, has_authorized_device, internet_probe_command, is_valid_changed_ip,
    };

    #[test]
    fn is_valid_changed_ip_detects_real_change() {
        // IP 회전이 'IP가 실제로 바뀜'을 판정하는 근거.
        assert!(is_valid_changed_ip("1.1.1.1", "2.2.2.2")); // 유효 + 다름 → 바뀜
        assert!(!is_valid_changed_ip("1.1.1.1", "1.1.1.1")); // 같음 → 아직
        assert!(!is_valid_changed_ip("1.1.1.1", "(확인 실패)")); // 비정상 응답 → 아직
        assert!(!is_valid_changed_ip("1.1.1.1", "")); // 빈 값 → 아직
        // before가 비정상이었어도, 유효 IP를 새로 받으면 바뀐 것으로 본다.
        assert!(is_valid_changed_ip("(확인 실패)", "3.3.3.3"));
    }

    #[test]
    fn airplane_mode_args_use_cli_shell_form() {
        assert_eq!(
            airplane_mode_args(true),
            ["shell", "cmd", "connectivity", "airplane-mode", "enable"]
        );
        assert_eq!(
            airplane_mode_args(false),
            ["shell", "cmd", "connectivity", "airplane-mode", "disable"]
        );
    }

    #[test]
    fn has_authorized_device_accepts_only_authorized() {
        let authorized = "List of devices attached\nR3CN409J22V\tdevice\n";
        let unauthorized = "List of devices attached\nR3CN409J22V\tunauthorized\n";
        let offline = "List of devices attached\nR3CN409J22V\toffline\n";
        let empty = "List of devices attached\n";

        assert!(has_authorized_device(authorized));
        assert!(!has_authorized_device(unauthorized));
        assert!(!has_authorized_device(offline));
        assert!(!has_authorized_device(empty));
    }

    #[test]
    fn internet_probe_command_reports_explicit_status() {
        let command = internet_probe_command();

        assert!(command.contains("ping -c 1"));
        assert!(command.contains("echo ok"));
        assert!(command.contains("echo fail"));
    }
}
