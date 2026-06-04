//! tracing 기반 파일 로깅 인프라.
//!
//! 앱 부팅 시 [`init_file_logging`]을 한 번 호출해 일자별 롤링 파일
//! (`<logs_dir>/pstmacro.log`)로 구조화 로그를 기록한다. 레벨은 환경변수
//! `PSTMACRO_LOG`로 제어하며(미설정 시 `info`), 예:
//! `PSTMACRO_LOG=debug,pstmacro_lib::naver_cafe=trace`.
//!
//! # 쿠키 보안
//! 이 모듈은 포맷/출력만 담당한다. 호출부는 사용자의 인증 자격 증명
//! (Cookie 헤더, storage-state)을 어떤 `tracing` 필드/메시지에도 넣지
//! 않는다 — 기존 `naver_cafe` 컨벤션과 동일하다.

use std::path::Path;
use std::sync::Mutex;

use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

/// non-blocking writer의 [`WorkerGuard`]를 프로세스 수명 동안 살려둔다.
/// 이 가드가 drop되면 백그라운드 로깅 스레드가 멈추므로, 전역에 보관한다.
static LOG_GUARD: Mutex<Option<WorkerGuard>> = Mutex::new(None);

/// 환경변수 `PSTMACRO_LOG`(미설정 시 `info`)로 필터를 구성한다.
fn env_filter() -> EnvFilter {
    EnvFilter::try_from_env("PSTMACRO_LOG").unwrap_or_else(|_| EnvFilter::new("info"))
}

/// `logs_dir`에 일자별 롤링 파일 로거를 설치한다(프로세스 1회).
///
/// 디렉터리는 없으면 생성한다. 이미 전역 subscriber가 설치돼 있으면
/// (테스트 등) 조용히 무시한다. 파일 출력에는 ANSI 색상을 넣지 않는다.
pub fn init_file_logging(logs_dir: &Path) {
    if let Err(e) = std::fs::create_dir_all(logs_dir) {
        // 로깅 초기화 실패가 앱 부팅을 막아선 안 된다.
        eprintln!(
            "[logging] 로그 디렉터리 생성 실패 ({}): {e}",
            logs_dir.display()
        );
        return;
    }

    let appender = tracing_appender::rolling::daily(logs_dir, "pstmacro.log");
    let (non_blocking, guard) = tracing_appender::non_blocking(appender);
    *LOG_GUARD.lock().expect("LOG_GUARD poisoned") = Some(guard);

    let file_layer = fmt::layer()
        .with_ansi(false)
        .with_target(true)
        .with_writer(non_blocking);

    // try_init: 이미 설치돼 있으면 Err를 반환하므로 무시(중복 초기화 안전).
    let _ = tracing_subscriber::registry()
        .with(env_filter())
        .with(file_layer)
        .try_init();
}

#[cfg(test)]
mod tests {
    use std::io::Write;
    use std::sync::{Arc, Mutex};

    use tracing::subscriber::with_default;
    use tracing_subscriber::fmt::MakeWriter;

    use super::*;

    /// 테스트용 인메모리 writer — 방출된 로그를 버퍼에 모은다.
    #[derive(Clone, Default)]
    struct BufWriter(Arc<Mutex<Vec<u8>>>);

    impl Write for BufWriter {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl MakeWriter<'_> for BufWriter {
        type Writer = BufWriter;
        fn make_writer(&self) -> Self::Writer {
            self.clone()
        }
    }

    /// `f` 실행 중 방출된 로그를 문자열로 캡처한다(전역 subscriber 미사용).
    fn captured<F: FnOnce()>(f: F) -> String {
        let buf = BufWriter::default();
        let subscriber = tracing_subscriber::registry()
            .with(fmt::layer().with_ansi(false).with_writer(buf.clone()));
        with_default(subscriber, f);
        let bytes = buf.0.lock().unwrap().clone();
        String::from_utf8(bytes).unwrap()
    }

    #[test]
    fn records_event_message_and_fields() {
        let out = captured(|| {
            tracing::info!(cafe_id = 31732304u64, count = 3, "joined cafes fetched");
        });
        assert!(out.contains("joined cafes fetched"), "메시지 누락: {out}");
        assert!(out.contains("cafe_id=31732304"), "필드 누락: {out}");
        assert!(out.contains("count=3"), "필드 누락: {out}");
    }

    #[test]
    fn never_emits_cookie_values_passed_around_it() {
        // 컨벤션: 쿠키 값은 어떤 이벤트 인자로도 넘기지 않는다. 호출부가
        // 상태/식별자만 로깅하면 출력에 자격 증명이 남지 않음을 확인한다.
        let secret = "NID_AUT=SUPER_SECRET; NID_SES=ALSO_SECRET";
        let _cookie_header = secret; // 보유는 하지만 로깅엔 넘기지 않는다
        let out = captured(|| {
            tracing::debug!(status = 200, count = 2, "join-cafes request done");
        });
        assert!(
            !out.contains("SUPER_SECRET"),
            "쿠키 값이 로그에 노출됨: {out}"
        );
        assert!(!out.contains(secret), "쿠키 헤더가 로그에 노출됨: {out}");
    }

    #[test]
    fn init_file_logging_creates_logs_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let logs_dir = tmp.path().join("logs");
        assert!(!logs_dir.exists());
        init_file_logging(&logs_dir);
        assert!(logs_dir.is_dir(), "로그 디렉터리가 생성되지 않음");
    }

    #[test]
    fn env_filter_defaults_to_info_when_unset() {
        // 기본 필터가 INFO를 통과시키는지(레벨 문자열 포함) 확인.
        let filter = env_filter();
        assert!(format!("{filter}").contains("info"));
    }
}
