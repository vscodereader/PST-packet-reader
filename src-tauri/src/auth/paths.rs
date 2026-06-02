use std::{env, fs, path::PathBuf};

use super::{config, error::OrchestratorError, types::RuntimePaths};

/// 애플리케이션 데이터 루트 디렉토리 경로를 가져온다.
///
/// Windows에서는 `%LOCALAPPDATA%`를 사용한다. WSL/Linux 등 `LOCALAPPDATA`가 없는
/// 환경에서는 `XDG_DATA_HOME` 또는 `~/.local/share`로 대체한다.
pub fn app_data_root() -> Result<PathBuf, OrchestratorError> {
    if let Some(appdata) = env::var_os("LOCALAPPDATA") {
        return Ok(PathBuf::from(appdata).join(config::APP_NAME));
    }

    if let Some(xdg) = env::var_os("XDG_DATA_HOME") {
        return Ok(PathBuf::from(xdg).join(config::APP_NAME));
    }

    if let Some(home) = env::var_os("HOME") {
        return Ok(PathBuf::from(home)
            .join(".local")
            .join("share")
            .join(config::APP_NAME));
    }

    Err(OrchestratorError::MissingLocalAppData)
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

        // LOCALAPPDATA(Windows)가 있으면 그것을 우선 사용한다.
        env::set_var("LOCALAPPDATA", temp.path());
        assert_eq!(app_data_root().unwrap(), temp.path().join(config::APP_NAME));

        // CDP 로그인 전환 이후 로그인은 WSL/Linux 네이티브로도 동작하므로,
        // LOCALAPPDATA가 없으면 XDG_DATA_HOME으로 대체한다.
        env::remove_var("LOCALAPPDATA");
        let xdg = tempfile::tempdir().unwrap();
        env::set_var("XDG_DATA_HOME", xdg.path());
        assert_eq!(app_data_root().unwrap(), xdg.path().join(config::APP_NAME));

        // LOCALAPPDATA·XDG_DATA_HOME·HOME이 모두 없을 때만 명확한 오류를 낸다.
        // (HOME은 테스트 중 일시적으로 제거하고 곧바로 복원한다.)
        env::remove_var("XDG_DATA_HOME");
        let saved_home = env::var_os("HOME");
        env::remove_var("HOME");
        assert!(matches!(
            app_data_root(),
            Err(OrchestratorError::MissingLocalAppData)
        ));
        if let Some(home) = saved_home {
            env::set_var("HOME", home);
        }
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
