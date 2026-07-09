//! 조회수 부스트: 게시글 링크를 **시크릿(incognito) 창**으로 여닫으며 조회수를 올린다.
//!
//! 한 링크당 다음을 `repeats`번 **순차** 반복한다(사용자 명세):
//!   1. 보이는 시크릿창 실행(고유 임시 프로필 · `--incognito` — 로그인/게시와 동일한
//!      [`launch_debug_chrome`] 재사용, 매번 완전히 새 세션).
//!   2. 링크로 이동한 뒤 **`document.readyState === "complete"`(완전 로딩)** 까지 기다린다.
//!      ([`CdpClient::navigate`]는 `interactive`에서도 반환하므로, 여기서 한 번 더 조인다.)
//!   3. 페이지를 새로고침(`Page.reload`)하고 다시 완전 로딩을 기다린다.
//!   4. 그 시크릿창을 닫는다 — [`ChromeHandle`]의 `Drop`이 **그 프로세스 트리(자식 헬퍼 포함)만**
//!      `taskkill /PID <pid> /T /F`로 종료하고 `wait()`로 회수한 뒤 임시 프로필을 지운다.
//!      전체 Chrome을 닫지 않고, 다음 창을 열기 전에 완전 종료를 보장한다(고아 프로세스 방지).
//!
//! 기존 로그인/게시 인프라(`launch_debug_chrome` · `CdpClient`)만 재사용하고, 좋아요·카페·
//! 블로그·밴드 등 다른 기능은 건드리지 않는다.

use std::thread::sleep;
use std::time::{Duration, Instant};

use serde::Serialize;
use serde_json::json;

use crate::auth::launch_debug_chrome;
use crate::naver_automation::CdpClient;

/// CDP 연결 시 붙는 로컬 DevTools 호스트(포트는 `launch_debug_chrome`가 확정).
const DEVTOOLS_HOST: &str = "127.0.0.1";
/// `readyState === "complete"`(완전 로딩)를 기다리는 상한. 초과하면 이번 회차를 실패 처리한다.
const LOAD_COMPLETE_TIMEOUT: Duration = Duration::from_secs(30);
/// 완전 로딩 폴링 간격.
const LOAD_POLL_INTERVAL: Duration = Duration::from_millis(200);

/// 한 링크의 조회수 부스트 결과(프론트 표시용 — TS `ViewBoostOutcome` 미러).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ViewBoostOutcome {
    /// 조회수를 올린 게시글 링크.
    pub link: String,
    /// 요청한 반복 횟수(N).
    pub requested: u32,
    /// 실제로 "열기→완전로딩→새로고침→완전로딩→종료"까지 끝낸 횟수.
    pub completed: u32,
    /// `completed == requested`(요청 전량 성공)인지.
    pub success: bool,
    /// 표시용 메시지(성공 문구 또는 실패 사유).
    pub message: String,
}

/// 여러 링크를 순서대로 각각 `repeats`번 부스트한다. 링크 하나가 도중에 실패해도
/// 다음 링크로 계속 진행한다(각 결과는 개별 [`ViewBoostOutcome`]에 담긴다).
pub fn boost_views(links: &[String], repeats: u32) -> Vec<ViewBoostOutcome> {
    links
        .iter()
        .map(|link| boost_one(link, repeats))
        .collect()
}

/// 링크 하나를 `repeats`번 부스트한다. 한 회차라도 실패하면 그 링크는 거기서 멈추고
/// (같은 오류가 반복될 가능성이 높다) 지금까지의 진행분을 결과로 남긴다.
fn boost_one(link: &str, repeats: u32) -> ViewBoostOutcome {
    let mut completed = 0u32;
    let mut last_err: Option<String> = None;

    for round in 0..repeats {
        match single_cycle(link) {
            Ok(()) => {
                completed += 1;
                tracing::info!(
                    link = %link,
                    "[VIEW] 조회수 부스트 {}/{}회 완료",
                    completed,
                    repeats
                );
            }
            Err(error) => {
                tracing::warn!(
                    link = %link,
                    %error,
                    "[VIEW] 조회수 부스트 {}회차 실패 — 이 링크 중단",
                    round + 1
                );
                last_err = Some(format!("{}회차에서 {}", round + 1, error));
                break;
            }
        }
    }

    finalize_outcome(link, repeats, completed, last_err)
}

/// 진행 결과를 사람이 읽는 [`ViewBoostOutcome`]으로 정리한다(순수 함수 — 실제 브라우저 없이
/// 테스트 가능). 요청 전량을 채웠으면 성공, 하나도 못 했으면 실패 사유, 일부만 했으면
/// "몇/몇 회 후 중단"으로 구분한다.
fn finalize_outcome(
    link: &str,
    requested: u32,
    completed: u32,
    last_err: Option<String>,
) -> ViewBoostOutcome {
    let success = requested > 0 && completed == requested;
    let message = if success {
        format!("{requested}회 조회수 부스트 완료")
    } else if completed == 0 {
        match last_err {
            Some(error) => format!("실패 — {error}"),
            None => "실행되지 않았습니다".to_owned(),
        }
    } else {
        match last_err {
            Some(error) => format!("{completed}/{requested}회 완료 후 중단 — {error}"),
            None => format!("{completed}/{requested}회만 완료"),
        }
    };

    ViewBoostOutcome {
        link: link.to_owned(),
        requested,
        completed,
        success,
        message,
    }
}

/// 한 번의 "열기→완전로딩→새로고침→완전로딩→종료" 사이클. 성공하면 `Ok(())`.
///
/// 핵심: `handle`(시크릿창)은 이 함수가 끝나며 **명시적으로 `drop`** 되어, 그 프로세스 트리만
/// 강제 종료·회수된다. `drop`은 `wait()`로 블로킹하므로, 이 함수가 반환한 시점엔 창이 완전히
/// 닫혀 있고 그제서야 다음 회차가 새 창을 연다(사용자 명세: "완전히 닫힌 걸 확인했으면 다시").
fn single_cycle(link: &str) -> Result<(), String> {
    let handle =
        launch_debug_chrome(false).map_err(|error| format!("시크릿창 실행 실패: {error}"))?;

    // 창 조작은 클로저로 묶어, 성공/실패와 무관하게 아래에서 handle을 반드시 drop(종료)한다.
    let result = (|| -> Result<(), String> {
        let mut client = CdpClient::connect_to_existing_chrome(DEVTOOLS_HOST, handle.port)
            .map_err(|error| format!("CDP 연결 실패: {error}"))?;
        client
            .enable_page_only()
            .map_err(|error| format!("Page 도메인 활성화 실패: {error}"))?;

        // 링크로 이동 + 완전 로딩 대기(navigate는 interactive에서도 반환하므로 한 번 더 조인다).
        client
            .navigate(link)
            .map_err(|error| format!("페이지 이동 실패: {error}"))?;
        wait_for_load_complete(&mut client)?;

        // 새로고침 한 번 + 다시 완전 로딩 대기.
        client
            .call("Page.reload", json!({ "ignoreCache": false }))
            .map_err(|error| format!("새로고침 실패: {error}"))?;
        wait_for_load_complete(&mut client)?;

        Ok(())
    })();

    // 시크릿창 완전 종료 — ChromeHandle Drop이 그 PID 트리만 taskkill /T /F + wait 회수 + 프로필 삭제.
    drop(handle);
    result
}

/// `document.readyState === "complete"`(페이지 **완전** 로딩)가 될 때까지 기다린다.
/// `navigate`가 쓰는 `wait_for_ready_state`는 `interactive`에서도 통과하지만, 여기서는
/// 사용자 명세대로 `complete`만 인정한다.
fn wait_for_load_complete(client: &mut CdpClient) -> Result<(), String> {
    let deadline = Instant::now() + LOAD_COMPLETE_TIMEOUT;
    loop {
        let state = client
            .evaluate_string("document.readyState")
            .map_err(|error| format!("readyState 확인 실패: {error}"))?;
        if state == "complete" {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(format!(
                "페이지 완전 로딩(readyState=complete) 대기 시간 초과 (마지막 상태: {state})"
            ));
        }
        sleep(LOAD_POLL_INTERVAL);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_success_marks_success_with_count() {
        let out = finalize_outcome("https://x/discussion/1", 30, 30, None);
        assert!(out.success);
        assert_eq!(out.completed, 30);
        assert_eq!(out.requested, 30);
        assert!(out.message.contains("30회"));
        assert!(out.message.contains("완료"));
    }

    #[test]
    fn zero_completed_with_error_reports_failure_reason() {
        let out = finalize_outcome(
            "https://x/discussion/1",
            10,
            0,
            Some("1회차에서 CDP 연결 실패: ...".to_owned()),
        );
        assert!(!out.success);
        assert_eq!(out.completed, 0);
        assert!(out.message.starts_with("실패 — "));
        assert!(out.message.contains("CDP 연결 실패"));
    }

    #[test]
    fn partial_progress_reports_stopped_after_n() {
        let out = finalize_outcome(
            "https://x/discussion/1",
            30,
            12,
            Some("13회차에서 페이지 이동 실패: timeout".to_owned()),
        );
        assert!(!out.success);
        assert_eq!(out.completed, 12);
        assert!(out.message.contains("12/30회 완료 후 중단"));
        assert!(out.message.contains("13회차"));
    }

    #[test]
    fn zero_repeats_is_not_success() {
        // 커맨드가 repeats>=1을 강제하지만, 방어적으로 0이 와도 success=false여야 한다.
        let out = finalize_outcome("https://x/discussion/1", 0, 0, None);
        assert!(!out.success);
        assert_eq!(out.message, "실행되지 않았습니다");
    }

    #[test]
    fn boost_views_returns_one_outcome_per_link_preserving_order() {
        // repeats=0이면 single_cycle을 한 번도 부르지 않으므로(브라우저 미실행) 순수하게
        // 링크 개수·순서·requested 매핑만 검증할 수 있다.
        let links = vec![
            "https://x/discussion/1".to_owned(),
            "https://x/discussion/2".to_owned(),
        ];
        let outs = boost_views(&links, 0);
        assert_eq!(outs.len(), 2);
        assert_eq!(outs[0].link, "https://x/discussion/1");
        assert_eq!(outs[1].link, "https://x/discussion/2");
        assert!(outs.iter().all(|o| o.requested == 0 && o.completed == 0));
    }
}
