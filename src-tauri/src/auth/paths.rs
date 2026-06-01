use std::{env, fs, path::PathBuf};

use super::{config, error::OrchestratorError, types::RuntimePaths};

/// 애플리케이션 데이터 루트 디렉토리 경로를 가져온다.
pub fn app_data_root() -> Result<PathBuf, OrchestratorError> {
    let appdata = env::var_os("LOCALAPPDATA").ok_or(OrchestratorError::MissingLocalAppData)?;
    Ok(PathBuf::from(appdata).join(config::APP_NAME))
}

/// 루트 경로로부터 실행 시간 경로들을 구성한다.
pub fn paths_for_root(root: impl Into<PathBuf>) -> RuntimePaths {
    let root = root.into();
    let accounts_dir = root.join(config::DIR_ACCOUNTS);
    let cookies_dir = root.join(config::DIR_COOKIES);
    let logs_dir = root.join(config::DIR_LOGS);
    RuntimePaths {
        accounts_file: accounts_dir.join(config::FILE_ACCOUNTS),
        root,
        accounts_dir,
        cookies_dir,
        logs_dir,
    }
}

/// 필요한 런타임 디렉토리들을 생성한다.
pub fn ensure_runtime_dirs(paths: &RuntimePaths) -> Result<(), OrchestratorError> {
    fs::create_dir_all(&paths.accounts_dir)?;
    fs::create_dir_all(&paths.cookies_dir)?;
    fs::create_dir_all(&paths.logs_dir)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_for_root_builds_expected_windows_layout() {
        let root = PathBuf::from(r"C:\Users\me\AppData\Local\pstmacro");
        let paths = paths_for_root(&root);

        assert_eq!(paths.root, root);
        assert_eq!(
            paths.accounts_file,
            paths.accounts_dir.join("accounts.json")
        );
        assert!(paths.cookies_dir.ends_with("cookies"));
        assert!(paths.logs_dir.ends_with("logs"));
    }

    #[test]
    fn app_data_root_joins_localappdata_with_app_name() {
        let _guard = config::ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let temp = tempfile::tempdir().unwrap();

        env::set_var("LOCALAPPDATA", temp.path());
        assert_eq!(app_data_root().unwrap(), temp.path().join(config::APP_NAME));

        env::remove_var("LOCALAPPDATA");
        assert!(matches!(
            app_data_root(),
            Err(OrchestratorError::MissingLocalAppData)
        ));
    }

    #[test]
    fn ensure_runtime_dirs_creates_every_subdirectory() {
        let temp = tempfile::tempdir().unwrap();
        let paths = paths_for_root(temp.path());

        ensure_runtime_dirs(&paths).unwrap();

        assert!(paths.accounts_dir.is_dir());
        assert!(paths.cookies_dir.is_dir());
        assert!(paths.logs_dir.is_dir());
    }
}
