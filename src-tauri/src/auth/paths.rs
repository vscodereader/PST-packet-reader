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
}
