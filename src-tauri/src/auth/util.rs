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

/// 로그용 ID 마스킹: 앞 3글자만 남기고 나머지는 `****`. 3자 미만이면 전부 가린다.
/// 비밀번호는 어떤 경우에도 로그에 넣지 않으므로 마스킹 대상에서 제외한다.
pub(crate) fn mask_id(id: &str) -> String {
    let chars: Vec<char> = id.chars().collect();
    if chars.len() < 3 {
        return "****".to_string();
    }
    let prefix: String = chars.iter().take(3).collect();
    format!("{prefix}****")
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

    #[test]
    fn mask_id_keeps_prefix_and_hides_rest() {
        assert_eq!(mask_id("choisw0404"), "cho****");
        assert_eq!(mask_id("ab"), "****"); // 3자 미만은 전부 가린다
        assert_eq!(mask_id(""), "****");
    }
}
