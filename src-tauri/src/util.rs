//! 작은 공유 유틸리티.
use std::backtrace::Backtrace;
use std::time::{SystemTime, UNIX_EPOCH};

/// 현재 시각을 epoch milliseconds로 반환. 시계 오류 시 0.
pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}

/// 현재 호출 지점의 런타임 백트레이스를 문자열로 캡처한다(#199, "자세히 보기" trace용).
///
/// 게시/댓글 실패는 panic이 아니라 에러 값으로 흐르므로 백트레이스가 자동 생성되지 않는다 —
/// 실패 지점에서 이 함수를 호출해 호출 스택을 직접 캡처한다. `force_capture`라 `RUST_BACKTRACE`
/// 환경변수 없이도 항상 캡처한다.
///
/// 프레임이 `<unknown>` 대신 함수명으로 해석되려면 빌드에 디버그 정보가 있어야 한다
/// (`[profile.release] debug = true`, Cargo.toml 참조). 전 플랫폼:
///   - Linux/macOS: DWARF가 바이너리에 있어 in-process로 해석된다.
///   - Windows: 별도 `.pdb`가 `.exe` 옆에 있어야 dbghelp가 해석한다.
pub fn backtrace_string() -> String {
    Backtrace::force_capture().to_string()
}

/// 호출 지점의 `"함수경로 (파일:줄)"`을 **컴파일 시점 문자열**로 만든다(#199). [`backtrace_string`]
/// 의 앵커 — 백트레이스 심볼이 (디버그 정보 누락 등으로) 일부 `<unknown>`으로 떠도, 실제 실패
/// 지점만큼은 `file!()`/`line!()`/`type_name`(모두 컴파일타임)으로 바이너리에 박혀 항상 보인다.
#[macro_export]
macro_rules! here {
    () => {{
        // 호출이 일어난 함수 안에 정의되는 로컬 fn — 그 type_name이 바깥 함수 경로를 담는다.
        fn __here_fn() {}
        fn __type_name_of<T>(_: T) -> &'static str {
            ::std::any::type_name::<T>()
        }
        let __raw = __type_name_of(__here_fn);
        // "crate::module::enclosing_fn::__here_fn" → 끝의 "::__here_fn" 제거.
        let __func = __raw.strip_suffix("::__here_fn").unwrap_or(__raw);
        ::std::format!("{} ({}:{})", __func, ::core::file!(), ::core::line!())
    }};
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn now_ms_is_positive_and_post_2020() {
        // 2020-01-01 = 1_577_836_800_000 ms
        assert!(now_ms() > 1_577_836_800_000);
    }

    #[test]
    fn backtrace_string_captures_non_empty_frames() {
        // force_capture라 RUST_BACKTRACE 없이도 스택을 캡처한다 — 자세히 보기 trace의 본문.
        // 디버그 정보가 있는 빌드(테스트=dev 프로파일)에선 이 테스트 함수명이 프레임에 보인다.
        let bt = backtrace_string();
        assert!(!bt.trim().is_empty(), "백트레이스가 비어선 안 됨");
        assert!(
            bt.contains("backtrace_string_captures_non_empty_frames"),
            "캡처한 스택에 호출 함수가 보여야 함(심볼 해석됨): {bt}"
        );
    }

    #[test]
    fn here_macro_reports_enclosing_function_and_file_line() {
        // 컴파일타임에 "함수경로 (파일:줄)"이 박힌다 — PDB/심볼 없이 항상 해석됨.
        let loc = crate::here!();
        assert!(
            loc.contains("here_macro_reports_enclosing_function_and_file_line"),
            "함수명이 들어가야 함: {loc}"
        );
        assert!(loc.contains("util.rs"), "파일명이 들어가야 함: {loc}");
    }
}
