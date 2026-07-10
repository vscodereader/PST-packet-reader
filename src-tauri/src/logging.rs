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

use std::collections::VecDeque;
use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::fmt::time::FormatTime;
use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

/// non-blocking writer의 [`WorkerGuard`]를 프로세스 수명 동안 살려둔다.
/// 이 가드가 drop되면 백그라운드 로깅 스레드가 멈추므로, 전역에 보관한다.
static LOG_GUARD: Mutex<Option<WorkerGuard>> = Mutex::new(None);

/// 로그 타임스탬프를 로컬 시각 `YYYY-MM-DD HH:MM:SS`로 출력한다(기본 UTC 마이크로초
/// 대신 사람이 읽기 쉬운 형식). 표현만 바꾸며, 로깅 동작에는 영향이 없다.
struct LocalTimer;

impl FormatTime for LocalTimer {
    fn format_time(&self, w: &mut fmt::format::Writer<'_>) -> std::fmt::Result {
        write!(w, "{}", chrono::Local::now().format("%Y-%m-%d %H:%M:%S"))
    }
}

/// 환경변수 `PSTMACRO_LOG`(미설정 시 `info`)로 필터를 구성한다.
fn env_filter() -> EnvFilter {
    EnvFilter::try_from_env("PSTMACRO_LOG").unwrap_or_else(|_| EnvFilter::new("info"))
}

/// [원격제어 #324] 앱 tracing 로그를 메모리 링버퍼에 담아, 하위 에이전트가 서버(Admin 로그 창)로
/// 흘려보낼 수 있게 한다. 파일·콘솔과 같은 이벤트가 여기에도 한 줄씩(같은 포맷) 쌓이고, 상한
/// (`LOG_RING_CAP`) 초과 시 오래된 줄부터 버린다. 자격증명(쿠키 등)은 로그에 안 들어가는 기존
/// 컨벤션이 그대로라 이 버퍼도 안전하다.
static LOG_RING: Mutex<VecDeque<String>> = Mutex::new(VecDeque::new());
const LOG_RING_CAP: usize = 3000;

/// 링버퍼에 로그 줄을 쌓는 tracing writer. fmt 레이어가 포맷한 한 줄(들)을 그대로 받는다.
struct RingWriter;

impl io::Write for RingWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if let Ok(text) = std::str::from_utf8(buf) {
            for line in text.split('\n') {
                let line = line.trim_end();
                if !line.is_empty() {
                    if let Ok(mut ring) = LOG_RING.lock() {
                        ring.push_back(line.to_owned());
                        while ring.len() > LOG_RING_CAP {
                            ring.pop_front();
                        }
                    }
                }
            }
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl<'a> fmt::MakeWriter<'a> for RingWriter {
    type Writer = RingWriter;
    fn make_writer(&'a self) -> Self::Writer {
        RingWriter
    }
}

/// 하위 에이전트가 서버로 보낼, 아직 안 보낸 로그 줄을 최대 `max`개 FIFO로 꺼낸다(꺼낸 건 제거).
/// best-effort — 전송 실패 시 그 줄은 유실될 수 있으나 로그 스트림엔 허용된다(#324).
pub fn drain_agent_logs(max: usize) -> Vec<String> {
    let mut out = Vec::new();
    if let Ok(mut ring) = LOG_RING.lock() {
        while out.len() < max {
            match ring.pop_front() {
                Some(line) => out.push(line),
                None => break,
            }
        }
    }
    out
}

/// 외부에서 로그 파일을 지우거나 비워도 다음 기록 시 같은 날짜 파일을 자동
/// 재생성하는 self-healing 일자별 writer.
///
/// `tracing_appender::rolling::daily` 와 동일하게 `<prefix>.<YYYY-MM-DD>` 파일에
/// append 하되, 그 appender가 파일 핸들을 프로세스 수명 동안 잡고 외부 삭제를
/// 감지하지 못하는 한계를 보완한다(매 기록 시 당일 파일 존재를 확인해, 없으면
/// 재생성). 날짜가 바뀌면 새 날짜 파일로 롤오버한다. 날짜는 `LocalTimer` 와 동일
/// 하게 로컬 시각 기준이라, "오늘 날짜" 파일이 직관적으로 생긴다.
struct SelfHealingDailyWriter {
    dir: PathBuf,
    prefix: String,
    /// 현재 열려 있는 `(날짜, 파일)`. 첫 기록 전엔 `None`.
    current: Option<(String, File)>,
}

impl SelfHealingDailyWriter {
    fn new(dir: impl Into<PathBuf>, prefix: impl Into<String>) -> Self {
        Self {
            dir: dir.into(),
            prefix: prefix.into(),
            current: None,
        }
    }

    /// 오늘 날짜 문자열(`YYYY-MM-DD`, 로컬). `LocalTimer` 와 동일 기준.
    fn today() -> String {
        chrono::Local::now().format("%Y-%m-%d").to_string()
    }

    /// 당일 로그 파일이 열려 있도록 보장하고(없으면/날짜가 바뀌면/외부에서 삭제됐으면
    /// 재오픈) 그 파일의 가변 참조를 돌려준다. 디렉터리째 지워졌을 수 있어 항상 보장한다.
    fn ensure_open(&mut self) -> io::Result<&mut File> {
        let date = Self::today();
        let path = self.dir.join(format!("{}.{}", self.prefix, date));
        let have = self.current.as_ref().map(|(d, _)| d.as_str());
        if should_reopen(have, &date, path.exists()) {
            std::fs::create_dir_all(&self.dir)?;
            let file = OpenOptions::new().create(true).append(true).open(&path)?;
            self.current = Some((date, file));
        }
        Ok(&mut self.current.as_mut().expect("current 는 위에서 보장됨").1)
    }
}

/// 로그 파일을 다시 열어야 하는지 결정한다(순수 함수, 테스트 가능).
///
/// - 아직 연 적 없음(`None`) → `true` (첫 기록)
/// - 같은 날짜인데 파일이 디스크에 없음 → `true` (외부 삭제 후 self-heal)
/// - 날짜가 바뀜 → `true` (일자 롤오버, 파일 존재 여부 무관)
/// - 같은 날짜이고 파일이 존재 → `false` (그대로 append)
fn should_reopen(current_date: Option<&str>, today: &str, file_exists: bool) -> bool {
    match current_date {
        Some(d) if d == today => !file_exists,
        _ => true,
    }
}

impl Write for SelfHealingDailyWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.ensure_open()?.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        match &mut self.current {
            Some((_, file)) => file.flush(),
            None => Ok(()),
        }
    }
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

    // self-healing writer: 외부에서 로그 파일을 지우거나 비워도 다음 기록 시
    // 같은 날짜 파일을 자동 재생성한다(앱 재시작 불필요). 일자별 파일명·append
    // 동작은 기존 rolling::daily 와 동일하다.
    let appender = SelfHealingDailyWriter::new(logs_dir, "pstmacro.log");
    let (non_blocking, guard) = tracing_appender::non_blocking(appender);
    *LOG_GUARD.lock().expect("LOG_GUARD poisoned") = Some(guard);

    let file_layer = fmt::layer()
        .with_ansi(false)
        .with_target(true)
        .with_timer(LocalTimer)
        .with_writer(non_blocking);

    // 콘솔(stderr) 레이어 — `pnpm tauri dev`에선 터미널에 그대로 보이고, 콘솔이 없는
    // 릴리즈 exe(`windows_subsystem = "windows"`)에선 detached stderr라 무해하다.
    // 같은 이벤트가 파일·콘솔 양쪽에 남으므로, exe로 돌려도 `[ADB]`/`[LOGIN]`/`[CHROME]`
    // 상태 로그를 파일(`logs/pstmacro.log`)에서 확인할 수 있다.
    let console_layer = fmt::layer()
        .with_ansi(false)
        .with_target(false)
        .with_timer(LocalTimer)
        .with_writer(std::io::stderr);

    // [원격제어 #324] 링버퍼 레이어 — 파일·콘솔과 같은 이벤트를 메모리에도 쌓아 에이전트가
    // 서버(Admin 로그 창)로 흘려보낸다. 파일 레이어와 동일 포맷(target·로컬시각).
    let ring_layer = fmt::layer()
        .with_ansi(false)
        .with_target(true)
        .with_timer(LocalTimer)
        .with_writer(RingWriter);

    // try_init: 이미 설치돼 있으면 Err를 반환하므로 무시(중복 초기화 안전).
    let _ = tracing_subscriber::registry()
        .with(env_filter())
        .with(file_layer)
        .with(console_layer)
        .with(ring_layer)
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
    fn never_emits_password_values_passed_around_it() {
        // 컨벤션: 비밀번호는 어떤 이벤트 인자로도 넘기지 않는다. 로그 호출부는
        // 마스킹된 식별자(#순번 cho****)와 상태만 기록하므로 PW 평문이 남지 않는다.
        let pw = "S3cr3tPassw0rd!";
        let _password = pw; // 보유는 하지만 로깅엔 넘기지 않는다
        let out = captured(|| {
            tracing::info!("[LOGIN] #3 cho****  로그인 성공 ✅");
        });
        assert!(!out.contains(pw), "비밀번호가 로그에 노출됨: {out}");
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

    #[test]
    fn should_reopen_first_write_and_stable_same_day() {
        // 첫 기록(아직 연 적 없음) → 재오픈.
        assert!(should_reopen(None, "2026-06-09", false));
        assert!(should_reopen(None, "2026-06-09", true));
        // 같은 날짜 + 파일 존재 → 그대로 append(재오픈 안 함).
        assert!(!should_reopen(Some("2026-06-09"), "2026-06-09", true));
    }

    #[test]
    fn should_reopen_on_deletion_or_day_change() {
        // 같은 날짜인데 파일이 사라짐(외부 삭제) → self-heal 재오픈.
        assert!(should_reopen(Some("2026-06-09"), "2026-06-09", false));
        // 날짜가 바뀜 → 롤오버 재오픈(파일 존재 여부 무관).
        assert!(should_reopen(Some("2026-06-08"), "2026-06-09", true));
        assert!(should_reopen(Some("2026-06-08"), "2026-06-09", false));
    }

    #[test]
    fn writer_recreates_log_file_after_external_deletion() {
        // 핵심 동작: 로그 파일을 외부에서 지워도 다음 기록 시 같은 날짜 파일이
        // 자동 재생성되고 이후 로그가 정상 기록된다(앱 재시작 불필요).
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("logs");
        let mut writer = SelfHealingDailyWriter::new(&dir, "pstmacro.log");
        let date = SelfHealingDailyWriter::today();
        let path = dir.join(format!("pstmacro.log.{date}"));

        // 1) 첫 기록 → 당일 파일 생성.
        writer.write_all(b"first\n").unwrap();
        writer.flush().unwrap();
        assert!(path.exists(), "첫 기록 후 로그 파일이 생성돼야 한다");

        // 2) 외부에서 파일 삭제.
        std::fs::remove_file(&path).unwrap();
        assert!(!path.exists());

        // 3) 다시 기록 → 같은 날짜 파일이 자동 재생성되고 새 내용이 기록된다.
        writer.write_all(b"after-delete\n").unwrap();
        writer.flush().unwrap();
        assert!(path.exists(), "삭제 후 재기록 시 파일이 재생성돼야 한다");
        let body = std::fs::read_to_string(&path).unwrap();
        assert!(
            body.contains("after-delete"),
            "재생성된 파일에 새 로그가 있어야 한다: {body}"
        );
    }

    #[test]
    fn writer_appends_within_same_day_without_reopening() {
        // 같은 날 연속 기록은 한 파일에 누적된다(불필요한 재오픈으로 내용이 날아가지 않음).
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("logs");
        let mut writer = SelfHealingDailyWriter::new(&dir, "pstmacro.log");
        let date = SelfHealingDailyWriter::today();
        let path = dir.join(format!("pstmacro.log.{date}"));

        writer.write_all(b"line-1\n").unwrap();
        writer.write_all(b"line-2\n").unwrap();
        writer.flush().unwrap();

        let body = std::fs::read_to_string(&path).unwrap();
        assert!(body.contains("line-1") && body.contains("line-2"), "{body}");
    }
}
