use std::{
    fs,
    io::{self, Cursor},
    path::{Path, PathBuf},
};

use zip::ZipArchive;

use super::{config, error::OrchestratorError, types::RuntimePaths};

/// ADB 실행 파일을 확인하고 없으면 다운로드하여 설치한다.
pub async fn ensure_adb(paths: &RuntimePaths) -> Result<(), OrchestratorError> {
    if paths.adb_path.exists() {
        return Ok(());
    }

    let bytes = wreq::get(config::SCRCPY_URL).send().await?.bytes().await?;
    extract_scrcpy_zip(&bytes, &paths.scrcpy_dir)?;

    if !paths.adb_path.exists() {
        return Err(OrchestratorError::MissingAdb);
    }
    Ok(())
}

fn extract_scrcpy_zip(bytes: &[u8], output_dir: &Path) -> Result<(), OrchestratorError> {
    let reader = Cursor::new(bytes);
    let mut archive = ZipArchive::new(reader)?;

    for index in 0..archive.len() {
        let mut file = archive.by_index(index)?;
        if file.is_dir() {
            continue;
        }

        let Some(path) = file.enclosed_name() else {
            continue;
        };
        let mut components = path.components();
        components.next();
        let relative: PathBuf = components.collect();
        if relative.as_os_str().is_empty() {
            continue;
        }

        let output_path = output_dir.join(relative);
        if let Some(parent) = output_path.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut output = fs::File::create(output_path)?;
        io::copy(&mut file, &mut output)?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {

    #[test]
    fn scrcpy_url_targets_v4_win64_zip() {
        assert_eq!(
            super::config::SCRCPY_URL,
            "https://github.com/Genymobile/scrcpy/releases/download/v4.0/scrcpy-win64-v4.0.zip"
        );
    }
}
