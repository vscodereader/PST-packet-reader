use std::{env, fs, path::PathBuf};

use super::{config, error::OrchestratorError, types::RuntimePaths};

/// 애플리케이션 데이터 루트 디렉토리 경로를 가져온다.
pub fn app_data_root() -> Result<PathBuf, OrchestratorError> {
    let appdata = env::var_os("APPDATA").ok_or(OrchestratorError::MissingAppData)?;
    Ok(PathBuf::from(appdata)
        .join(config::APP_DATA_SUBDIR)
        .join(config::APP_NAME))
}

/// 루트 경로로부터 실행 시간 경로들을 구성한다.
pub fn paths_for_root(root: impl Into<PathBuf>) -> RuntimePaths {
    let root = root.into();
    let accounts_dir = root.join(config::DIR_ACCOUNTS);
    let cookies_dir = root.join(config::DIR_COOKIES);
    let scrcpy_dir = root.join(config::DIR_SCRCPY);
    let logs_dir = root.join(config::DIR_LOGS);
    RuntimePaths {
        accounts_file: accounts_dir.join(config::FILE_ACCOUNTS),
        adb_path: scrcpy_dir.join(config::FILE_ADB),
        root,
        accounts_dir,
        cookies_dir,
        scrcpy_dir,
        logs_dir,
    }
}

/// 필요한 런타임 디렉토리들을 생성한다.
pub fn ensure_runtime_dirs(paths: &RuntimePaths) -> Result<(), OrchestratorError> {
    fs::create_dir_all(&paths.accounts_dir)?;
    fs::create_dir_all(&paths.cookies_dir)?;
    fs::create_dir_all(&paths.scrcpy_dir)?;
    fs::create_dir_all(&paths.logs_dir)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_for_root_builds_expected_windows_layout() {
        let root = PathBuf::from(r"C:\Users\me\AppData\Roaming\.local\pstmacro");
        let paths = paths_for_root(&root);

        assert_eq!(paths.root, root);
        assert_eq!(
            paths.accounts_file,
            paths.accounts_dir.join("accounts.json")
        );
        assert_eq!(paths.adb_path, paths.scrcpy_dir.join("adb.exe"));
        assert!(paths.cookies_dir.ends_with("cookies"));
        assert!(paths.logs_dir.ends_with("logs"));
    }
}
