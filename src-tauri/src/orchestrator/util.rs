use std::time::{SystemTime, UNIX_EPOCH};

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

pub(crate) fn now_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn safe_file_stem_replaces_path_separators() {
        assert_eq!(safe_file_stem("a/b\\c@example.com"), "a_b_c_example.com");
    }
}
