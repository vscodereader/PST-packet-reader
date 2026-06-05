//! 작은 공유 유틸리티.
use std::time::{SystemTime, UNIX_EPOCH};

/// 현재 시각을 epoch milliseconds로 반환. 시계 오류 시 0.
pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn now_ms_is_positive_and_post_2020() {
        // 2020-01-01 = 1_577_836_800_000 ms
        assert!(now_ms() > 1_577_836_800_000);
    }
}
