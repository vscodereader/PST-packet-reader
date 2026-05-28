use std::{env, fs, path::PathBuf};

use super::{error::OrchestratorError, types::RuntimePaths};

pub fn app_data_root() -> Result<PathBuf, OrchestratorError> {
    let appdata = env::var_os("APPDATA").ok_or(OrchestratorError::MissingAppData)?;
    Ok(PathBuf::from(appdata).join(".local").join("pstmacro"))
}

pub fn paths_for_root(root: impl Into<PathBuf>) -> RuntimePaths {
    let root = root.into();
    let accounts_dir = root.join("accounts");
    let cookies_dir = root.join("cookies");
    let scrcpy_dir = root.join("scrcpy");
    let logs_dir = root.join("logs");
    RuntimePaths {
        accounts_file: accounts_dir.join("accounts.json"),
        adb_path: scrcpy_dir.join("adb.exe"),
        root,
        accounts_dir,
        cookies_dir,
        scrcpy_dir,
        logs_dir,
    }
}

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
