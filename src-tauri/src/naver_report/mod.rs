//! 네이버 종목토론방 글 **신고하기**(설계서 `docs/naver-report-design.md`).
//!
//! 하이브리드(설계서 §4): 조회(by-item/profile)·사유·신고 POST는 순수 Rust HTTP,
//! `ncaptchaTokenId`만 보이는 크롬(CDP)으로 생성한다. 계정 바깥루프 / 링크 안쪽루프이며 계정마다
//! 크롬을 한 번 열어(설계서 §5) 그 계정의 링크 n개 토큰을 이어서 만든다. IP 회전 ON이면 계정 사이에
//! ADB로 IP를 돌리고 새 IP에서 재로그인한다(설계서 §6, 기존 `auth::process_account` 재사용).
//!
//! 이 모듈의 공개 진입점은 [`run_report_batch`]이며, `lib.rs`의 `report_posts` 커맨드가 이를
//! 백그라운드 태스크에서 돌려 화면을 막지 않는다(설계서 §5·§8.2).

mod content_resolver;
mod error;
mod report_client;
mod token;

use std::thread::sleep;
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Runtime};

pub use error::ReportError;
pub use report_client::{ReportReason, REPORT_REASONS};

use content_resolver::{build_content_id, parse_discussion_link, resolve_target};
use report_client::{is_valid_reason_code, reason_label, ReportHttp};
use token::ReportBrowser;

/// 계정×링크 한 건의 신고 결과(프론트 표시용 — TS `ReportOutcome` 미러). accountId는 loginId(쿠키 키).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReportOutcome {
    /// 신고한 계정(loginId, 쿠키 파일 키).
    pub account_id: String,
    /// 신고 대상 글 링크.
    pub link: String,
    /// 신고 성공(`{"success":true}`) 여부.
    pub success: bool,
    /// 표시용 메시지(성공 문구 또는 실패 사유 원문).
    pub message: String,
}

/// 신고 사이의 사람같은 간격(설계서 §5·§7 — 위험점수 완화). 첫 건 제외 매 건 앞에 둔다.
const REPORT_GAP: Duration = Duration::from_millis(1500);

/// 프론트로 보내는 완료 이벤트 이름(설계서 §8.2: 결과는 이벤트/알림).
pub const REPORT_FINISHED_EVENT: &str = "report-finished";

/// 완료 이벤트 페이로드 — 총/성공 건수와 계정×링크별 상세.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReportFinished {
    pub total: usize,
    pub succeeded: usize,
    pub outcomes: Vec<ReportOutcome>,
}

/// 결과 목록을 총/성공 건수로 요약한다(순수 함수 — 알림 문구·이벤트에 쓴다).
pub fn summarize(outcomes: &[ReportOutcome]) -> (usize, usize) {
    let succeeded = outcomes.iter().filter(|o| o.success).count();
    (outcomes.len(), succeeded)
}

/// n×m 신고 배치를 실행한다(설계서 §5). 계정 바깥루프/링크 안쪽루프, 계정마다 크롬 1회. 한 건이
/// 실패해도 다음으로 계속 진행하며, 계정×링크별 [`ReportOutcome`]을 모아 돌려준다. `rotate_ip`가
/// true면 계정 사이에 ADB로 IP를 돌리고 새 IP에서 재로그인한다(`auth::process_account`).
///
/// 브라우저·HTTP 왕복이 있는 블로킹 작업이라 호출부(커맨드)는 이 함수를 백그라운드에서 돌려야 한다.
pub async fn run_report_batch<R: Runtime>(
    app: AppHandle<R>,
    links: Vec<String>,
    account_ids: Vec<String>,
    reason_code: String,
    rotate_ip: bool,
) -> Vec<ReportOutcome> {
    let mut outcomes: Vec<ReportOutcome> = Vec::with_capacity(account_ids.len() * links.len());

    for account_id in &account_ids {
        // IP 회전 ON: 앞 계정 크롬은 이미 drop으로 종료됨 → ADB 회전 후 새 IP에서 재로그인(설계서 §6).
        // process_account(use_adb=true, force=true)가 회전+재로그인을 한 번에 처리한다. ADB가 없으면
        // 내부에서 회전을 건너뛰고 현재 IP로 로그인한다(사수 지시). 실패해도 그 계정은 저장 쿠키로
        // 계속 시도한다(패닉 금지).
        if rotate_ip {
            if let Err(error) =
                crate::auth::process_account(&app, account_id, false, true, true).await
            {
                tracing::warn!(
                    account = %crate::auth::mask_id(account_id),
                    %error,
                    "[REPORT] IP 회전/재로그인 실패 — 저장 쿠키로 진행"
                );
            }
        }

        // 이 계정의 링크 전량을 블로킹 스레드에서 처리한다(HTTP + 보이는 크롬 토큰 생성).
        let account_id_owned = account_id.clone();
        let links_owned = links.clone();
        let reason_owned = reason_code.clone();
        let account_outcomes = tauri::async_runtime::spawn_blocking(move || {
            report_one_account(&account_id_owned, &links_owned, &reason_owned)
        })
        .await
        .unwrap_or_else(|join_error| {
            // 스레드 자체가 죽은 경우(패닉/취소): 이 계정의 모든 링크를 실패로 보고한다.
            links
                .iter()
                .map(|link| ReportOutcome {
                    account_id: account_id.clone(),
                    link: link.clone(),
                    success: false,
                    message: format!("신고 작업 스레드 오류: {join_error}"),
                })
                .collect()
        });
        outcomes.extend(account_outcomes);
    }

    let (total, succeeded) = summarize(&outcomes);
    tracing::info!(total, succeeded, "[REPORT] 신고 배치 완료");
    emit_finished(&app, &outcomes);
    outcomes
}

/// 계정 하나의 링크 전량을 처리한다(블로킹). 이 계정용 크롬을 한 번 열고(토큰 생성) 링크마다
/// 조회→토큰→제출을 수행한 뒤 크롬을 닫는다. 쿠키/크롬 준비 실패는 그 계정의 모든 링크를 같은
/// 사유로 실패 처리한다.
fn report_one_account(account_id: &str, links: &[String], reason_code: &str) -> Vec<ReportOutcome> {
    // 저장 로그인 쿠키(storage-state)를 한 번 로드해 HTTP 클라이언트와 토큰 브라우저 주입용 쿠키를
    // 함께 만든다. 없으면 이 계정 전부 실패(재로그인 필요).
    let (http, cookies) = match load_session(account_id) {
        Ok(session) => session,
        Err(error) => return fail_all(account_id, links, &error.to_string()),
    };

    // 계정당 크롬 1회(설계서 §4.2). navigate 전에 이 계정 세션 쿠키를 주입해 로그인 상태로 신고
    // 페이지를 연다(비로그인이면 nid 로그인으로 튕겨 SDK 가 안 뜸). 열기 실패면 이 계정 전부 실패.
    let mut browser = match ReportBrowser::open(&cookies) {
        Ok(browser) => browser,
        Err(error) => return fail_all(account_id, links, &error.to_string()),
    };
    // 사유 라벨은 코드 1개당 고정이라 계정 루프 밖에서 한 번 구한다(사유 UI 텍스트 매칭 폴백용).
    let reason_label = reason_label(reason_code);

    let mut outcomes = Vec::with_capacity(links.len());
    for (index, link) in links.iter().enumerate() {
        if index > 0 {
            sleep(REPORT_GAP); // 사람같은 간격(위험점수 완화).
        }
        let result = report_one_link(&http, &mut browser, link, reason_code, reason_label);
        let (success, message) = match result {
            Ok(()) => (true, "신고 완료".to_owned()),
            Err(error) => (false, error.to_string()),
        };
        // 링크별 outcome을 로그로 남긴다 — 실패는 원문 사유를 트레이스 OFF여도 항상 warn!으로.
        if success {
            tracing::info!(
                target: "report",
                account = %crate::auth::mask_id(account_id),
                link = %link,
                "[REPORT] 링크 신고 성공"
            );
        } else {
            tracing::warn!(
                target: "report",
                account = %crate::auth::mask_id(account_id),
                link = %link,
                reason = %message,
                "[REPORT] 링크 신고 실패 — 원문 사유"
            );
        }
        outcomes.push(ReportOutcome {
            account_id: account_id.to_owned(),
            link: link.clone(),
            success,
            message,
        });
    }
    // browser는 여기서 drop → ChromeHandle Drop이 이 계정 크롬 트리를 완전 종료(설계서 §4.1).
    outcomes
}

/// 링크 한 건의 신고: 파싱 → 대상 해석(encryptedUserId + 표시용 title/nickname) → contentId →
/// 신고 페이지를 브라우저로 열어 사유 선택·제출(페이지 SDK 가 ncaptcha 토큰 생성 후 스스로 POST).
fn report_one_link(
    http: &ReportHttp,
    browser: &mut ReportBrowser,
    link: &str,
    reason_code: &str,
    reason_label: &str,
) -> Result<(), ReportError> {
    let parsed = parse_discussion_link(link)?;
    let target = resolve_target(http, &parsed)?;
    let content_id = build_content_id(&parsed.post_id);
    browser.submit_report(&content_id, &target, reason_code, reason_label)
}

/// 저장 쿠키(storage-state)를 한 번 읽어 신고 HTTP 클라이언트와 토큰 브라우저 주입용 네이버 쿠키
/// (name/value 쌍)를 함께 만든다. 쿠키가 없거나 세션이 없으면 실패. 쿠키 값은 자격증명이라 로그에
/// 남기지 않는다(주입 시점 이름/개수만 남긴다 — token.rs 참고).
fn load_session(account_id: &str) -> Result<(ReportHttp, Vec<(String, String)>), ReportError> {
    let storage = crate::auth::read_account_cookies(account_id)
        .map_err(|error| ReportError::NoCookies(format!("쿠키 조회 실패: {error}")))?
        .ok_or_else(|| {
            ReportError::NoCookies(
                "저장된 로그인 쿠키가 없거나 만료됨 — 재로그인이 필요합니다.".to_owned(),
            )
        })?;
    let http = ReportHttp::from_storage_state(&storage).map_err(ReportError::NoCookies)?;
    let cookies = naver_cookie_pairs(&storage);
    Ok((http, cookies))
}

/// storage-state JSON에서 네이버 도메인 쿠키만 골라 (name, value) 쌍으로 뽑는다(토큰 브라우저 CDP
/// 주입용). `cookie_header_from_storage_state`와 같은 필터(도메인에 "naver" 포함)를 쓴다. 값은
/// 자격증명이라 여기서도 로그에 남기지 않는다(호출부가 이름/개수만 남긴다).
fn naver_cookie_pairs(storage: &serde_json::Value) -> Vec<(String, String)> {
    storage
        .get("cookies")
        .and_then(|cookies| cookies.as_array())
        .map(|cookies| {
            cookies
                .iter()
                .filter_map(|cookie| {
                    let domain = cookie.get("domain")?.as_str()?;
                    if !domain.contains("naver") {
                        return None;
                    }
                    let name = cookie.get("name")?.as_str()?;
                    let value = cookie.get("value")?.as_str()?;
                    Some((name.to_owned(), value.to_owned()))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// 계정 준비 실패 시 그 계정의 모든 링크를 같은 사유로 실패 처리한다.
fn fail_all(account_id: &str, links: &[String], message: &str) -> Vec<ReportOutcome> {
    links
        .iter()
        .map(|link| ReportOutcome {
            account_id: account_id.to_owned(),
            link: link.clone(),
            success: false,
            message: message.to_owned(),
        })
        .collect()
}

/// 완료 결과를 프론트로 이벤트 발행한다(best-effort — 실패해도 배치는 이미 끝났다).
fn emit_finished<R: Runtime>(app: &AppHandle<R>, outcomes: &[ReportOutcome]) {
    use tauri::Emitter;
    let (total, succeeded) = summarize(outcomes);
    let payload = ReportFinished {
        total,
        succeeded,
        outcomes: outcomes.to_vec(),
    };
    if let Err(error) = app.emit(REPORT_FINISHED_EVENT, payload) {
        tracing::warn!(%error, "[REPORT] 완료 이벤트 발행 실패");
    }
}

/// 커맨드가 프론트 입력을 실행 전 검증한다(순수). 링크·계정이 비었거나 사유 코드가 실측 7개가
/// 아니면 사람이 읽는 오류를 돌려준다.
pub fn validate_request(
    links: &[String],
    account_ids: &[String],
    reason_code: &str,
) -> Result<(), String> {
    if links.is_empty() {
        return Err("신고할 게시글 링크를 한 개 이상 입력하세요.".to_owned());
    }
    if account_ids.is_empty() {
        return Err("신고할 계정을 한 개 이상 선택하세요.".to_owned());
    }
    if !is_valid_reason_code(reason_code) {
        return Err(format!("알 수 없는 신고 사유 코드입니다: {reason_code}"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn outcome(success: bool) -> ReportOutcome {
        ReportOutcome {
            account_id: "acct".to_owned(),
            link: "https://x/discussion/1".to_owned(),
            success,
            message: "m".to_owned(),
        }
    }

    #[test]
    fn summarize_counts_total_and_succeeded() {
        let outs = vec![outcome(true), outcome(false), outcome(true)];
        assert_eq!(summarize(&outs), (3, 2));
        assert_eq!(summarize(&[]), (0, 0));
    }

    #[test]
    fn fail_all_marks_every_link_failed_with_same_reason() {
        let links = vec!["l1".to_owned(), "l2".to_owned()];
        let outs = fail_all("acct", &links, "쿠키 없음");
        assert_eq!(outs.len(), 2);
        assert!(outs.iter().all(|o| !o.success && o.message == "쿠키 없음"));
        assert_eq!(outs[0].link, "l1");
        assert_eq!(outs[1].link, "l2");
    }

    #[test]
    fn validate_request_rejects_empty_and_bad_reason() {
        assert!(validate_request(&[], &["a".to_owned()], "AA01").is_err());
        assert!(validate_request(&["l".to_owned()], &[], "AA01").is_err());
        assert!(validate_request(&["l".to_owned()], &["a".to_owned()], "ZZ99").is_err());
        assert!(validate_request(&["l".to_owned()], &["a".to_owned()], "AA01").is_ok());
    }

    #[test]
    fn report_reasons_reexported_are_seven() {
        assert_eq!(REPORT_REASONS.len(), 7);
    }

    #[test]
    fn naver_cookie_pairs_keeps_only_naver_domain_cookies() {
        let storage = serde_json::json!({
            "cookies": [
                { "name": "NID_AUT", "value": "aaa", "domain": ".naver.com" },
                { "name": "NID_SES", "value": "bbb", "domain": ".naver.com" },
                { "name": "other", "value": "ccc", "domain": ".example.com" }
            ]
        });
        let pairs = naver_cookie_pairs(&storage);
        assert_eq!(
            pairs,
            vec![
                ("NID_AUT".to_owned(), "aaa".to_owned()),
                ("NID_SES".to_owned(), "bbb".to_owned()),
            ]
        );
    }

    #[test]
    fn naver_cookie_pairs_empty_when_no_cookies_field() {
        assert!(naver_cookie_pairs(&serde_json::json!({})).is_empty());
    }
}
