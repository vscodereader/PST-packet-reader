//! PR #60 리뷰 지적 사항 + 그 외 위험 지점에 대한 통합 회귀 테스트 스위트.
//!
//! 각 테스트는 "그 버그가 다시 생기면 빨갛게 실패"하도록 작성했다. 모듈 이름의 번호는
//! #60에 남긴 리뷰 코멘트에 대응한다. 순수 로직(파일/네트워크/브라우저 비의존)만
//! 모아 호스트 환경과 무관하게 항상 동일하게 돈다.

use crate::auth::accounts::merge_accounts;
use crate::auth::login_flow::{
    classify, credentials_present, decide_loop_step, ChallengeKind, LoopDecision, PageSignals,
    Signal,
};
use crate::auth::Account;
use crate::naver_automation::packet_client::{
    build_cookie_header, cookie_applies_to_host, NaverCookie,
};

fn account(id: &str, pw: &str) -> Account {
    Account {
        id: id.to_owned(),
        password: pw.to_owned(),
        label: id.to_owned(),
    }
}

// === 리뷰 #1: 계정 저장이 파일을 통째로 덮어써 선택 안 한 계정이 사라지던 버그 ===
mod accounts_merge {
    use super::*;

    #[test]
    fn subset_save_preserves_unselected_accounts() {
        // 10개 중 2개만 저장(로그인)해도 나머지 8개가 사라지면 안 된다(데이터 손실 회귀).
        let existing = (0..10)
            .map(|i| account(&format!("id{i}"), "pw"))
            .collect::<Vec<_>>();
        let merged = merge_accounts(existing, &[account("id3", "new"), account("id7", "new")]);

        assert_eq!(merged.len(), 10, "선택하지 않은 계정도 보존되어야 한다");
        assert_eq!(
            merged.iter().find(|a| a.id == "id3").unwrap().password,
            "new"
        );
        assert_eq!(
            merged.iter().find(|a| a.id == "id7").unwrap().password,
            "new"
        );
        assert!(merged.iter().any(|a| a.id == "id0"));
        assert!(merged.iter().any(|a| a.id == "id9"));
    }

    #[test]
    fn updates_existing_and_appends_new() {
        let merged = merge_accounts(
            vec![account("a", "1")],
            &[account("a", "2"), account("b", "9")],
        );
        assert_eq!(merged.len(), 2);
        assert_eq!(merged.iter().find(|x| x.id == "a").unwrap().password, "2");
        assert!(merged.iter().any(|x| x.id == "b"));
    }

    #[test]
    fn empty_incoming_keeps_everything() {
        let merged = merge_accounts(vec![account("a", "1"), account("b", "2")], &[]);
        assert_eq!(merged.len(), 2);
    }

    #[test]
    fn preserves_original_order() {
        let merged = merge_accounts(
            vec![account("a", "1"), account("b", "2"), account("c", "3")],
            &[account("b", "x")],
        );
        let ids = merged.iter().map(|a| a.id.clone()).collect::<Vec<_>>();
        assert_eq!(ids, ["a", "b", "c"]);
    }

    #[test]
    fn duplicate_incoming_id_last_wins() {
        let merged = merge_accounts(vec![], &[account("a", "1"), account("a", "2")]);
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].password, "2");
    }
}

// === 리뷰 #2: 빈/공백 자격증명으로 로그인 버튼이 눌리던 버그 ===
mod credential_guard {
    use super::*;

    #[test]
    fn complete_credentials_pass() {
        assert!(credentials_present("user", "pw"));
    }

    #[test]
    fn empty_or_whitespace_id_is_rejected() {
        assert!(!credentials_present("", "pw"));
        assert!(!credentials_present("   ", "pw"));
        assert!(!credentials_present("\t\n", "pw"));
    }

    #[test]
    fn empty_password_is_rejected() {
        assert!(!credentials_present("user", ""));
    }

    #[test]
    fn password_with_spaces_is_allowed() {
        // 비밀번호는 공백을 포함할 수 있으므로 trim으로 거르지 않는다.
        assert!(credentials_present("user", " p w "));
    }
}

// === 리뷰 #3: 클릭 직후 과도기 신호(#err_common 깜빡임 등)를 영구 실패로 latch하던 버그 ===
mod login_signal_confirmation {
    use super::*;

    #[test]
    fn success_returns_immediately_even_after_negative() {
        assert_eq!(
            decide_loop_step(None, Signal::Success, false),
            LoopDecision::Success
        );
        // 직전에 음성 신호가 누적돼 있었어도 성공이면 성공으로 끝낸다.
        assert_eq!(
            decide_loop_step(Some(Signal::BadCredentials), Signal::Success, false),
            LoopDecision::Success
        );
    }

    #[test]
    fn bad_credentials_needs_two_consecutive_polls() {
        // 첫 히트는 확정하지 않고 대기(다음 폴링에서 재확인).
        assert_eq!(
            decide_loop_step(None, Signal::BadCredentials, false),
            LoopDecision::KeepWaiting(Some(Signal::BadCredentials))
        );
        // 2회 연속일 때만 확정.
        assert_eq!(
            decide_loop_step(Some(Signal::BadCredentials), Signal::BadCredentials, false),
            LoopDecision::ConfirmedBad
        );
    }

    #[test]
    fn transient_bad_then_other_signal_does_not_confirm() {
        // Bad → Pending(리셋) 이후엔 다시 Bad가 떠도 누적이 끊겨 곧장 확정되지 않는다.
        assert_eq!(
            decide_loop_step(Some(Signal::BadCredentials), Signal::Pending, false),
            LoopDecision::KeepWaiting(None)
        );
        assert_eq!(
            decide_loop_step(None, Signal::BadCredentials, false),
            LoopDecision::KeepWaiting(Some(Signal::BadCredentials))
        );
    }

    #[test]
    fn blocked_confirms_only_in_headless_over_two_polls() {
        assert_eq!(
            decide_loop_step(None, Signal::Blocked, false),
            LoopDecision::KeepWaiting(Some(Signal::Blocked))
        );
        assert_eq!(
            decide_loop_step(Some(Signal::Blocked), Signal::Blocked, false),
            LoopDecision::ConfirmedBlocked
        );
    }

    #[test]
    fn blocked_in_headed_keeps_waiting_for_user() {
        // headed에서는 기기등록/인증 중간 페이지를 차단으로 단정하지 않고 기다린다.
        assert_eq!(
            decide_loop_step(None, Signal::Blocked, true),
            LoopDecision::KeepWaiting(None)
        );
        assert_eq!(
            decide_loop_step(Some(Signal::BadCredentials), Signal::Blocked, true),
            LoopDecision::KeepWaiting(Some(Signal::BadCredentials))
        );
    }

    #[test]
    fn challenge_promotes_in_headless_but_waits_in_headed() {
        assert_eq!(
            decide_loop_step(None, Signal::Challenge(ChallengeKind::Captcha), false),
            LoopDecision::PromoteChallenge(ChallengeKind::Captcha)
        );
        assert_eq!(
            decide_loop_step(None, Signal::Challenge(ChallengeKind::Otp), true),
            LoopDecision::KeepWaiting(None)
        );
    }

    #[test]
    fn pending_resets_negative_accumulation() {
        assert_eq!(
            decide_loop_step(Some(Signal::Blocked), Signal::Pending, false),
            LoopDecision::KeepWaiting(None)
        );
    }

    #[test]
    fn classify_prioritizes_success_over_other_signals() {
        let signals = PageSignals {
            logged_in: true,
            bad_credentials: true,
            blocked: true,
            ..Default::default()
        };
        assert_eq!(classify(&signals), Signal::Success);
    }
}

// === 리뷰 #7: 패킷 쿠키가 이름만으로 합쳐져 서브도메인 간 누출되던 버그 ===
mod cookie_host_scoping {
    use super::*;

    #[test]
    fn domain_cookie_applies_to_subdomains() {
        assert!(cookie_applies_to_host(".naver.com", "stock.naver.com"));
        assert!(cookie_applies_to_host(".naver.com", "apis.naver.com"));
        assert!(cookie_applies_to_host(".naver.com", "naver.com"));
    }

    #[test]
    fn host_only_cookie_matches_exact_host_only() {
        assert!(cookie_applies_to_host("stock.naver.com", "stock.naver.com"));
        assert!(!cookie_applies_to_host(
            "stock.naver.com",
            "m.stock.naver.com"
        ));
        assert!(!cookie_applies_to_host("stock.naver.com", "apis.naver.com"));
    }

    #[test]
    fn host_only_cookie_does_not_leak_across_hosts() {
        // 같은 이름 NNB가 도메인 전역(.naver.com)과 host-only(stock.naver.com) 둘 다 존재.
        let cookies = vec![
            NaverCookie::new(".naver.com", "NID_AUT", "aut"),
            NaverCookie::new(".naver.com", "NID_SES", "ses"),
            NaverCookie::new(".naver.com", "NNB", "global"),
            NaverCookie::new("stock.naver.com", "NNB", "stockonly"),
        ];

        let stock = build_cookie_header(&cookies, "stock.naver.com");
        assert!(
            stock.contains("NNB=stockonly"),
            "host-only가 자기 호스트에서 우선"
        );

        let apis = build_cookie_header(&cookies, "apis.naver.com");
        assert!(apis.contains("NNB=global"));
        assert!(
            !apis.contains("stockonly"),
            "stock host-only 쿠키가 apis.naver.com 요청으로 새면 안 된다"
        );
    }

    #[test]
    fn session_cookies_present_on_every_naver_host() {
        let cookies = vec![
            NaverCookie::new(".naver.com", "NID_AUT", "aut"),
            NaverCookie::new(".naver.com", "NID_SES", "ses"),
        ];
        for host in [
            "stock.naver.com",
            "m.stock.naver.com",
            "apis.naver.com",
            "static.nid.naver.com",
        ] {
            let header = build_cookie_header(&cookies, host);
            assert!(
                header.contains("NID_AUT=aut") && header.contains("NID_SES=ses"),
                "세션 쿠키는 모든 네이버 호스트에 실려야 한다: {host}"
            );
        }
    }

    #[test]
    fn empty_cookie_jar_yields_empty_header() {
        assert_eq!(build_cookie_header(&[], "stock.naver.com"), "");
    }
}
