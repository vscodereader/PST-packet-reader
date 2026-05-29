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

/// 현재 시간을 밀리초 단위로 반환한다.
pub(crate) fn now_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
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
}
