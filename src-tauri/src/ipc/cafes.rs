//! Naver cafe destinations (네이버 카페) domain — JSON-file-backed, served over
//! Tauri IPC. These are the user's connected cafes and their boards, used as
//! publish targets.
//!
//! A cafe is registered through the "+ 카페 추가" flow, which resolves the user's
//! URL/slug input into a numeric `cafeId` and discovers its writable boards once,
//! then caches the result here. The publish modal reads this cache directly — it
//! never re-discovers at publish time — so each [`Board`] carries the real
//! `menuId`/`boardType` the article-write API needs.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::auth::read_account_cookies;
use crate::naver_cafe::distribute::{distribute_comments, mulberry32, seed_from_clock};
use crate::naver_cafe::post::cookie_header_from_storage_state;
use crate::naver_cafe::{
    fetch_article_list_for_account, run_comment_jobs as run_comment_jobs_internal,
    run_post_jobs as run_jobs, ArticleListResponse, CafeOrchestrator, CommentJob, CommentJobReport,
    ErrorEnvelope, JobReport, JoinedCafe, Menu, NaverCafeCommonErrorData, PostJob, SortBy,
    CODE_NO_COOKIES,
};
use crate::store::JsonStore;

/// A single writable board (menu) inside a cafe, as resolved at registration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../src/shared/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct Board {
    /// Display name (e.g. "자유게시판").
    pub name: String,
    /// Numeric board (menu) id — used by the article-write API. Maps to a JS
    /// `number` (not `bigint`): IPC serializes via JSON and cafe/menu ids stay
    /// well within `Number.MAX_SAFE_INTEGER`.
    #[ts(type = "number")]
    pub menu_id: u64,
    /// Board layout type (e.g. "L") — used in the write Referer header.
    pub board_type: String,
}

/// A publish-target cafe. Resolved once at registration and cached.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../src/shared/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct Cafe {
    /// Display name.
    pub name: String,
    /// Original reference the user entered (URL/slug/numeric) — kept for
    /// re-resolution and display.
    pub cafe_ref: String,
    /// Resolved numeric cafe id. Maps to a JS `number` (see [`Board::menu_id`]).
    #[ts(type = "number")]
    pub cafe_id: u64,
    /// Writable boards discovered at registration.
    pub boards: Vec<Board>,
}

/// Cafes are registered by the user (via discovery), so the default seed is empty.
pub fn seed() -> Vec<Cafe> {
    Vec::new()
}

#[tauri::command]
pub fn list_cafes(store: tauri::State<'_, JsonStore<Cafe>>) -> Vec<Cafe> {
    store.snapshot()
}

/// Insert `cafe`, or replace an existing one with the same `cafeId` in place.
///
/// `cafeId` is the cafe's resolved identity, so re-registering the same cafe
/// (e.g. to refresh its boards) updates it rather than duplicating. New cafes
/// are prepended so the most recently added shows first.
pub fn apply_upsert(mut cafes: Vec<Cafe>, cafe: Cafe) -> Vec<Cafe> {
    match cafes.iter_mut().find(|c| c.cafe_id == cafe.cafe_id) {
        Some(slot) => *slot = cafe,    // update in place — single pass, no clone
        None => cafes.insert(0, cafe), // new → prepend (most recent first)
    }
    cafes
}

/// Persist a resolved cafe (from [`resolve_cafe`]); returns the updated list.
#[tauri::command]
pub fn upsert_cafe(store: tauri::State<'_, JsonStore<Cafe>>, cafe: Cafe) -> Vec<Cafe> {
    store.mutate(|cafes| apply_upsert(cafes, cafe))
}

/// Error type for cafe resolution — the same envelope the discovery layer uses,
/// so its errors propagate with `?`.
type ResolveCafeError = ErrorEnvelope<NaverCafeCommonErrorData>;

/// Map the orchestrator's writable [`Menu`] list into cached [`Board`]s.
///
/// `list_boards` already filters to general writable boards, so each menu maps
/// to a board verbatim.
fn boards_from_menus(menus: &[Menu]) -> Vec<Board> {
    menus
        .iter()
        .map(|m| Board {
            name: m.menu_name.clone(),
            menu_id: m.menu_id,
            board_type: m.board_type.clone(),
        })
        .collect()
}

/// Assemble a registrable [`Cafe`] from the resolved id, name, and boards.
fn assemble_cafe(cafe_ref: String, cafe_id: u64, cafe_name: String, menus: &[Menu]) -> Cafe {
    Cafe {
        name: cafe_name,
        cafe_ref,
        cafe_id,
        boards: boards_from_menus(menus),
    }
}

/// Build a NO_COOKIES error envelope. Cookie values are never included.
fn no_cookies_error(account_id: &str, detail: Option<String>) -> ResolveCafeError {
    let message = match detail {
        Some(d) => format!("계정 '{}'의 쿠키를 읽지 못했습니다: {}", account_id, d),
        None => format!(
            "계정 '{}'의 세션 쿠키가 없거나 만료되었습니다. 다시 로그인하세요.",
            account_id
        ),
    };
    ErrorEnvelope {
        trace_id: String::new(),
        code: CODE_NO_COOKIES.to_string(),
        message,
        error_data: Some(NaverCafeCommonErrorData {
            target: None,
            http_status: None,
            api_error_code: None,
            api_error_message: None,
            retryable: false,
        }),
    }
}

/// Resolve a user's cafe reference (URL/slug/numeric) into a registrable cafe.
///
/// Runs the discovery trio once — id resolution, basic info, writable boards —
/// using `account_id`'s stored session cookie. This backs the "+ 카페 추가" flow:
/// the UI caches the result so publishing never re-discovers. Cookie values
/// never appear in the returned error.
#[tauri::command]
pub async fn resolve_cafe(input: String, account_id: String) -> Result<Cafe, ResolveCafeError> {
    let cookie_value = match read_account_cookies(&account_id) {
        Ok(Some(value)) => value,
        Ok(None) => return Err(no_cookies_error(&account_id, None)),
        Err(e) => return Err(no_cookies_error(&account_id, Some(e.to_string()))),
    };
    let cookie_header = cookie_header_from_storage_state(&cookie_value)
        .ok_or_else(|| no_cookies_error(&account_id, None))?;
    let cookie = Some(cookie_header.as_str());

    let orchestrator = CafeOrchestrator::new();
    let cafe_id = orchestrator.resolve_cafe_id(&input, cookie).await?;
    // 카페 정보·게시판 목록 조회는 둘 다 cafe_id에만 의존하고 서로 독립적이라
    // 동시에 보낸다(순차 시 왕복 2회 → 1회).
    let (info, menus) = tokio::join!(
        orchestrator.fetch_cafe_info(cafe_id, cookie),
        orchestrator.list_boards(cafe_id, cookie),
    );
    let info = info?;
    let menus = menus?;

    Ok(assemble_cafe(input, cafe_id, info.cafe_name, &menus))
}

/// List every cafe the account has joined (crawled across all pages).
///
/// Reads `account_id`'s stored session cookie and queries the "내 카페 관리 >
/// 가입 카페" API. Backs an account-driven "가입 카페 자동 로드" flow so the user
/// doesn't paste cafe URLs by hand. Cookie values never appear in the returned
/// error.
#[tauri::command]
pub async fn list_joined_cafes(
    account_id: String,
) -> Result<Vec<JoinedCafe>, ErrorEnvelope<NaverCafeCommonErrorData>> {
    let cookie_value = match read_account_cookies(&account_id) {
        Ok(Some(value)) => value,
        Ok(None) => return Err(no_cookies_error(&account_id, None)),
        Err(e) => return Err(no_cookies_error(&account_id, Some(e.to_string()))),
    };
    let cookie_header = cookie_header_from_storage_state(&cookie_value)
        .ok_or_else(|| no_cookies_error(&account_id, None))?;

    let orchestrator = CafeOrchestrator::new();
    orchestrator
        .list_joined_cafes(Some(cookie_header.as_str()))
        .await
}

/// Map the UI's `sortBy` string into the backend [`SortBy`] enum.
///
/// Accepts the camelCase serde forms ("latest"/"popular"). Unknown values
/// return `Err` so the caller can surface an `INVALID_SORT_BY` error rather
/// than silently defaulting. Returns the lightweight unit error to keep the
/// large [`ErrorEnvelope`] off this helper's `Result` (clippy::result_large_err).
fn parse_sort_by(sort_by: &str) -> Result<SortBy, ()> {
    match sort_by {
        "latest" => Ok(SortBy::Latest),
        "popular" => Ok(SortBy::Popular),
        _ => Err(()),
    }
}

/// Build the `INVALID_SORT_BY` error envelope for an unrecognized sort value.
fn invalid_sort_by_error(sort_by: &str) -> ErrorEnvelope<NaverCafeCommonErrorData> {
    ErrorEnvelope {
        trace_id: String::new(),
        code: "INVALID_SORT_BY".to_string(),
        message: format!(
            "정렬 기준 '{}'을(를) 인식하지 못했습니다. latest 또는 popular여야 합니다.",
            sort_by
        ),
        error_data: None,
    }
}

/// List a cafe's articles (latest / popular) for the given account.
///
/// Reads `accountId`'s stored session cookie and queries the article-list API,
/// sorted by `sortBy` ("latest" | "popular"). Cookie values never appear in the
/// returned error.
#[tauri::command]
pub async fn list_cafe_articles(
    cafe_id: String,
    sort_by: String,
    account_id: String,
) -> Result<ArticleListResponse, ErrorEnvelope<NaverCafeCommonErrorData>> {
    let sort = parse_sort_by(&sort_by).map_err(|()| invalid_sort_by_error(&sort_by))?;
    // 미리보기는 첫 페이지(상위 15개)만 보여주면 충분하다.
    fetch_article_list_for_account(&cafe_id, sort, 1, &account_id).await
}

/// Slim per-job result returned to the UI — exactly what the publish modal
/// renders. The rich internal `JobReport`/`PostError` stays backend-only.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../src/shared/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct PublishOutcome {
    /// Account the job ran under.
    pub account_id: String,
    /// Original cafe reference the job targeted.
    pub cafe: String,
    /// Target board (menu) id.
    #[ts(type = "number")]
    pub menu_id: u64,
    /// Whether the article was posted.
    pub success: bool,
    /// Registered article id, on success.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional, type = "number")]
    pub article_id: Option<u64>,
    /// Error code, on failure (e.g. "INVALID_CAFE_INPUT", "NO_COOKIES").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub error_code: Option<String>,
    /// Human-readable error message, on failure. Never contains cookie values.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub error_message: Option<String>,
}

/// Map the internal [`JobReport`] to the slim UI-facing [`PublishOutcome`].
pub(crate) fn outcome_from_report(report: &JobReport) -> PublishOutcome {
    PublishOutcome {
        account_id: report.account_id.clone(),
        cafe: report.cafe.clone(),
        menu_id: report.menu_id,
        success: report.success,
        article_id: report.result.as_ref().map(|r| r.article_id),
        error_code: report.error.as_ref().map(|e| e.code.clone()),
        error_message: report.error.as_ref().map(|e| e.message.clone()),
    }
}

/// Run N publish jobs sequentially, returning a slim outcome per job.
///
/// Each job reads its account's session cookie internally; one job failing does
/// not stop the rest. Cookie values never appear in any outcome.
#[tauri::command]
pub async fn run_post_jobs(jobs: Vec<PostJob>) -> Vec<PublishOutcome> {
    run_jobs(&jobs)
        .await
        .iter()
        .map(outcome_from_report)
        .collect()
}

/// Slim per-job comment result returned to the UI. The rich internal
/// `CommentJobReport`/`CommentError` stays backend-only — mirrors
/// [`PublishOutcome`] for the post path.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../src/shared/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct CommentPublishOutcome {
    /// Account the job ran under.
    pub account_id: String,
    /// Target cafe id.
    #[ts(type = "number")]
    pub cafe_id: u64,
    /// Target article id the comment was posted to.
    #[ts(type = "number")]
    pub article_id: u64,
    /// Whether the comment was posted.
    pub success: bool,
    /// Registered comment id, on success.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional, type = "number")]
    pub comment_id: Option<u64>,
    /// Error code, on failure (e.g. "NO_COOKIES", "COMMENT_HTTP_ERROR").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub error_code: Option<String>,
    /// Human-readable error message, on failure. Never contains cookie values.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub error_message: Option<String>,
}

/// Map the internal [`CommentJobReport`] to the slim UI-facing outcome.
pub(crate) fn comment_outcome_from_report(report: &CommentJobReport) -> CommentPublishOutcome {
    CommentPublishOutcome {
        account_id: report.account_id.clone(),
        cafe_id: report.cafe_id,
        article_id: report.article_id,
        success: report.success,
        comment_id: report.result.as_ref().map(|r| r.comment_id),
        error_code: report.error.as_ref().map(|e| e.code.clone()),
        error_message: report.error.as_ref().map(|e| e.message.clone()),
    }
}

/// One comment destination — the account plus the numeric cafe/article it will
/// comment on. The comment *text* is not chosen here: the backend deals it from
/// the pool (see [`run_comment_jobs`]). Comes either from a just-posted article
/// or a URL the UI parsed.
#[derive(Debug, Clone, Deserialize, TS)]
#[ts(export, export_to = "../../../src/shared/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct CommentDistributionTarget {
    /// Account the comment runs under (cookie-file lookup key).
    pub account_id: String,
    /// Target cafe id. (JS `number`)
    #[ts(type = "number")]
    pub cafe_id: u64,
    /// Target article id the comment attaches to.
    #[ts(type = "number")]
    pub article_id: u64,
}

/// Request for [`run_comment_jobs`]: the destinations plus the comment pool.
/// The backend shuffles `comments` and deals one to each target (issue #98), so
/// different accounts post different comments — matching the UI's promise.
#[derive(Debug, Clone, Deserialize, TS)]
#[ts(export, export_to = "../../../src/shared/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct CommentDistributionRequest {
    /// Where each comment lands (one comment dealt per target).
    pub targets: Vec<CommentDistributionTarget>,
    /// The comment text pool to distribute across the targets.
    pub comments: Vec<String>,
}

/// Distribute the comment pool across the targets (issue #98), then run the
/// resulting jobs sequentially, returning a slim outcome per job.
///
/// The pool is shuffled and dealt one comment per target with a wall-clock-seeded
/// RNG, so accounts don't all post the same text in the same order. Each job
/// reads its account's session cookie internally; one job failing does not stop
/// the rest. Cookie values never appear in any outcome.
#[tauri::command]
pub async fn run_comment_jobs(req: CommentDistributionRequest) -> Vec<CommentPublishOutcome> {
    let mut rng = mulberry32(seed_from_clock());
    let contents = distribute_comments(req.targets.len(), &req.comments, &mut rng);
    let jobs: Vec<CommentJob> = req
        .targets
        .into_iter()
        .zip(contents)
        .map(|(t, content)| CommentJob {
            account_id: t.account_id,
            cafe_id: t.cafe_id,
            article_id: t.article_id,
            content,
        })
        .collect();
    run_comment_jobs_internal(&jobs)
        .await
        .iter()
        .map(comment_outcome_from_report)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::naver_cafe::post::ArticleRegisterResult;
    use crate::naver_cafe::{JobReport, Menu};
    use serde_json::json;

    fn success_report() -> JobReport {
        JobReport {
            account_id: "acc1".into(),
            cafe: "cafe.naver.com/x".into(),
            menu_id: 1,
            success: true,
            result: Some(ArticleRegisterResult {
                cafe_id: 100,
                article_id: 55,
                menu_id: 1,
            }),
            error: None,
        }
    }

    fn failure_report() -> JobReport {
        JobReport {
            account_id: "acc2".into(),
            cafe: "bad-input".into(),
            menu_id: 2,
            success: false,
            result: None,
            error: Some(ErrorEnvelope {
                trace_id: String::new(),
                code: "INVALID_CAFE_INPUT".into(),
                message: "카페 식별자를 인식하지 못했습니다".into(),
                error_data: None,
            }),
        }
    }

    #[test]
    fn parse_sort_by_accepts_latest_and_popular() {
        assert_eq!(parse_sort_by("latest").unwrap(), SortBy::Latest);
        assert_eq!(parse_sort_by("popular").unwrap(), SortBy::Popular);
    }

    #[test]
    fn parse_sort_by_rejects_unknown() {
        assert!(
            parse_sort_by("trending").is_err(),
            "알 수 없는 값은 Err여야 함"
        );
    }

    #[test]
    fn invalid_sort_by_error_carries_code() {
        let err = invalid_sort_by_error("trending");
        assert_eq!(err.code, "INVALID_SORT_BY");
        assert!(err.message.contains("trending"));
    }

    #[tokio::test]
    async fn list_cafe_articles_with_unknown_sort_returns_invalid_sort_by() {
        let err = list_cafe_articles("31732304".into(), "trending".into(), "acc".into())
            .await
            .expect_err("잘못된 정렬은 Err여야 함");
        assert_eq!(err.code, "INVALID_SORT_BY");
    }

    #[tokio::test]
    async fn list_cafe_articles_without_session_returns_no_cookies() {
        // 정렬은 유효하지만 쿠키 없는 계정 → NO_COOKIES (네트워크 미발생).
        let err = list_cafe_articles(
            "31732304".into(),
            "latest".into(),
            "no-such-account-xyz".into(),
        )
        .await
        .expect_err("쿠키 없는 계정은 Err여야 함");
        assert_eq!(err.code, CODE_NO_COOKIES);
    }

    #[tokio::test]
    async fn list_joined_cafes_without_session_returns_no_cookies() {
        // 존재하지 않는 계정 → 쿠키 없음/읽기 실패 → NO_COOKIES (네트워크 미발생).
        let err = list_joined_cafes("no-such-account-xyz".to_string())
            .await
            .expect_err("쿠키 없는 계정은 Err여야 함");
        assert_eq!(err.code, CODE_NO_COOKIES);
    }

    #[test]
    fn outcome_from_success_report_carries_article_id() {
        let o = outcome_from_report(&success_report());
        assert!(o.success);
        assert_eq!(o.account_id, "acc1");
        assert_eq!(o.cafe, "cafe.naver.com/x");
        assert_eq!(o.menu_id, 1);
        assert_eq!(o.article_id, Some(55));
        assert_eq!(o.error_code, None);
        assert_eq!(o.error_message, None);
    }

    #[test]
    fn outcome_from_failure_report_carries_error_code_and_message() {
        let o = outcome_from_report(&failure_report());
        assert!(!o.success);
        assert_eq!(o.article_id, None);
        assert_eq!(o.error_code.as_deref(), Some("INVALID_CAFE_INPUT"));
        assert_eq!(
            o.error_message.as_deref(),
            Some("카페 식별자를 인식하지 못했습니다")
        );
    }

    fn comment_success_report() -> CommentJobReport {
        CommentJobReport {
            account_id: "acc1".into(),
            cafe_id: 31732304,
            article_id: 9,
            success: true,
            result: Some(crate::naver_cafe::CommentResult {
                comment_id: 62628988,
                ref_comment_id: 62628988,
            }),
            error: None,
        }
    }

    fn comment_failure_report() -> CommentJobReport {
        CommentJobReport {
            account_id: "acc2".into(),
            cafe_id: 31732304,
            article_id: 9,
            success: false,
            result: None,
            error: Some(ErrorEnvelope {
                trace_id: String::new(),
                code: "COMMENT_HTTP_ERROR".into(),
                message: "댓글 등록 요청이 실패했습니다.".into(),
                error_data: None,
            }),
        }
    }

    #[test]
    fn comment_outcome_from_success_report_carries_comment_id() {
        let o = comment_outcome_from_report(&comment_success_report());
        assert!(o.success);
        assert_eq!(o.account_id, "acc1");
        assert_eq!(o.cafe_id, 31732304);
        assert_eq!(o.article_id, 9);
        assert_eq!(o.comment_id, Some(62628988));
        assert_eq!(o.error_code, None);
        assert_eq!(o.error_message, None);
    }

    #[test]
    fn comment_outcome_from_failure_report_carries_error_code_and_message() {
        let o = comment_outcome_from_report(&comment_failure_report());
        assert!(!o.success);
        assert_eq!(o.comment_id, None);
        assert_eq!(o.error_code.as_deref(), Some("COMMENT_HTTP_ERROR"));
        assert_eq!(
            o.error_message.as_deref(),
            Some("댓글 등록 요청이 실패했습니다.")
        );
    }

    fn menu(menu_id: u64, name: &str, board_type: &str) -> Menu {
        Menu {
            cafe_id: 31732304,
            menu_id,
            menu_name: name.into(),
            menu_type: "B".into(),
            board_type: board_type.into(),
            writable: true,
            hidden: false,
            separator_menu_type: false,
            use_head: false,
            order: 0,
        }
    }

    #[test]
    fn boards_from_menus_maps_id_name_and_type() {
        let menus = vec![menu(1, "자유게시판", "L"), menu(5, "공지사항", "M")];
        assert_eq!(
            boards_from_menus(&menus),
            vec![
                Board {
                    name: "자유게시판".into(),
                    menu_id: 1,
                    board_type: "L".into(),
                },
                Board {
                    name: "공지사항".into(),
                    menu_id: 5,
                    board_type: "M".into(),
                },
            ]
        );
    }

    #[test]
    fn assemble_cafe_builds_from_parts() {
        let menus = vec![menu(1, "자유게시판", "L")];
        let cafe = assemble_cafe(
            "cafe.naver.com/testcafe".into(),
            31732304,
            "테스트카페".into(),
            &menus,
        );
        assert_eq!(cafe.name, "테스트카페");
        assert_eq!(cafe.cafe_ref, "cafe.naver.com/testcafe");
        assert_eq!(cafe.cafe_id, 31732304);
        assert_eq!(cafe.boards, boards_from_menus(&menus));
    }

    #[test]
    fn upsert_prepends_a_new_cafe() {
        let mut other = sample();
        other.cafe_id = 999;
        other.name = "다른 카페".into();
        let next = apply_upsert(vec![sample()], other);
        assert_eq!(next.len(), 2);
        assert_eq!(next[0].cafe_id, 999);
    }

    #[test]
    fn upsert_replaces_existing_by_cafe_id() {
        let mut edited = sample();
        edited.name = "이름 변경".into();
        edited.boards = vec![];
        let next = apply_upsert(vec![sample()], edited);
        assert_eq!(next.len(), 1);
        assert_eq!(next[0].name, "이름 변경");
        assert!(next[0].boards.is_empty());
    }

    fn sample() -> Cafe {
        Cafe {
            name: "테스트 카페".into(),
            cafe_ref: "cafe.naver.com/testcafe".into(),
            cafe_id: 31732304,
            boards: vec![Board {
                name: "자유게시판".into(),
                menu_id: 1,
                board_type: "L".into(),
            }],
        }
    }

    #[test]
    fn seed_is_empty_until_user_registers() {
        // 카페는 "+ 카페 추가"로 사용자가 직접 해석·등록하므로 기본 시드는 비어 있다.
        assert!(seed().is_empty());
    }

    #[test]
    fn serializes_with_camel_case_keys() {
        let value = serde_json::to_value(sample()).unwrap();
        assert_eq!(
            value,
            json!({
                "name": "테스트 카페",
                "cafeRef": "cafe.naver.com/testcafe",
                "cafeId": 31732304,
                "boards": [
                    { "name": "자유게시판", "menuId": 1, "boardType": "L" }
                ]
            })
        );
    }

    #[test]
    fn roundtrips_through_json() {
        let cafe = sample();
        let back: Cafe = serde_json::from_str(&serde_json::to_string(&cafe).unwrap()).unwrap();
        assert_eq!(cafe, back);
    }
}
