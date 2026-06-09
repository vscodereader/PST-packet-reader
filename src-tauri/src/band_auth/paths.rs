//! band 전용 쿠키 디렉토리 경로 헬퍼. 네이버 쿠키(`cookies/`)와 충돌하지 않도록
//! band 세션 쿠키는 `app_data_root()/cookies-band/` 아래에 저장한다.

use std::{fs, path::PathBuf};

use crate::auth::{app_data_root, OrchestratorError};

use super::util::safe_file_stem;

/// band 쿠키를 저장하는 디렉토리 이름. 네이버 `cookies`와 분리한다.
pub(crate) const DIR_COOKIES_BAND: &str = "cookies-band";

/// 루트 경로로부터 band 쿠키 디렉토리 경로를 구성한다(순수 함수).
pub(crate) fn band_cookies_dir(root: impl Into<PathBuf>) -> PathBuf {
    root.into().join(DIR_COOKIES_BAND)
}

/// 앱 데이터 루트 아래의 band 쿠키 디렉토리를 반환한다.
pub(crate) fn band_cookies_dir_for_app_data() -> Result<PathBuf, OrchestratorError> {
    Ok(band_cookies_dir(app_data_root()?))
}

/// band 쿠키 디렉토리를 생성(존재 보장)한다.
pub(crate) fn ensure_band_cookies_dir(dir: &std::path::Path) -> Result<(), OrchestratorError> {
    fs::create_dir_all(dir)?;
    Ok(())
}

/// 계정 id에 해당하는 band 쿠키 파일 경로(`cookies-band/{safe_id}.json`)를 구성한다.
pub(crate) fn band_cookie_file_path(dir: &std::path::Path, account_id: &str) -> PathBuf {
    dir.join(format!("{}.json", safe_file_stem(account_id)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn band_cookies_dir_ends_with_cookies_band() {
        let root = PathBuf::from(r"C:\Users\me\AppData\Local\pstmacro");
        let dir = band_cookies_dir(&root);
        assert!(dir.ends_with("cookies-band"));
        assert_eq!(dir, root.join("cookies-band"));
    }

    #[test]
    fn band_cookie_file_path_sanitizes_account_id() {
        let dir = PathBuf::from("/tmp/cookies-band");
        let path = band_cookie_file_path(&dir, "a/b@example.com");
        assert_eq!(path, dir.join("a_b_example.com.json"));
    }

    #[test]
    fn ensure_band_cookies_dir_creates_directory() {
        let temp = tempfile::tempdir().unwrap();
        let dir = band_cookies_dir(temp.path());
        ensure_band_cookies_dir(&dir).unwrap();
        assert!(dir.is_dir());
    }
}
