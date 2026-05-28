use std::{path::Path, process::Command, time::Duration};

use tokio::time::sleep;

use super::error::OrchestratorError;

pub async fn assert_adb_device(adb_path: &Path) -> Result<(), OrchestratorError> {
    let output = run_command(adb_path, &["devices"])?;
    let has_device = output
        .lines()
        .skip(1)
        .any(|line| line.split_whitespace().nth(1) == Some("device"));
    if !has_device {
        return Err(OrchestratorError::CommandFailed(
            "ADB device is not connected or unauthorized".to_string(),
        ));
    }
    Ok(())
}

pub async fn toggle_airplane_mode(adb_path: &Path) -> Result<(), OrchestratorError> {
    run_command(
        adb_path,
        &["shell", "cmd", "connectivity", "airplane-mode", "enable"],
    )?;
    sleep(Duration::from_secs(2)).await;
    run_command(
        adb_path,
        &["shell", "cmd", "connectivity", "airplane-mode", "disable"],
    )?;
    sleep(Duration::from_secs(8)).await;
    Ok(())
}

fn run_command(program: &Path, args: &[&str]) -> Result<String, OrchestratorError> {
    let output = Command::new(program).args(args).output()?;
    if !output.status.success() {
        return Err(OrchestratorError::CommandFailed(format!(
            "{} {} failed: {}",
            program.display(),
            args.join(" "),
            String::from_utf8_lossy(&output.stderr)
        )));
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}
