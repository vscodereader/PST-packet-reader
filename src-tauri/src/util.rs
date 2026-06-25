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

/// `std::error::Error`의 `source()` 체인을 끝까지 펼친다(인접 중복 제거). 순수 함수.
///
/// reqwest 등 상위 에러는 Display에 "error sending request for url (...)"처럼 wrapper만
/// 보여주고 **진짜 원인(연결 거부/리셋/타임아웃/DNS)은 source() 체인에 숨긴다**. 그 체인을
/// 펼쳐 사람이 원인을 알 수 있게 한다.
pub fn error_source_chain(e: &dyn std::error::Error) -> Vec<String> {
    let mut chain = Vec::new();
    let mut src = e.source();
    while let Some(s) = src {
        let msg = s.to_string();
        // hyper/reqwest가 같은 사유를 중첩해 싣는 경우가 있어 인접 중복은 한 번만 남긴다.
        if chain.last() != Some(&msg) {
            chain.push(msg);
        }
        src = s.source();
    }
    chain
}

/// 전송 오류 상세 한 줄을 만든다(순수 함수): `[분류] {wrapper} → 원인: {체인}`.
/// 체인이 비면 분류 + wrapper만.
pub fn format_transport_detail(display: &str, kind: &str, chain: &[String]) -> String {
    if chain.is_empty() {
        format!("[{kind}] {display}")
    } else {
        format!("[{kind}] {display} → 원인: {}", chain.join(" → "))
    }
}

/// reqwest 전송 오류를 분류(타임아웃/연결실패/본문/디코드/요청)한다. source() 체인이 가린
/// "왜"를 한눈에 보이게 하는 라벨.
fn reqwest_kind(e: &reqwest::Error) -> &'static str {
    if e.is_timeout() {
        "타임아웃"
    } else if e.is_connect() {
        "연결 실패"
    } else if e.is_redirect() {
        "리다이렉트 오류"
    } else if e.is_body() {
        "본문 오류"
    } else if e.is_decode() {
        "디코드 오류"
    } else if e.is_request() {
        "요청 오류"
    } else {
        "전송 오류"
    }
}

/// reqwest 에러의 **진짜 원인을 드러낸** 한 줄을 만든다. Display(wrapper)만으로는 알 수 없는
/// 분류 + source() 체인을 합친다 — "error sending request for url (...)" → `[연결 실패] … →
/// 원인: connection reset by peer (os error 104)` 처럼 보이게 한다.
pub fn describe_reqwest_error(e: &reqwest::Error) -> String {
    format_transport_detail(&e.to_string(), reqwest_kind(e), &error_source_chain(e))
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

/// 전송(reqwest) 오류 메시지를 한 번에 만든다 — (A) 분류 + source() 원인 체인
/// ([`describe_reqwest_error`]) + (B) 실패 지점 앵커([`here!`]) + 런타임 백트레이스
/// ([`backtrace_string`]). 결과는 "자세히 보기" trace로 노출된다. `here!`가 호출부 함수를
/// 잡아야 하므로 함수가 아니라 매크로다.
///
/// 예: `transport_error_message!("HTTP 전송 오류가 발생했습니다", e)`
#[macro_export]
macro_rules! transport_error_message {
    ($prefix:expr, $e:expr) => {
        ::std::format!(
            "{}: {}\n\nat {}\n\n{}",
            $prefix,
            $crate::util::describe_reqwest_error(&$e),
            $crate::here!(),
            $crate::util::backtrace_string(),
        )
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn now_ms_is_positive_and_post_2020() {
        // 2020-01-01 = 1_577_836_800_000 ms
        assert!(now_ms() > 1_577_836_800_000);
    }

    #[derive(Debug)]
    struct Wrap {
        msg: String,
        src: Option<Box<dyn std::error::Error>>,
    }
    impl std::fmt::Display for Wrap {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "{}", self.msg)
        }
    }
    impl std::error::Error for Wrap {
        fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
            self.src.as_deref()
        }
    }

    #[test]
    fn error_source_chain_unwraps_to_root_cause() {
        // wrapper가 진짜 원인을 source()에 숨겨도 끝까지 펼친다.
        let inner = Wrap {
            msg: "connection reset by peer (os error 104)".into(),
            src: None,
        };
        let outer = Wrap {
            msg: "error sending request for url (https://x)".into(),
            src: Some(Box::new(inner)),
        };
        let chain = error_source_chain(&outer);
        assert_eq!(chain, vec!["connection reset by peer (os error 104)"]);
    }

    #[test]
    fn error_source_chain_dedupes_adjacent_duplicates() {
        // hyper/reqwest가 같은 사유를 중첩해 실어도 인접 중복은 한 번만.
        let a = Wrap {
            msg: "timed out".into(),
            src: None,
        };
        let b = Wrap {
            msg: "timed out".into(),
            src: Some(Box::new(a)),
        };
        let c = Wrap {
            msg: "send failed".into(),
            src: Some(Box::new(b)),
        };
        assert_eq!(error_source_chain(&c), vec!["timed out"]);
    }

    #[test]
    fn format_transport_detail_with_and_without_chain() {
        // 체인이 있으면 분류 + wrapper + 원인 체인을, 없으면 분류 + wrapper만.
        let with = format_transport_detail(
            "error sending request",
            "연결 실패",
            &["connection refused".to_owned(), "os error 111".to_owned()],
        );
        assert_eq!(
            with,
            "[연결 실패] error sending request → 원인: connection refused → os error 111"
        );
        let without = format_transport_detail("error sending request", "타임아웃", &[]);
        assert_eq!(without, "[타임아웃] error sending request");
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
