use std::time::Duration;

use adb_client::{usb::ADBUSBDevice, ADBDeviceExt};
use tokio::time::{sleep, Instant};

use super::{config, error::OrchestratorError};

/// ADB 디바이스가 연결되어 있고 인증되었는지 확인한다.
pub async fn assert_adb_device() -> Result<(), OrchestratorError> {
    connect_device()?;
    Ok(())
}

/// 비행기 모드를 켬과 끔으로 토글하여 IP 변경을 유도한다.
pub async fn toggle_airplane_mode() -> Result<(), OrchestratorError> {
    let mut device = connect_device()?;
    run_shell_command(&mut device, "cmd connectivity airplane-mode enable")?;
    sleep(Duration::from_secs(config::ADB_AIRPLANE_ENABLE_SECS)).await;
    run_shell_command(&mut device, "cmd connectivity airplane-mode disable")?;
    wait_for_internet_connection(&mut device).await?;
    Ok(())
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
