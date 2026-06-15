//! `auth::util`은 비공개라 재사용할 수 없어, band 모듈이 쓰는 소량의 시간/파일명 헬퍼를
//! 동일 구현으로 복제한다(네이버 `auth/util.rs` 미러).

use std::time::{SystemTime, UNIX_EPOCH};

/// 파일명으로 사용 가능하도록 문자열을 정제한다.
pub(crate) fn safe_file_stem(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.') {
                ch
            } else {
                '_'
            }
        })
        .collect()
}

/// 현재 시간을 초 단위 Unix timestamp로 반환한다.
pub(crate) fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn safe_file_stem_replaces_path_separators() {
        assert_eq!(safe_file_stem("a/b\\c@example.com"), "a_b_c_example.com");
    }

    #[test]
    fn now_helpers_return_post_2020_unix_time() {
        // 2020-09-13 이후의 합리적인 하한으로 now_secs가 동작함을 확인한다.
        assert!(now_secs() > 1_600_000_000);
    }
}
