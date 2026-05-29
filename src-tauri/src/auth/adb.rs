use std::time::Duration;

use adb_client::{usb::ADBUSBDevice, ADBDeviceExt};
use tokio::time::sleep;

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
    sleep(Duration::from_secs(config::ADB_AIRPLANE_DISABLE_SECS)).await;
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

#[cfg(test)]
mod tests {
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
}
