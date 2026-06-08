//! band_post용 소량 시간 헬퍼. `band_auth::util`은 비공개 모듈이라 재사용할 수 없어
//! 필요한 것만 복제한다.

use std::time::{SystemTime, UNIX_EPOCH};

/// 현재 시간을 밀리초 Unix timestamp로 반환한다(band api `ts` 쿼리용).
pub fn now_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn now_millis_is_post_2020() {
        assert!(now_millis() > 1_600_000_000_000);
    }
}
