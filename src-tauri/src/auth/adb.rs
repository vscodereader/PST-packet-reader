use std::time::Duration;

use adb_client::{usb::ADBUSBDevice, ADBDeviceExt};
use tokio::time::{sleep, Instant};

use super::{config, error::OrchestratorError};

/// ADB 디바이스가 연결되어 있고 인증되었는지 확인한다.
pub async fn assert_adb_device() -> Result<(), OrchestratorError> {
    connect_device()?;
    Ok(())
}

/// 진단용: ADB 디바이스가 연결되어 있는지 부작용 없이 확인한다.
///
/// `autodetect` 는 동기 USB 스캔이라 블로킹되므로 `spawn_blocking` 으로 감싼다.
/// 연결만 확인하고 디바이스를 즉시 해제하며, shell 명령(비행기 모드 등)은
/// 호출하지 않는다 — 상태 조회 전용이라 부작용이 없어야 한다.
pub async fn probe_adb_connection() -> Result<(), OrchestratorError> {
    tokio::task::spawn_blocking(|| connect_device().map(|_device| ()))
        .await
        .map_err(|e| OrchestratorError::CommandFailed(format!("adb probe join error: {e}")))?
}

/// 비행기 모드를 켬과 끔으로 토글하여 IP 변경을 유도한다.
/// 토글 전후의 외부 IP를 stderr로 출력해 `pnpm tauri dev` 콘솔에서 IP 회전 여부를
/// 직접 눈으로 확인할 수 있게 한다. (Samsung One UI는 `cmd connectivity airplane-mode`로
/// 토글해도 상단 버튼에 불이 안 들어올 수 있으나, IP가 바뀌면 라디오는 실제로 순환한 것.)
pub async fn toggle_airplane_mode() -> Result<(), OrchestratorError> {
    let before = fetch_external_ip().await;
    let mut device = connect_device()?;
    eprintln!("[ADB] ✈ 비행기모드 ON");
    run_shell_command(&mut device, "cmd connectivity airplane-mode enable")?;
    sleep(Duration::from_secs(config::ADB_AIRPLANE_ENABLE_SECS)).await;
    eprintln!("[ADB] ✈ 비행기모드 OFF — 인터넷 복구 대기");
    run_shell_command(&mut device, "cmd connectivity airplane-mode disable")?;
    wait_for_internet_connection(&mut device).await?;
    let after = fetch_external_ip().await;

    eprintln!("[ADB] ─────────── IP 회전 결과 ───────────");
    eprintln!("[ADB]   기존 IP: {before}");
    eprintln!("[ADB]   바뀐 IP: {after}");
    if before.starts_with('(') || after.starts_with('(') {
        eprintln!("[ADB]   (IP 확인 실패 — PC 인터넷/테더링 확인)");
    } else if before == after {
        eprintln!(
            "[ADB]   ⚠ IP가 그대로 — USB 테더링이 PC 기본 경로인지 / 통신사 CGNAT인지 확인 필요"
        );
    } else {
        eprintln!("[ADB]   ✓ IP 변경됨!");
    }
    eprintln!("[ADB] ────────────────────────────────────");
    Ok(())
}

/// PC의 현재 외부 IP를 조회한다(USB 테더링이면 = 폰 모바일 IP). best-effort.
/// blocking reqwest를 async 런타임에서 직접 호출하면 패닉하므로 spawn_blocking으로 감싼다.
async fn fetch_external_ip() -> String {
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

fn connect_device() -> Result<ADBUSBDevice, OrchestratorError> {
    ADBUSBDevice::autodetect().map_err(OrchestratorError::from)
}

fn run_shell_command(device: &mut ADBUSBDevice, command: &str) -> Result<(), OrchestratorError> {
    let mut stdout = Vec::new();
    let status = device.shell_command(&command, Some(&mut stdout), None)?;
    if let Some(code) = status {
        if code != 0 {
            let output = String::from_utf8_lossy(&stdout).trim().to_string();
            let message = if output.is_empty() {
                format!("adb shell `{command}` failed with exit code {code}")
            } else {
                format!("adb shell `{command}` failed with exit code {code}: {output}")
            };
            return Err(OrchestratorError::CommandFailed(message));
        }
    }

    Ok(())
}

async fn wait_for_internet_connection(device: &mut ADBUSBDevice) -> Result<(), OrchestratorError> {
    let timeout = Duration::from_secs(config::ADB_INTERNET_TIMEOUT_SECS);
    let interval = Duration::from_millis(config::ADB_INTERNET_POLL_INTERVAL_MS);
    let deadline = Instant::now() + timeout;

    loop {
        if has_internet_connection(device)? {
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

fn has_internet_connection(device: &mut ADBUSBDevice) -> Result<bool, OrchestratorError> {
    let output = run_shell_command_with_stdout(device, &internet_probe_command())?;
    Ok(output.lines().any(|line| line.trim() == "ok"))
}

fn run_shell_command_with_stdout(
    device: &mut ADBUSBDevice,
    command: &str,
) -> Result<String, OrchestratorError> {
    let mut stdout = Vec::new();
    let status = device.shell_command(&command, Some(&mut stdout), None)?;
    let output = String::from_utf8_lossy(&stdout).to_string();
    if let Some(code) = status {
        if code != 0 {
            let message = if output.trim().is_empty() {
                format!("adb shell `{command}` failed with exit code {code}")
            } else {
                format!(
                    "adb shell `{command}` failed with exit code {code}: {}",
                    output.trim()
                )
            };
            return Err(OrchestratorError::CommandFailed(message));
        }
    }

    Ok(output)
}

fn internet_probe_command() -> String {
    format!(
        "sh -c 'ping -c 1 -W 2 {} >/dev/null 2>&1 && echo ok || echo fail'",
        config::ADB_INTERNET_PING_HOST
    )
}

#[cfg(test)]
mod tests {
    use super::internet_probe_command;

    #[test]
    fn airplane_mode_shell_commands_do_not_include_adb_cli_prefix() {
        let commands = [
            "cmd connectivity airplane-mode enable",
            "cmd connectivity airplane-mode disable",
        ];

        for command in commands {
            assert!(!command.starts_with("adb "));
            assert!(!command.starts_with("shell "));
        }
    }

    #[test]
    fn internet_probe_command_reports_explicit_status() {
        let command = internet_probe_command();

        assert!(command.contains("ping -c 1"));
        assert!(command.contains("echo ok"));
        assert!(command.contains("echo fail"));
    }
}
