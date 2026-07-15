//! 하위 에이전트 레이어(설계 §9). 기존 pstmacro 앱에 **추가만** 되는 모듈 — 기존 로그인·큐·계정
//! 코드는 한 줄도 바꾸지 않고, 그 함수/스토어를 호출만 한다(adb.rs의 상태신호는 "추가").
//!
//! 하는 일:
//! ① 서버에 SSE로 연결해 명령 수신(distribute/login/delete)
//! ② 받은 계정을 기존 계정 스토어에 등록 + 기존 종토 선택로그인 경로로 자동 전체 로그인 enqueue
//! ③ 로그인 끝나면 결과 4분류(성공/보류/대기초과/실패) + 누적을 서버에 보고 + 실패 자동삭제(§10-4)
//! ④ 하트비트(현재 IP)·ROTATING 상태 보고(§4) + 끊기면 백오프 재연결(§4-1)

mod config;
mod net;

pub use config::AgentConfig;

use std::sync::OnceLock;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, Runtime};
use tokio::sync::mpsc;

use crate::ipc::accounts::{Account, AccountStatus, PlatformId};
use crate::ipc::log_batches::LogBatch;
use crate::ipc::posts::{CommentTarget, ModeValue};
use crate::ipc::queue::{
    apply_priority_order, as_fresh_now_item, BandTarget, BlogTarget, ClipTarget, CommentTargetSpec,
    ContentChange, ForumTarget, LoginTarget, NaverTarget, PublishPlan, QueueLocation, QueueNowItem,
    QueueState,
};
use crate::ipc::queue_runner::{start_if_idle, NowQueueRunner};
use crate::store::JsonStore;

/// 서버가 SSE로 내려보내는 명령.
#[derive(Deserialize)]
struct Command {
    #[serde(rename = "type")]
    kind: String,
    #[serde(rename = "commandId", default)]
    command_id: Option<String>,
    #[serde(default)]
    accounts: Vec<AccountIn>,
    /// 게시 명령(`publish_posts`)일 때만 채워진다 — 서버가 확정한 계정×종목·글 정보(07-게시명령).
    #[serde(default)]
    publish: Option<PublishCmd>,
    /// 중지 명령(`kill_publish`)일 때만 채워진다 — 어느 큐(queueId)/계정(loginId)/전체(all)를
    /// 완전 종료할지(설계서 08 §10).
    #[serde(default)]
    kill: Option<KillCmd>,
    /// 계정 상태/플랫폼 원격 편집 명령(`update_account_meta`)일 때만 채워진다(14-계정상태-관리).
    /// Admin이 하위 accountRows를 폴링해 바꾼 행만 모아 보낸다(loginId별 platform·status 선택 갱신).
    #[serde(rename = "accountUpdates", default)]
    account_updates: Vec<AccountMetaUpdate>,
    /// 닉네임 잔여 횟수 실시간 조회 명령(`query_nickname_remaining`)일 때만 채워진다(15-기타명령 §3·
    /// 결정 §6-2 실시간). 하위가 계정별 `forum_nickname_remaining`을 조회해 서버로 회신한다.
    #[serde(rename = "nicknameQuery", default)]
    nickname_query: Option<NicknameQueryCmd>,
    /// 기타 명령(`like_posts`/`dislike_posts`/`boost_view`/`rotate_ip`)일 때만 채워진다(15-기타명령 §2).
    #[serde(default)]
    etc: Option<EtcCmd>,
    /// 블로그 새 글 발행 명령(`publish_blog_write`)일 때만 채워진다(16-블로그새글). Admin이 편집기
    /// 툴바로 작성한 제목·본문 블록·발행설정 + 대상 계정(계정별 블로그명)을 실어 보낸다. 하위는 이
    /// 명령을 게시 큐에 태우지 않고, 계정 쿠키로 `naver_blog::publish_blog_post_blocks_for_account`를
    /// 직접 호출해 RabbitWrite로 발행한다(카페/밴드 게시 큐 경로와 별개).
    #[serde(rename = "blogWrite", default)]
    blog_write: Option<BlogWriteCmd>,
}

/// 블로그 새 글 발행 페이로드(16-블로그새글). 제목·본문 블록·발행설정은 대상 계정 전체에 공통이며,
/// `targets`가 계정별 (loginId, 블로그명) 쌍을 담는다. 각 계정이 자기 블로그에 같은 글을 발행한다.
#[derive(Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
struct BlogWriteCmd {
    #[serde(default)]
    title: String,
    /// 편집기 블록 배열(프론트 blocks.ts 모양). 서버는 그대로 통과시키고, 하위가 여기서
    /// `naver_blog::Block`으로 파싱한다(파싱 실패 블록은 무시하지 않고 발행 자체를 실패로 보고).
    #[serde(default)]
    blocks: Vec<serde_json::Value>,
    #[serde(default)]
    settings: BlogWriteSettings,
    #[serde(default)]
    targets: Vec<BlogWriteTarget>,
}

/// 블로그 발행 설정(Admin이 고른 공개범위·댓글·검색·태그). 나머지 세부 설정은 하위 기본값을 쓴다.
#[derive(Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct BlogWriteSettings {
    /// 공개 범위: 0=전체공개·1=이웃공개·2=서로이웃공개·3=비공개.
    #[serde(default)]
    open_type: u8,
    #[serde(default = "default_true")]
    comment_yn: bool,
    #[serde(default = "default_true")]
    search_yn: bool,
    /// 태그(# 없이 공백 구분). 빈 문자열이면 태그 없음.
    #[serde(default)]
    tags: String,
}

impl Default for BlogWriteSettings {
    fn default() -> Self {
        Self {
            open_type: 0,
            comment_yn: true,
            search_yn: true,
            tags: String::new(),
        }
    }
}

fn default_true() -> bool {
    true
}

/// 블로그 새 글 발행 대상 1건 — (계정 loginId, 발행할 블로그명). blogId가 비면 loginId를 블로그명으로.
#[derive(Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
struct BlogWriteTarget {
    #[serde(default)]
    login_id: String,
    #[serde(default)]
    blog_id: String,
}

/// 닉네임 잔여 횟수 조회 페이로드(15-기타명령 §3). Admin이 닉네임 랜덤 체크박스를 켤 때, 선택한
/// 종토 계정들의 loginId를 실어 보낸다. 하위가 각 계정의 `forum_nickname_remaining`을 조회해 회신.
#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct NicknameQueryCmd {
    #[serde(default)]
    login_ids: Vec<String>,
}

/// 기타 명령 페이로드(15-기타명령 §2). 좋아요/싫어요=links×loginIds, 조회수=links×repeats,
/// IP 변경=빈값. Admin이 고른 하위 1대에 SSE로 내려온다. 하위는 게시 큐를 타지 않고 데스크톱
/// 즉시 실행 엔진(`run_reaction_batch`/`boost_views`/`toggle_airplane_mode`)을 그대로 호출한다.
#[derive(Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
struct EtcCmd {
    #[serde(default)]
    links: Vec<String>,
    #[serde(default)]
    login_ids: Vec<String>,
    #[serde(default)]
    repeats: u32,
}

/// 계정 1건의 원격 메타 편집(14-계정상태-관리 §4). loginId로 매칭해 platform(있으면)·status(있으면)만
/// 바꾼다 — 둘 다 Option이라 생략한 필드는 건드리지 않는다. status는 사람이 되돌릴 수 있는 값
/// (active/waiting/onHold)만 유효하고, 그 외(워커 판정값)는 무시한다.
#[derive(Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
struct AccountMetaUpdate {
    login_id: String,
    #[serde(default)]
    platform: Option<String>,
    #[serde(default)]
    status: Option<String>,
}

/// 중지 명령 페이로드(설계서 08). queueId=그 큐 1개, all=디바이스 전 실행/대기 큐, loginId=그
/// 계정 큐. Admin "중지 명령" 페이지가 실시간 큐 스냅샷으로 queueId를, 디바이스 버튼이 all을 보낸다.
#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct KillCmd {
    #[serde(default)]
    queue_id: Option<String>,
    #[serde(default)]
    all: bool,
    #[serde(default)]
    login_id: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AccountIn {
    login_id: String,
    pw: String,
    /// 계정 플랫폼("forum"/"naver"/"blog"/"clip"/"band"). 빈값=forum(하위호환). 카페("naver")는
    /// 분배 시 로그인하지 않고 등록만 한다(카페는 게시 순간 로그인). 그 외는 종전대로 자동 로그인.
    #[serde(default)]
    platform: String,
}

/// 계정 플랫폼 문자열 → PlatformId(빈값·미상=Forum). 카페=naver.
fn platform_from_str(s: &str) -> PlatformId {
    match s {
        "naver" => PlatformId::Naver,
        "blog" => PlatformId::Blog,
        "clip" => PlatformId::Clip,
        "band" => PlatformId::Band,
        "instagram" => PlatformId::Instagram,
        "threads" => PlatformId::Threads,
        _ => PlatformId::Forum,
    }
}

/// PlatformId → 인벤토리/명령용 문자열(프론트 PlatformId 리터럴과 동일, lowercase).
fn platform_to_str(p: &PlatformId) -> &'static str {
    match p {
        PlatformId::Forum => "forum",
        PlatformId::Naver => "naver",
        PlatformId::Blog => "blog",
        PlatformId::Clip => "clip",
        PlatformId::Band => "band",
        PlatformId::Instagram => "instagram",
        PlatformId::Threads => "threads",
    }
}

/// 카페(네이버 카페) 계정인지 — 분배 시 로그인 건너뛰기 판정용.
fn is_cafe_platform(s: &str) -> bool {
    s == "naver"
}

/// 로그인 엔진 선택용 플랫폼: **밴드만 Band**(band.us 로그인=process_band_account), 그 외(종토/
/// 블로그/클립 등)는 **Naver**(process_account). 러너가 LoginTarget.platform으로 분기하므로
/// (queue_runner.rs), 밴드 계정은 반드시 Band로 태워야 band.us 로그인이 돈다. 문자열(AccountIn) 판.
fn login_platform_from_str(s: &str) -> PlatformId {
    if s == "band" {
        PlatformId::Band
    } else {
        PlatformId::Naver
    }
}

/// 위와 동일하되 저장된 계정의 PlatformId 판(import_then_login_all 재로그인용).
fn login_platform_for(p: &PlatformId) -> PlatformId {
    match p {
        PlatformId::Band => PlatformId::Band,
        _ => PlatformId::Naver,
    }
}

/// 계정 상태 → 인벤토리용 문자열(AccountStatus serde camelCase와 동일).
fn status_to_str(s: &AccountStatus) -> &'static str {
    match s {
        AccountStatus::New => "new",
        AccountStatus::Active => "active",
        AccountStatus::Waiting => "waiting",
        AccountStatus::OnHold => "onHold",
        AccountStatus::TimedOut => "timedOut",
        AccountStatus::BadCredentials => "badCredentials",
        AccountStatus::Challenge => "challenge",
        AccountStatus::Relogin => "relogin",
        AccountStatus::Blocked => "blocked",
        AccountStatus::Error => "error",
    }
}

/// 원격 편집이 허용하는 상태 문자열 → AccountStatus. **사람이 되돌릴 수 있는** 값(active/waiting/
/// onHold)만 받고, 그 외(badCredentials/blocked 등 워커 판정값)는 None을 돌려 무시한다
/// (14-계정상태-관리 §5). 와이어 문자열은 status_to_str(AccountStatus serde camelCase)와 동일.
fn status_from_str(s: &str) -> Option<AccountStatus> {
    match s {
        "active" => Some(AccountStatus::Active),
        "waiting" => Some(AccountStatus::Waiting),
        "onHold" => Some(AccountStatus::OnHold),
        _ => None,
    }
}

/// Admin이 보낸 계정 메타 편집을 계정 목록에 반영한다(순수, 14-계정상태-관리 §4). 각 update는
/// loginId로 매칭해 platform(있으면 platform_from_str로 변환)·status(있으면 status_from_str로
/// 변환, 유효값만)를 갱신한다. 생략한 필드는 그대로 두고, 매칭되는 loginId가 없으면 무시한다.
/// status는 코어 `apply_status_by_login_id`를 재사용해 "사람이 되돌림"(status_msg/trace=None)으로 쓴다.
fn apply_account_meta(mut accounts: Vec<Account>, updates: &[AccountMetaUpdate]) -> Vec<Account> {
    for u in updates {
        if let Some(p) = u.platform.as_deref() {
            let pid = platform_from_str(p);
            for a in accounts.iter_mut() {
                if a.login_id == u.login_id {
                    a.platform = pid.clone();
                }
            }
        }
        if let Some(st) = u.status.as_deref().and_then(status_from_str) {
            accounts = crate::ipc::accounts::apply_status_by_login_id(
                accounts,
                &u.login_id,
                st,
                None,
                None,
            );
        }
    }
    accounts
}

/// 게시 명령 페이로드 — 서버가 계정×종목을 확정해 내려보낸다(하위는 그대로 ForumTarget으로 조립).
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PublishCmd {
    post_id: String,
    #[serde(default)]
    post_title: String,
    #[serde(default)]
    target_label: String,
    #[serde(default)]
    split: bool,
    /// 게시 종류: "post"(글)·"comment"(댓글)·"both"(글+댓글). 빈값=post(하위호환).
    #[serde(default)]
    mode: String,
    /// 댓글 모드의 특정 게시글 URL들(종토 댓글=특정게시글, 사용자 확정 2026-07-06).
    #[serde(default)]
    comment_urls: Vec<String>,
    /// 종토 "특정 게시글" 댓글을 계정들에 1개씩 나눠 답기(#403). true면 전체 계정×URL을 단일 큐로
    /// 묶어 엔진이 링크마다 댓글을 계정에 1:1 분배한다. 댓글(comment) 모드에서만 의미. 빈값=false.
    #[serde(default)]
    forum_comment_distribute: bool,
    /// 게시 대상 플랫폼("forum"=종토(기본)·"naver"=네이버 카페). 빈값=forum(하위호환).
    /// forum이면 계정×종목(assignments.stocks), naver면 계정×게시판(cafe_boards)로 조립한다.
    #[serde(default)]
    target: String,
    /// 카페 게시판 링크에서 파싱한 대상들(target=="naver"일 때). 각 계정이 이 게시판(들)에
    /// 글/댓글을 올린다(계정×게시판). board_type은 게시 시점 백엔드가 menu_id로 해석한다.
    #[serde(default)]
    cafe_boards: Vec<CafeBoardIn>,
    /// 블로그 댓글 링크 파싱 결과(target=="blog"일 때). 각 계정이 이 블로그(들)에 댓글을 단다
    /// (계정×블로그링크). logNo가 있으면 특정 글, 없으면 최신 N개(count/categoryNo) 대상이다.
    #[serde(default)]
    blog_links: Vec<BlogLinkIn>,
    /// 클립 댓글 대상(target=="clip"일 때). 각 계정이 이 창작자(들)의 최신 N개 미디어에 댓글을 단다
    /// (계정×클립링크). 클립은 최신 N개 단일 모드다(특정 영상·인기 정렬 없음).
    #[serde(default)]
    clip_links: Vec<ClipLinkIn>,
    /// 밴드 게시 대상(target=="band"일 때). 각 계정이 이 밴드(들)에 글/댓글을 올린다(계정×밴드).
    /// 글/글+댓글=새 글, 댓글=아래 comment_mode/comment_count로 엔진이 최신/인기/특정글URL 해석.
    #[serde(default)]
    band_targets: Vec<BandTargetIn>,
    /// 카페·밴드 댓글 대상 모드(Admin 게시 명령에서 운영자가 고른 값). "url"(특정 글)·"latest"(최신)·
    /// "popular"(인기). 빈값이면 글(LibraryPost)에 저장된 commentTarget으로 폴백(하위호환).
    #[serde(default)]
    comment_mode: String,
    /// 카페·밴드 최신/인기 댓글 개수(상위 N). 0이면 글에 저장된 commentCount로 폴백(하위호환).
    #[serde(default)]
    comment_count: u32,
    /// 닉네임 랜덤 댓글(설계서 §2·15-기타명령). 종토 댓글 게시에서 각 댓글마다 닉네임을 랜덤으로
    /// 바꾼다(계정 내 중복 금지·5회 한도, 엔진이 처리). Admin이 켜면 true. 빈값=false(하위호환).
    #[serde(default)]
    comment_nickname_random: bool,
    /// 게시 후 내용 변경(설계서 §5·15-기타명령). 채워지면 종토 글 게시 후 `delaySec`초 뒤 새 제목/
    /// 본문으로 edit한다(엔진 spawn_forum_content_edit이 처리). None=변경 없음(하위호환).
    #[serde(default)]
    content_change: Option<ContentChange>,
    assignments: Vec<PublishAssign>,
}
/// 카페 게시판/글 링크 파싱 결과(Admin이 parseCafeBoardLink/parseCafeArticleUrl로 파싱해 보냄).
/// menu_id는 게시판(글쓰기 대상), article_id는 특정 글(url 댓글 대상). 둘 다 0이면 무시.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CafeBoardIn {
    cafe_id: u64,
    #[serde(default)]
    menu_id: u64,
    #[serde(default)]
    article_id: u64,
    #[serde(default)]
    link: String,
}
/// 블로그 댓글 링크 파싱 결과(Admin이 parseBlogPostLink/parseBlogLink로 파싱해 보냄). logNo가
/// 비어있지 않으면 특정 글 1개 댓글(count/categoryNo 미사용), 비어있으면 최신 N개 댓글
/// (count=글 수, categoryNo=글 목록 카테고리). blogId는 문자열 식별자(예: "press02").
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct BlogLinkIn {
    blog_id: String,
    #[serde(default)]
    log_no: String,
    #[serde(default)]
    category_no: u32,
    #[serde(default)]
    count: u32,
    #[serde(default)]
    link: String,
}
/// 클립 댓글 링크 파싱 결과(Admin이 parseClipLink로 파싱해 보냄). handle=창작자 핸들(@ 제외),
/// media_type="video"면 영상만·그 외/빈값=전체, count=최신 미디어 개수(기본 1). 클립은 최신 N개
/// 단일 모드라 특정 영상/인기 정렬 대상은 없다.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ClipLinkIn {
    handle: String,
    #[serde(default)]
    media_type: String,
    #[serde(default)]
    count: u32,
    #[serde(default)]
    link: String,
}
/// 밴드 게시 링크 파싱 결과(Admin이 bandNoFromLink/parseBandPostUrl로 파싱해 보냄). band_no=밴드
/// 식별자(표시·라벨용), link=게시 시점 백엔드가 band_no/post_no를 뽑는 원본 링크(밴드 홈 또는
/// 특정 글 URL). 댓글 대상 모드/개수는 글(commentTarget/commentCount)에 동결된 값을 쓴다.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct BandTargetIn {
    #[serde(default)]
    band_no: String,
    #[serde(default)]
    link: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PublishAssign {
    login_id: String,
    stocks: Vec<PublishStockIn>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PublishStockIn {
    code: String,
    #[serde(default)]
    name: String,
}

/// 에이전트 상태(하위 등록 화면 표시용).
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentStatus {
    pub configured: bool,
    pub server_url: String,
    pub device_name: String,
}

/// dispatch가 즉시 응답 후 백그라운드로 이어갈 후속 작업(로그인 결과 보고).
struct Followup {
    queue_id: String,
    login_ids: Vec<String>,
    // 이 분배에서 새로 등록된 계정 수와, 그중 로그인 엔진이 볼 수 있는 수(§10-1 등록 확인).
    registered: usize,
    registered_visible: usize,
}

fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

// IP 회전 등 상태신호를 adb.rs(AppHandle 없는 곳)에서 에이전트로 보내는 전역 채널.
static STATE_TX: OnceLock<mpsc::UnboundedSender<(String, Option<String>)>> = OnceLock::new();

/// 기존 로그인/회전 흐름에서 호출하는 상태신호(추가 전용). 미등록·미기동이면 no-op.
/// `state`="rotating"|"online", online이면 `ip`=바뀐 공인 IP(§4-1).
pub fn report_state_change(state: &str, ip: Option<String>) {
    if let Some(tx) = STATE_TX.get() {
        let _ = tx.send((state.to_string(), ip));
    }
}

/// 앱 시작 시 호출(setup, 추가 1줄). 명령 수신·하트비트·상태보고 루프를 백그라운드로 띄운다.
pub fn start<R: Runtime>(app: AppHandle<R>) {
    let (tx, rx) = mpsc::unbounded_channel::<(String, Option<String>)>();
    let _ = STATE_TX.set(tx);

    let cmd_app = app.clone();
    let post_app = app.clone();
    let inv_app = app.clone();
    let qs_app = app.clone();
    tauri::async_runtime::spawn(async move { command_loop(cmd_app).await });
    tauri::async_runtime::spawn(async move { heartbeat_loop().await });
    tauri::async_runtime::spawn(async move { state_report_loop(rx).await });
    tauri::async_runtime::spawn(async move { post_report_loop(post_app).await });
    tauri::async_runtime::spawn(async move { log_forward_loop().await });
    tauri::async_runtime::spawn(async move { inventory_report_loop(inv_app).await });
    // 실시간 실행큐 스냅샷 보고(설계서 08 §10-2) → Admin "중지 명령" 페이지.
    tauri::async_runtime::spawn(async move { queue_state_report_loop(qs_app).await });
}

// ───────────────────────── 인벤토리 보고 루프(07-게시명령 3단계) ─────────────────────────

/// 하위 글목록(LibraryPost)·성공(Active)계정을 주기적으로 서버에 보고 → Admin 게시명령 화면이
/// 실데이터로 렌더한다. 서버는 최신 1건만 두고 **바뀌었을 때만** 통신로그에 남긴다(도배 방지).
/// 미등록이면 보고 안 함(단독 동작 무영향).
async fn inventory_report_loop<R: Runtime>(app: AppHandle<R>) {
    let client = reqwest::Client::new();
    loop {
        tokio::time::sleep(Duration::from_secs(15)).await;
        let Some(cfg) = config::load() else {
            continue;
        };
        let posts: Vec<(String, String, &'static str, String, u32)> = app
            .state::<JsonStore<crate::ipc::posts::LibraryPost>>()
            .snapshot()
            .into_iter()
            .map(|p| {
                // 작성한 댓글 수(≥2 게이트용, 15-기타명령 §3). Admin이 이 값으로 닉네임 랜덤
                // 체크박스 노출을 판정한다(N≥2일 때만). 없으면 0.
                let comment_count = p.comments.as_ref().map(|c| c.len()).unwrap_or(0) as u32;
                (p.id, p.title, mode_to_str(&p.kind), p.excerpt, comment_count)
            })
            .collect();
        let accounts = app.state::<JsonStore<Account>>().snapshot();
        let body = inventory_body(&posts, &accounts);
        let _ = net::post_inventory(&client, &cfg.server_url, &cfg.device_token, &body).await;
    }
}

/// 인벤토리 보고 본문(서버 `DeviceInventory` 모양). 글=(id,title) 전부, 계정=성공(Active) loginId만.
/// 순수함수(테스트 대상) — 스토어 스냅샷 투영값을 받아 JSON을 만든다.
/// ModeValue → 인벤토리·명령용 문자열("post"|"comment"|"both"). ModeValue serde(lowercase)와 일치.
fn mode_to_str(m: &ModeValue) -> &'static str {
    match m {
        ModeValue::Post => "post",
        ModeValue::Comment => "comment",
        ModeValue::Both => "both",
    }
}

fn inventory_body(
    posts: &[(String, String, &str, String, u32)],
    accounts: &[Account],
) -> serde_json::Value {
    // 댓글은 제목이 없어(당연) title이 비거나 "제목 없음"이다 → excerpt(작성한 댓글 내용)를 함께
    // 실어 Admin이 제목 대신 내용을 보여주게 한다(글이 제목 보여주는 것과 똑같이).
    // commentCount=작성한 댓글 수(≥2 게이트용, 15-기타명령 §3) — Admin이 닉네임 랜덤 노출을 판정.
    let posts: Vec<serde_json::Value> = posts
        .iter()
        .map(|(id, title, kind, excerpt, comment_count)| {
            serde_json::json!({ "id": id, "title": title, "kind": kind, "excerpt": excerpt, "commentCount": comment_count })
        })
        .collect();
    // 전체 계정(loginId·platform·status) — 카페 게시명령은 로그인 성공/실패 무관 카페 계정을 전부
    // 보여줘야 하므로 상태를 그대로 싣는다(Admin이 platform==naver 전부 노출). 종토는 아래 active만.
    let account_rows: Vec<serde_json::Value> = accounts
        .iter()
        .map(|a| {
            serde_json::json!({
                "loginId": a.login_id,
                "platform": platform_to_str(&a.platform),
                "status": status_to_str(&a.status),
            })
        })
        .collect();
    // 게시 대상 계정 = 로그인 성공(Active)만(종토 화면 기존 동작 유지). 카페는 accountRows를 쓴다.
    let active: Vec<String> = accounts
        .iter()
        .filter(|a| matches!(a.status, AccountStatus::Active))
        .map(|a| a.login_id.clone())
        .collect();
    serde_json::json!({ "posts": posts, "accounts": active, "accountRows": account_rows })
}

// ───────────────────────── 실시간 실행큐 스냅샷 보고 루프(설계서 08 §10-2) ─────────────────────────

/// 하위의 실행/대기 게시큐 스냅샷을 주기적으로 서버에 보고 → Admin "중지 명령" 페이지가 하위 앱
/// 게시큐 화면과 **동일한 내용을 실시간으로** 본다. 변화 없으면 전송 생략(트래픽 절약). 미등록이면
/// 보고 안 함(단독 동작 무영향).
async fn queue_state_report_loop<R: Runtime>(app: AppHandle<R>) {
    let client = reqwest::Client::new();
    let mut last: Option<String> = None;
    loop {
        tokio::time::sleep(Duration::from_secs(2)).await;
        let Some(cfg) = config::load() else {
            continue;
        };
        let items = app.state::<JsonStore<QueueNowItem>>().snapshot();
        let body = queue_state_body(&items);
        let key = body.to_string();
        if last.as_deref() == Some(key.as_str()) {
            continue; // 직전과 동일 → 전송 생략(도배 방지)
        }
        last = Some(key);
        let _ = net::post_queue_state(&client, &cfg.server_url, &cfg.device_token, &body).await;
    }
}

/// 큐 스냅샷 보고 본문(서버 `DeviceQueueState` 모양). 실행/대기 아이템만, 하위 게시큐 화면과
/// 같은 표시값(제목·종류·진행률·대상 계정). 순수함수(테스트 대상).
fn queue_state_body(items: &[QueueNowItem]) -> serde_json::Value {
    let rows: Vec<serde_json::Value> = items
        .iter()
        .filter(|i| matches!(i.state, QueueState::Running | QueueState::Waiting))
        .map(|i| {
            let (done, total) = i.progress.unwrap_or((0, 0));
            let state = match i.state {
                QueueState::Running => "running",
                QueueState::Waiting => "waiting",
                QueueState::Done => "done",
            };
            let (kind, login_ids) = plan_summary(i.plan.as_ref());
            serde_json::json!({
                "id": i.id,
                "title": i.title,
                "kind": kind,
                "state": state,
                "done": done,
                "total": total,
                "loginIds": login_ids,
            })
        })
        .collect();
    serde_json::json!({ "items": rows })
}

/// 큐 아이템의 표시 종류 라벨과 대상 계정 목록. 종토는 계정별 큐라 loginIds가 실질적이고,
/// 그 외(카페/밴드/블로그/클립/로그인)는 라벨만(계정 목록은 표시 우선순위 낮아 생략).
fn plan_summary(plan: Option<&PublishPlan>) -> (&'static str, Vec<String>) {
    let Some(p) = plan else {
        return ("게시", vec![]);
    };
    if !p.forum.is_empty() {
        let mut ids: Vec<String> = p.forum.iter().map(|t| t.account_id.clone()).collect();
        ids.sort();
        ids.dedup();
        ("종토", ids)
    } else if !p.naver.is_empty() {
        ("카페", vec![])
    } else if !p.band.is_empty() {
        ("밴드", vec![])
    } else if !p.blog.is_empty() {
        ("블로그", vec![])
    } else if !p.clip.is_empty() {
        ("클립", vec![])
    } else if p.login.as_ref().is_some_and(|l| !l.is_empty()) {
        ("로그인", vec![])
    } else {
        ("게시", vec![])
    }
}

// ───────────────────────── 로그 전송 루프(#324) ─────────────────────────

/// 앱 tracing 로그(링버퍼)를 주기적으로 꺼내 서버로 올린다 — 서버는 이를 감사로그에 실어 Admin
/// 로그 창(통신로그)에 하위의 실제 로그(네이버 원문 응답 등)를 그대로 보여준다. 미등록(서버주소·
/// 토큰 없음)이면 draining 없이 대기해 링버퍼가 로그를 보존한다(상한까지). 전송 실패는 무음
/// 처리한다 — 실패 로그를 남기면 그 로그가 다시 링버퍼로 들어가 피드백 루프가 되기 때문.
async fn log_forward_loop() {
    let client = reqwest::Client::new();
    loop {
        tokio::time::sleep(Duration::from_secs(2)).await;
        let Some(cfg) = config::load() else {
            continue;
        };
        let lines = crate::logging::drain_agent_logs(200);
        if lines.is_empty() {
            continue;
        }
        let _ = net::post_log(&client, &cfg.server_url, &cfg.device_token, &lines).await;
    }
}

// ───────────────────────── 명령 수신 루프 ─────────────────────────

async fn command_loop<R: Runtime>(app: AppHandle<R>) {
    let client = reqwest::Client::new();
    let mut backoff = 1u64;
    loop {
        let Some(cfg) = config::load() else {
            tokio::time::sleep(Duration::from_secs(5)).await;
            continue;
        };
        match net::open_stream(&client, &cfg.server_url, &cfg.device_token).await {
            Ok(mut resp) => {
                backoff = 1;
                tracing::info!("[AGENT] SSE 연결됨 → {}", cfg.server_url);
                let mut buf = String::new();
                loop {
                    match resp.chunk().await {
                        Ok(Some(bytes)) => {
                            buf.push_str(&String::from_utf8_lossy(&bytes));
                            drain_events(&app, &client, &cfg, &mut buf).await;
                        }
                        Ok(None) => break,
                        Err(e) => {
                            tracing::warn!("[AGENT] 스트림 끊김: {e}");
                            break;
                        }
                    }
                }
            }
            Err(e) => tracing::warn!("[AGENT] 연결 실패: {e}"),
        }
        tokio::time::sleep(Duration::from_secs(backoff)).await;
        backoff = (backoff * 2).min(30);
    }
}

async fn drain_events<R: Runtime>(
    app: &AppHandle<R>,
    client: &reqwest::Client,
    cfg: &AgentConfig,
    buf: &mut String,
) {
    while let Some(nl) = buf.find('\n') {
        let line = buf[..nl].trim().to_string();
        buf.drain(..=nl);
        let Some(data) = line.strip_prefix("data:") else {
            continue;
        };
        let data = data.trim();
        let Ok(cmd) = serde_json::from_str::<Command>(data) else {
            continue;
        };
        let cid = cmd
            .command_id
            .clone()
            .unwrap_or_else(|| format!("c-{}", now_ms()));
        // 동기 디스패치(기존 스토어/큐 호출) → 즉시 응답.
        let (level, msg, followup) = dispatch(app, &cmd);
        let _ = net::post_result(
            client,
            &cfg.server_url,
            &cfg.device_token,
            &cid,
            level,
            &msg,
        )
        .await;
        // 로그인이 걸렸으면 끝날 때까지 지켜보고 §10-4 결과를 같은 commandId로 보고(백그라운드).
        if let Some(f) = followup {
            let (app2, client2, cfg2, cid2) =
                (app.clone(), client.clone(), cfg.clone(), cid.clone());
            tauri::async_runtime::spawn(async move {
                report_login_results(app2, client2, cfg2, cid2, f).await;
            });
        }
        // 닉네임 잔여 조회(15-기타명령 §3·§6-2 실시간): 계정별 `forum_nickname_remaining`을
        // 블로킹으로 조회해 서버로 회신한다(dispatch는 즉시 ack만, 실제 조회는 여기 백그라운드).
        if cmd.kind == "query_nickname_remaining" {
            if let Some(q) = cmd.nickname_query {
                let (client2, cfg2) = (client.clone(), cfg.clone());
                tauri::async_runtime::spawn(async move {
                    report_nickname_remaining(client2, cfg2, q.login_ids).await;
                });
            }
        }
        // 기타 명령(15-기타명령 §2) 실제 실행 — 데스크톱 즉시 실행 엔진을 그대로 호출하고 결과를
        // 서버로 회신(post-report에 종류 태그). 브라우저·ADB 블로킹이라 백그라운드로 돌린다.
        if matches!(
            cmd.kind.as_str(),
            "like_posts" | "dislike_posts" | "boost_view" | "rotate_ip"
        ) {
            let (app2, client2, cfg2) = (app.clone(), client.clone(), cfg.clone());
            let kind = cmd.kind.clone();
            let etc = cmd.etc.clone().unwrap_or_default();
            tauri::async_runtime::spawn(async move {
                run_etc_command(app2, client2, cfg2, kind, etc).await;
            });
        }
        // 블로그 새 글 발행(16-블로그새글) 실제 실행 — 계정 쿠키로 RabbitWrite를 호출하고 결과를
        // post-report로 회신한다. HTTP 블로킹이라 백그라운드로 돌린다(엔진 무손상, ADD ONLY).
        if cmd.kind == "publish_blog_write" {
            if let Some(bw) = cmd.blog_write.clone() {
                let (app2, client2, cfg2) = (app.clone(), client.clone(), cfg.clone());
                tauri::async_runtime::spawn(async move {
                    run_blog_write_command(app2, client2, cfg2, bw).await;
                });
            }
        }
    }
}

/// 닉네임 잔여 횟수 조회+회신(15-기타명령 §3·§6-2 실시간). 각 계정의 저장 쿠키로 프로필 form을
/// GET해 `remainingEditCount`(5회 한도 중 남은 횟수)를 얻어 서버로 올린다. 데스크톱 IPC와 같은
/// 엔진(`naver_automation::forum_nickname_remaining`)을 그대로 호출한다(엔진 무손상). 조회는
/// 블로킹이라 `spawn_blocking`으로 감싸 executor를 막지 않는다. 실패한 계정은 remaining=null.
async fn report_nickname_remaining(
    client: reqwest::Client,
    cfg: AgentConfig,
    login_ids: Vec<String>,
) {
    let mut results: Vec<serde_json::Value> = Vec::new();
    for id in login_ids {
        let id2 = id.clone();
        let remaining: Option<i64> = tokio::task::spawn_blocking(move || {
            crate::naver_automation::forum_nickname_remaining(&id2).ok().flatten()
        })
        .await
        .ok()
        .flatten();
        results.push(serde_json::json!({ "loginId": id, "remaining": remaining }));
    }
    let body = serde_json::json!({ "results": results });
    let _ = net::post_nickname_remaining(&client, &cfg.server_url, &cfg.device_token, &body).await;
}

// ───────────────────────── 기타 명령(15-기타명령 §2) ─────────────────────────

/// 기타 명령의 dispatch 즉시 ack 레벨(실제 결과는 post-report로 뒤따른다).
fn etc_ack_level(kind: &str) -> &'static str {
    match kind {
        "like_posts" | "dislike_posts" | "boost_view" | "rotate_ip" => "info",
        _ => "fail",
    }
}

/// 기타 명령의 결과 보고 종류 태그(§6-3) — 게시 결과 목록에서 게시와 구분해 표시한다.
fn etc_report_kind(kind: &str) -> &'static str {
    match kind {
        "like_posts" => "좋아요",
        "dislike_posts" => "싫어요",
        "boost_view" => "조회수",
        "rotate_ip" => "IP",
        _ => "기타",
    }
}

/// 기타 명령 dispatch 즉시 ack 메시지(통신 로그용) — 무엇을 몇 건 시작하는지 원문.
fn etc_ack_message(kind: &str, etc: &EtcCmd) -> String {
    match kind {
        "like_posts" | "dislike_posts" => format!(
            "{} 실행 시작 — 링크 {}개 × 계정 {}개",
            etc_report_kind(kind),
            etc.links.len(),
            etc.login_ids.len()
        ),
        "boost_view" => format!(
            "조회수 실행 시작 — 링크 {}개 × {}회",
            etc.links.len(),
            etc.repeats
        ),
        "rotate_ip" => "IP 변경 실행 시작 — 이 하위 PC IP 회전".to_string(),
        other => format!("알 수 없는 기타 명령: {other}"),
    }
}

/// 기타 명령 결과 1줄(post-report의 items 모양 = PostItemDto). platform/target/loginId/status/msg.
fn etc_item(
    platform: &str,
    target: &str,
    login_id: &str,
    success: bool,
    msg: &str,
) -> serde_json::Value {
    serde_json::json!({
        "platform": platform,
        "target": target,
        "loginId": login_id,
        "status": if success { "success" } else { "fail" },
        "msg": msg,
    })
}

/// 기타 명령 결과 회신 본문(§6-3) — PostReportReq 모양 + 종류 태그(kind). 서버가 결과 보고에
/// 종류로 구분해 싣고, 통신 로그엔 요약 1줄을 남긴다. items는 PostItemDto 모양.
fn etc_report_body(kind: &str, title: &str, items: Vec<serde_json::Value>) -> serde_json::Value {
    serde_json::json!({
        "id": format!("etc-{}-{}", kind, now_ms()),
        "title": title,
        "at": now_ms() as i64,
        "kind": etc_report_kind(kind),
        "items": items,
    })
}

/// 기타 명령 실행 + 결과 회신(15-기타명령 §2·§6-3). 게시 큐를 타지 않고 데스크톱 즉시 실행 엔진을
/// 그대로 호출한다(엔진 무손상): 좋아요/싫어요=`run_reaction_batch`, 조회수=`view_boost::boost_views`,
/// IP변경=`auth::toggle_airplane_mode`. 각 결과를 PostItemDto 모양으로 모아 종류 태그를 붙여
/// post-report로 회신한다. IP변경에 폰/ADB가 없으면 앱이 죽지 않고 실패 1줄로 보고한다(§6-5).
async fn run_etc_command<R: Runtime>(
    app: AppHandle<R>,
    client: reqwest::Client,
    cfg: AgentConfig,
    kind: String,
    etc: EtcCmd,
) {
    let (title, items): (String, Vec<serde_json::Value>) = match kind.as_str() {
        "like_posts" | "dislike_posts" => {
            let (reaction, label) = if kind == "dislike_posts" {
                ("bad", "싫어요")
            } else {
                ("good", "좋아요")
            };
            let links = etc.links.clone();
            let login_ids = etc.login_ids.clone();
            let title = format!("{label} — 링크 {}개 × 계정 {}개", links.len(), login_ids.len());
            match crate::run_reaction_batch(app, links, login_ids, reaction, label).await {
                Ok(outcomes) => {
                    let items = outcomes
                        .iter()
                        .map(|o| {
                            etc_item("forum", &o.post_url, &o.account_id, o.success, &o.message)
                        })
                        .collect();
                    (title, items)
                }
                Err(e) => (title, vec![etc_item("forum", "-", "-", false, &e)]),
            }
        }
        "boost_view" => {
            let links = etc.links.clone();
            let repeats = etc.repeats.max(1);
            let title = format!("조회수 — 링크 {}개 × {}회", links.len(), repeats);
            let outcomes = tokio::task::spawn_blocking(move || {
                crate::view_boost::boost_views(&links, repeats)
            })
            .await
            .unwrap_or_default();
            let items = outcomes
                .iter()
                .map(|o| {
                    etc_item(
                        "forum",
                        &o.link,
                        "-",
                        o.success,
                        &format!("{}/{}회 · {}", o.completed, o.requested, o.message),
                    )
                })
                .collect();
            (title, items)
        }
        "rotate_ip" => {
            let title = "IP 변경 — 이 하위 PC IP 회전".to_string();
            // 폰/ADB 없으면 assert_adb_device/toggle이 Err → 실패 1줄로 보고(앱은 안 죽음, §6-5).
            let item = match crate::auth::assert_adb_device().await {
                Ok(()) => match crate::auth::toggle_airplane_mode().await {
                    Ok(rot) => {
                        let msg = format!("{} → {}", rot.before, rot.after);
                        etc_item("", "IP", "-", rot.changed, &msg)
                    }
                    Err(e) => etc_item("", "IP", "-", false, &format!("IP 변경 실패: {e}")),
                },
                Err(e) => etc_item("", "IP", "-", false, &format!("IP 변경 실패: {e}")),
            };
            (title, vec![item])
        }
        _ => return,
    };
    let body = etc_report_body(&kind, &title, items);
    let _ = net::post_report(&client, &cfg.server_url, &cfg.device_token, &body).await;
}

// ───────────────────────── 블로그 새 글 발행(16-블로그새글) ─────────────────────────

/// Admin 발행 설정(공개범위 코드·댓글·검색·태그)을 데스크톱 `BlogPublishSettings`로 변환(순수 함수).
/// open_type: 0=전체공개·1=이웃공개·2=서로이웃공개·3=비공개(그 외=전체공개). 나머지 세부 설정은
/// 데스크톱 기본값(카테고리/공감/스크랩 등)을 그대로 쓴다(Admin은 공개범위·댓글·검색·태그만 고른다).
fn blog_write_settings(s: &BlogWriteSettings) -> crate::naver_blog::BlogPublishSettings {
    use crate::naver_blog::{BlogPublishSettings, OpenType};
    let open_type = match s.open_type {
        1 => OpenType::Neighbor,
        2 => OpenType::MutualNeighbor,
        3 => OpenType::Private,
        _ => OpenType::Public,
    };
    BlogPublishSettings {
        open_type,
        comment_yn: s.comment_yn,
        search_yn: s.search_yn,
        tags: s.tags.clone(),
        ..BlogPublishSettings::default()
    }
}

/// 블로그 발행 결과 1줄(post-report items 모양 = PostItemDto). 성공이면 게시글 URL을 posted로 싣는다
/// (Admin '게시 결과'가 링크를 그대로 보여준다). 순수 함수(테스트 대상).
fn blog_write_item(
    login_id: &str,
    blog_id: &str,
    title: &str,
    success: bool,
    msg: &str,
    url: Option<&str>,
) -> serde_json::Value {
    let mut item = serde_json::json!({
        "platform": "blog",
        "target": blog_id,
        "loginId": login_id,
        "status": if success { "success" } else { "fail" },
        "msg": msg,
    });
    if let Some(u) = url {
        item["posted"] = serde_json::json!({ "title": title, "body": "", "url": u });
    }
    item
}

/// 블로그 발행 결과 회신 본문(PostReportReq 모양 + 종류 태그 "게시"). 순수 함수(테스트 대상).
fn blog_write_report_body(title: &str, items: Vec<serde_json::Value>) -> serde_json::Value {
    serde_json::json!({
        "id": format!("blogwrite-{}", now_ms()),
        "title": title,
        "at": now_ms() as i64,
        "kind": "게시",
        "items": items,
    })
}

/// Admin 원격 발행이 보내는 **원본(raw) 미디어 블록** — Admin은 브라우저라 계정 세션이 없어 사진/파일을
/// base64로, 링크를 URL만으로 미해결 상태로 보낸다. 하위 에이전트가 계정 세션으로 업로드/조회해 실제
/// 블록으로 바꾼다. 데스크톱은 이미 해결된 블록을 보내므로 이 타입을 쓰지 않는다.
#[derive(Debug, PartialEq)]
enum BlogMediaInput {
    Image { file_name: String, data_base64: String },
    File { file_name: String, data_base64: String },
    Oglink { link: String },
}

/// 원본 블록 JSON을 분류한다(순수). `imageUpload`/`fileUpload`/`oglinkUrl`만 미디어 입력으로 보고, 그 외
/// (text/code/schedule/sticker/이미 해결된 image 등)는 `None` → 호출부가 **기존대로** Block으로 역직렬화한다.
/// 즉 raw 미디어가 없으면 동작이 지금과 100% 동일하다(기존 경로 무손상).
fn blog_media_input(value: &serde_json::Value) -> Option<BlogMediaInput> {
    let get = |k: &str| value.get(k).and_then(|v| v.as_str()).map(str::to_owned);
    match value.get("type").and_then(|v| v.as_str())? {
        "imageUpload" => Some(BlogMediaInput::Image {
            file_name: get("fileName").unwrap_or_default(),
            data_base64: get("dataBase64")?,
        }),
        "fileUpload" => Some(BlogMediaInput::File {
            file_name: get("fileName").unwrap_or_default(),
            data_base64: get("dataBase64")?,
        }),
        "oglinkUrl" => Some(BlogMediaInput::Oglink { link: get("link")? }),
        _ => None,
    }
}

/// OglinkMeta(JSON)를 oglink 블록 JSON으로 바꾼다(순수). 핵심: OglinkMeta의 `url`(정규화·서명된 URL)을
/// OglinkBlock의 `link`로 옮긴다 — 발행이 통과하려면 oglinkSign이 서명한 그 url이 link여야 한다. 나머지
/// 필드(title/domain/description/thumbnail*/oglinkSign)는 1:1. `type:"oglink"` 태그를 붙인다.
fn remap_oglink_meta_to_block(mut meta_json: serde_json::Value) -> serde_json::Value {
    if let Some(obj) = meta_json.as_object_mut() {
        if let Some(url) = obj.remove("url") {
            obj.insert("link".to_owned(), url);
        }
        obj.insert(
            "type".to_owned(),
            serde_json::Value::String("oglink".to_owned()),
        );
    }
    meta_json
}

/// 원본 블록들을 **이 계정의 세션으로** 해결한다: `imageUpload`/`fileUpload`는 base64를 디코드해 계정
/// 세션으로 업로드하고 실제 image/file 블록으로, `oglinkUrl`은 조회해 oglink 블록으로 바꾼다. 그 외
/// 블록은 그대로 `naver_blog::Block`으로 역직렬화한다(기존 경로 무손상). 하나라도 실패하면
/// 사람이 읽는 사유로 `Err`. Admin 원격 미디어(사진/파일/링크) 배선의 핵심 — 계정마다 자기 블로그로
/// 업로드해야 하므로 대상 계정별로 호출한다(공유 불가).
async fn resolve_blocks_for_account(
    account_id: &str,
    raw_blocks: &[serde_json::Value],
) -> Result<Vec<crate::naver_blog::Block>, String> {
    use base64::Engine;
    let b64 = base64::engine::general_purpose::STANDARD;
    let mut out = Vec::with_capacity(raw_blocks.len());
    for raw in raw_blocks {
        let block_json = match blog_media_input(raw) {
            Some(BlogMediaInput::Image {
                file_name,
                data_base64,
            }) => {
                let bytes = b64
                    .decode(data_base64.trim())
                    .map_err(|e| format!("사진 base64 디코드 실패({file_name}): {e}"))?;
                let img = crate::naver_blog::upload_blog_photo_for_account_bytes(
                    account_id, &file_name, bytes,
                )
                .await
                .map_err(|e| format!("사진 업로드 실패({file_name}): {}", e.message()))?;
                let mut v = serde_json::to_value(&img)
                    .map_err(|e| format!("사진 블록 직렬화 실패: {e}"))?;
                v["type"] = serde_json::Value::String("image".to_owned());
                v
            }
            Some(BlogMediaInput::File {
                file_name,
                data_base64,
            }) => {
                let bytes = b64
                    .decode(data_base64.trim())
                    .map_err(|e| format!("파일 base64 디코드 실패({file_name}): {e}"))?;
                let f = crate::naver_blog::upload_blog_file_for_account_bytes(
                    account_id, &file_name, bytes,
                )
                .await
                .map_err(|e| format!("파일 업로드 실패({file_name}): {}", e.message()))?;
                let mut v =
                    serde_json::to_value(&f).map_err(|e| format!("파일 블록 직렬화 실패: {e}"))?;
                v["type"] = serde_json::Value::String("file".to_owned());
                v
            }
            Some(BlogMediaInput::Oglink { link }) => {
                let meta = crate::naver_blog::fetch_oglink_for_account(account_id, &link)
                    .await
                    .map_err(|e| format!("링크 조회 실패({link}): {}", e.message()))?;
                let meta_json = serde_json::to_value(&meta)
                    .map_err(|e| format!("링크 블록 직렬화 실패: {e}"))?;
                remap_oglink_meta_to_block(meta_json)
            }
            None => raw.clone(),
        };
        let block: crate::naver_blog::Block = serde_json::from_value(block_json)
            .map_err(|e| format!("블록 형식 오류: {e}"))?;
        out.push(block);
    }
    Ok(out)
}

/// 블로그 새 글 발행 실행 + 결과 회신(16-블로그새글). 게시 큐를 타지 않고 계정 쿠키로 RabbitWrite를
/// 직접 호출한다(엔진 무손상): 각 대상 계정마다 `publish_blog_post_blocks_for_account`로 같은 제목/블록/
/// 설정을 자기 블로그에 발행하고, 결과를 PostItemDto로 모아 post-report로 회신한다. blocks가 하나라도
/// 파싱되지 않으면 발행을 시도하지 않고 전 대상 실패로 보고한다(무엇이 잘못됐는지 원문 로그에 남김).
async fn run_blog_write_command<R: Runtime>(
    app: AppHandle<R>,
    client: reqwest::Client,
    cfg: AgentConfig,
    bw: BlogWriteCmd,
) {
    use crate::ipc::activity::{record, ActivityItem, ActivityType};
    use crate::store::JsonStore;
    // 로컬 알림(시스템 탭)에 기록한다 — Admin 원격 발행도 데스크톱 직접 발행과 똑같이 하위 pstmacro
    // 알림에 뜨게(사용자 지적: Admin 명령 발행글은 알림이 안 떴다). 서버 post_report는 그대로 유지(ADD ONLY).
    let activity = app.state::<JsonStore<ActivityItem>>();
    let title = bw.title.clone();
    let settings = blog_write_settings(&bw.settings);
    tracing::info!(
        title = %title,
        targets = bw.targets.len(),
        blocks = bw.blocks.len(),
        "[AGENT] publish_blog_write 수신 — 블로그 새 글 발행 시작(계정×블로그명)"
    );
    let mut items: Vec<serde_json::Value> = Vec::new();
    for t in &bw.targets {
        let blog_id = if t.blog_id.trim().is_empty() {
            t.login_id.clone()
        } else {
            t.blog_id.clone()
        };
        // 원본 블록을 **이 계정 세션으로** 해결한다(Admin 원격 미디어: 사진/파일 업로드, 링크 조회).
        // raw 미디어(imageUpload/fileUpload/oglinkUrl)가 없으면 기존과 동일하게 그대로 Block으로 파싱된다
        // (무손상). 계정마다 자기 블로그에 업로드해야 하므로 대상별로 해결한다.
        let blocks = match resolve_blocks_for_account(&t.login_id, &bw.blocks).await {
            Ok(b) => b,
            Err(e) => {
                tracing::warn!(login_id = %t.login_id, blog_id = %blog_id, "[AGENT] 블로그 블록 해결 실패: {e}");
                record(
                    activity.inner(),
                    ActivityType::Error,
                    format!("블로그 글 발행 실패({blog_id}) — {e}"),
                );
                items.push(blog_write_item(&t.login_id, &blog_id, &title, false, &e, None));
                continue;
            }
        };
        let item = match crate::naver_blog::publish_blog_post_blocks_for_account(
            &t.login_id,
            &blog_id,
            &title,
            &blocks,
            &settings,
        )
        .await
        {
            Ok(res) => {
                let url = res.redirect_url.clone();
                tracing::info!(
                    login_id = %t.login_id, blog_id = %blog_id, url = %url,
                    "[AGENT] 블로그 새 글 발행 성공"
                );
                record(
                    activity.inner(),
                    ActivityType::Success,
                    format!("블로그 글 발행 성공({blog_id}) — {url}"),
                );
                blog_write_item(
                    &t.login_id,
                    &blog_id,
                    &title,
                    true,
                    &format!("발행 성공 · {url}"),
                    Some(&url),
                )
            }
            Err(e) => {
                let msg = e.message().to_owned();
                tracing::warn!(
                    login_id = %t.login_id, blog_id = %blog_id,
                    "[AGENT] 블로그 새 글 발행 실패: {msg}"
                );
                record(
                    activity.inner(),
                    ActivityType::Error,
                    format!("블로그 글 발행 실패({blog_id}) — {msg}"),
                );
                blog_write_item(&t.login_id, &blog_id, &title, false, &msg, None)
            }
        };
        items.push(item);
    }
    let body = blog_write_report_body(&title, items);
    let _ = net::post_report(&client, &cfg.server_url, &cfg.device_token, &body).await;
}

/// 명령 디스패치(동기). 반환: (level, 즉시 메시지, 로그인 결과 후속).
fn dispatch<R: Runtime>(
    app: &AppHandle<R>,
    cmd: &Command,
) -> (&'static str, String, Option<Followup>) {
    match cmd.kind.as_str() {
        "distribute_accounts" => {
            let (added, visible) = add_accounts(app, &cmd.accounts);
            // 카페(naver)·밴드는 분배 시 로그인하지 않고 등록만 한다 — 둘 다 게시 순간 로그인
            // (카페=게시 순간 id/pw, 밴드=게시 순간 band.us 로그인). 그 외(종토·블로그·클립)만
            // 분배 직후 자동 로그인 큐에 태운다. 로그인 엔진은 계정 플랫폼별로 다르다 —
            // 밴드=band.us(Band), 그 외=네이버(Naver). login_platform_from_str로 정한다.
            let logins: Vec<(String, PlatformId)> = cmd
                .accounts
                .iter()
                .filter(|a| !is_cafe_platform(&a.platform) && a.platform != "band")
                .map(|a| (a.login_id.clone(), login_platform_from_str(&a.platform)))
                .collect();
            let cafe_only = cmd.accounts.len().saturating_sub(logins.len());
            let login_ids: Vec<String> = logins.iter().map(|(id, _)| id.clone()).collect();
            let queue_id = enqueue_login(app, &logins);
            (
                "ok",
                format!(
                    "계정 {added}건 등록(로그인 대상 {visible}건) + 자동 로그인 시작 · 카페 등록만 {cafe_only}건"
                ),
                queue_id.map(|q| Followup {
                    queue_id: q,
                    login_ids,
                    registered: added,
                    registered_visible: visible,
                }),
            )
        }
        "import_then_login_all" => {
            let logins = all_logins(app);
            let ids: Vec<String> = logins.iter().map(|(id, _)| id.clone()).collect();
            let n = ids.len();
            let queue_id = enqueue_login(app, &logins);
            (
                "ok",
                format!("전체 로그인 시작 — {n}건"),
                queue_id.map(|q| Followup {
                    queue_id: q,
                    login_ids: ids,
                    registered: 0,
                    registered_visible: 0,
                }),
            )
        }
        "delete_accounts" => {
            let ids: Vec<String> = cmd.accounts.iter().map(|a| a.login_id.clone()).collect();
            let removed = delete_by_login_ids(app, &ids);
            ("info", format!("계정 {removed}건 삭제"), None)
        }
        "publish_posts" => match &cmd.publish {
            Some(p) => enqueue_publish(app, p),
            None => (
                "fail",
                "publish_posts에 publish 페이로드가 없습니다".into(),
                None,
            ),
        },
        "kill_publish" => match &cmd.kill {
            Some(k) => agent_kill(app, k),
            None => (
                "fail",
                "kill_publish에 kill 페이로드가 없습니다".into(),
                None,
            ),
        },
        "update_account_meta" => update_account_meta(app, &cmd.account_updates),
        // 기타 명령(15-기타명령 §2) — 좋아요/싫어요/조회수/IP변경. 게시 큐를 안 타는 즉시 실행이라
        // dispatch는 ack만 남기고, 실제 엔진 호출+결과 회신(post-report에 종류 태그 첨부)은
        // drain_events가 백그라운드로 돌린다(엔진이 브라우저·ADB를 블로킹으로 여닫아 executor를 막지
        // 않도록). §6-3: 결과는 결과 보고(종류 태그)에, 원시 로그는 통신 로그에.
        "like_posts" | "dislike_posts" | "boost_view" | "rotate_ip" => {
            let etc = cmd.etc.clone().unwrap_or_default();
            (etc_ack_level(&cmd.kind), etc_ack_message(&cmd.kind, &etc), None)
        }
        // 닉네임 잔여 조회는 실제 조회+회신을 drain_events가 백그라운드로 돌린다(네트워크 블로킹
        // 회피). 여기선 즉시 ack만 남긴다(15-기타명령 §3·§6-2).
        "query_nickname_remaining" => {
            let n = cmd
                .nickname_query
                .as_ref()
                .map(|q| q.login_ids.len())
                .unwrap_or(0);
            ("info", format!("닉네임 잔여 조회 {n}건 시작"), None)
        }
        // 블로그 새 글 발행(16-블로그새글) — 게시 큐를 타지 않는 직접 발행이라 dispatch는 ack만 남기고,
        // 실제 RabbitWrite 호출 + 결과 회신(post-report)은 drain_events가 백그라운드로 돌린다(HTTP 블로킹).
        "publish_blog_write" => {
            let n = cmd
                .blog_write
                .as_ref()
                .map(|b| b.targets.len())
                .unwrap_or(0);
            ("info", format!("블로그 새 글 발행 {n}건 시작"), None)
        }
        other => ("fail", format!("알 수 없는 명령: {other}"), None),
    }
}

/// 계정 상태/플랫폼 원격 편집(14-계정상태-관리 §4) — Admin이 보낸 update들을 계정 스토어에 반영한다.
/// 코어 `apply_account_meta`(순수)로 갱신하고, **무엇을 어떻게 바꿨는지 원문**(loginId·before→after)을
/// 통신로그에 남긴다(server log-forward로 Admin 통신로그에 그대로 뜬다 — 무필터 로그). 다음
/// inventory_body 보고(≤4초)에 반영돼 Admin 폴링이 왕복을 닫는다.
fn update_account_meta<R: Runtime>(
    app: &AppHandle<R>,
    updates: &[AccountMetaUpdate],
) -> (&'static str, String, Option<Followup>) {
    if updates.is_empty() {
        return ("info", "변경할 계정이 없습니다".into(), None);
    }
    let store = app.state::<JsonStore<Account>>();
    let before = store.snapshot();
    // before→after 원문 로그(무필터). 매칭 loginId만 기록, 유효하지 않은 status/미매칭은 건너뜀 표시.
    let mut lines: Vec<String> = Vec::new();
    for u in updates {
        let Some(cur) = before.iter().find(|a| a.login_id == u.login_id) else {
            lines.push(format!("{}(미매칭·무시)", u.login_id));
            continue;
        };
        let mut parts: Vec<String> = Vec::new();
        if let Some(p) = u.platform.as_deref() {
            parts.push(format!(
                "platform {}→{}",
                platform_to_str(&cur.platform),
                platform_to_str(&platform_from_str(p))
            ));
        }
        if let Some(s) = u.status.as_deref() {
            match status_from_str(s) {
                Some(st) => parts.push(format!(
                    "status {}→{}",
                    status_to_str(&cur.status),
                    status_to_str(&st)
                )),
                None => parts.push(format!("status {s}(비허용·무시)")),
            }
        }
        lines.push(format!("{}: {}", u.login_id, parts.join(", ")));
    }
    store.mutate(|list| apply_account_meta(list, updates));
    let n = updates.len();
    tracing::info!(
        count = n,
        changes = %lines.join(" · "),
        "[AGENT] 계정 상태/플랫폼 원격 편집(원문)"
    );
    (
        "ok",
        format!("계정 {n}건 상태/플랫폼 변경 · {}", lines.join(" · ")),
        None,
    )
}

/// 중지 명령(kill_publish) 처리(설계서 08 §10) — Admin이 보낸 대상(queueId 1개 / all 디바이스
/// 전체 / loginId 계정)을 로컬 실행 중 큐에서 찾아 **로컬 UI와 동일한 kill 경로**(`kill_one`)로
/// 완전 종료한다. 무엇을 왜 중지하는지 **원문 그대로** 로그에 남긴다(server log-forward로 Admin
/// 통신로그에 그대로 뜬다 — Stage5 무필터 로그).
fn agent_kill<R: Runtime>(
    app: &AppHandle<R>,
    k: &KillCmd,
) -> (&'static str, String, Option<Followup>) {
    let snapshot = app.state::<JsonStore<QueueNowItem>>().snapshot();
    let live: Vec<&QueueNowItem> = snapshot
        .iter()
        .filter(|i| matches!(i.state, QueueState::Running | QueueState::Waiting))
        .collect();
    let target_ids: Vec<String> = if k.all {
        live.iter().map(|i| i.id.clone()).collect()
    } else if let Some(qid) = k.queue_id.as_deref() {
        live.iter()
            .filter(|i| i.id == qid)
            .map(|i| i.id.clone())
            .collect()
    } else if let Some(lid) = k.login_id.as_deref() {
        live.iter()
            .filter(|i| item_targets_login(i, lid))
            .map(|i| i.id.clone())
            .collect()
    } else {
        vec![]
    };
    if target_ids.is_empty() {
        tracing::info!(
            all = k.all, queue_id = ?k.queue_id, login_id = ?k.login_id,
            "[AGENT] 중지 명령 수신 — 대상 실행 중 큐 없음(이미 끝났거나 대상 불일치)"
        );
        return ("info", "중지할 실행 중 큐가 없습니다".into(), None);
    }
    tracing::warn!(
        count = target_ids.len(), all = k.all, ids = ?target_ids,
        queue_id = ?k.queue_id, login_id = ?k.login_id,
        "[AGENT] 중지 명령 수신 — 큐 완전 종료 시작(원문)"
    );
    // 결과보고 "중지" 섹션(설계서 §10-3)용 요약을 kill 전에 캡처 — kill_one이 큐에서 지우기 전에
    // 계정별 진행률(done/total)·PW를 확보한다("N개 중 M개 진행 후 중지").
    let accounts = app.state::<JsonStore<Account>>().snapshot();
    let pw_of = |lid: &str| {
        accounts
            .iter()
            .find(|a| a.login_id == lid)
            .map(|a| a.pw.clone())
            .unwrap_or_default()
    };
    let mut stop_lines: Vec<serde_json::Value> = Vec::new();
    for item in live
        .iter()
        .filter(|i| target_ids.iter().any(|t| t == &i.id))
    {
        let (done, total) = item.progress.unwrap_or((0, 0));
        let (_, lids) = plan_summary(item.plan.as_ref());
        let lids = if lids.is_empty() {
            vec![String::new()]
        } else {
            lids
        };
        for lid in lids {
            stop_lines.push(serde_json::json!({
                "loginId": lid,
                "pw": pw_of(&lid),
                "title": item.title,
                "done": done,
                "total": total,
            }));
        }
    }
    for id in &target_ids {
        crate::ipc::kill::kill_one(app, id);
    }
    // 중지 요약을 서버로 비동기 보고(결과보고 렌더용). dispatch는 동기라 spawn한다.
    if !stop_lines.is_empty() {
        tauri::async_runtime::spawn(async move {
            if let Some(cfg) = config::load() {
                let client = reqwest::Client::new();
                let body = serde_json::json!({ "stopped": stop_lines });
                let _ =
                    net::post_stop_report(&client, &cfg.server_url, &cfg.device_token, &body).await;
            }
        });
    }
    // 원문 로그 자기완결(Stage5): "수신 → 각 큐 정지(kill_one 로그) → 완료"가 통신로그·하위
    // 로그에 그대로 남아, Admin에서 하위가 제대로 멈췄는지 원문으로 확인할 수 있다.
    tracing::warn!(
        count = target_ids.len(),
        ids = ?target_ids,
        "[AGENT] 중지 명령 완료 — kill 요청 전송(각 큐 정지·Chrome 정리·다음 큐 승계는 위 [QUEUE]/[POST]/[CHROME] 로그 참조)"
    );
    (
        "ok",
        format!(
            "중지 처리 — {}개 큐 완전 종료(다음 대기 큐 승계)",
            target_ids.len()
        ),
        None,
    )
}

/// 이 큐 아이템이 특정 계정(loginId)을 게시 대상으로 삼는지(종토 계정당 큐 매칭용).
fn item_targets_login(item: &QueueNowItem, login_id: &str) -> bool {
    item.plan
        .as_ref()
        .is_some_and(|p| p.forum.iter().any(|t| t.account_id == login_id))
}

/// 게시 명령(publish_posts) 큐 적재 — 서버가 확정한 계정×종목을 그대로 `ForumTarget`으로 조립해
/// 기존 now 큐에 적재하고 러너를 기동한다(게시 로직 100% 재사용). 무엇을·어느 계정에·어느 종목으로
/// 게시하는지 **원문 전부**를 로그에 남긴다(server log-forward로 통신로그에 그대로 뜬다).
fn enqueue_publish<R: Runtime>(
    app: &AppHandle<R>,
    p: &PublishCmd,
) -> (&'static str, String, Option<Followup>) {
    let detail: String = p
        .assignments
        .iter()
        .map(|a| {
            format!(
                "{}=[{}]",
                a.login_id,
                a.stocks
                    .iter()
                    .map(|s| format!("{}({})", s.name, s.code))
                    .collect::<Vec<_>>()
                    .join(",")
            )
        })
        .collect::<Vec<_>>()
        .join(" · ");
    tracing::info!(
        post_id = %p.post_id,
        title = %p.post_title,
        target = %p.target_label,
        split = p.split,
        mode = %p.mode,
        comment_urls = ?p.comment_urls,
        assignments = %detail,
        "[AGENT] publish_posts 수신 — 게시 큐 적재(계정×종목/댓글URL 원문)"
    );

    let post = load_post(app, &p.post_id);
    let mode = mode_from_str(&p.mode);
    let plan_title = if p.post_title.is_empty() {
        post.title.clone()
    } else {
        p.post_title.clone()
    };
    let effective_title = if post.title.is_empty() {
        plan_title.clone()
    } else {
        post.title.clone()
    };

    // ★ 대원칙: **한 계정 = 한 큐**(사용자 지시·데스크톱 dispatchSplitNow와 동일). 서버가 이미 계정별로
    // 종목을 나눠 보냈으므로(전체=각 계정 전 종목, 나눠서=계정별 분배분), assignment 하나당 QueueNowItem
    // 하나를 만든다. 그러면 워커가 계정별로 **독립·병렬** 실행하고(종토=계정별 격리 Chrome), 5계정이면
    // 5개의 큐가 각자 자기 종목만 올린다 — 한 큐에 전 계정이 몰려 직렬 처리되던 것을 고친다.
    // 대상 플랫폼 분기: 카페(naver)=계정×게시판(plan.naver), 그 외=종토 계정×종목(plan.forum).
    // 어느 쪽이든 큐 러너가 기존 엔진으로 게시(카페는 게시 시점에 id/pw로 로그인).
    let new_items = if p.target == "naver" {
        // 댓글 대상/개수는 Admin 명령값 우선(운영자가 최신/인기/특정글 + 개수를 게시 시점에 고름),
        // 없으면 글에 저장된 값으로 폴백. build 함수는 그대로 재사용(엔진 무변경).
        let (ct, cc) = resolve_comment_spec(
            &p.comment_mode,
            p.comment_count,
            post.comment_target,
            post.comment_count,
        );
        build_cafe_publish_items(
            &p.assignments,
            &p.cafe_boards,
            &p.post_id,
            &plan_title,
            &effective_title,
            &post.body,
            mode,
            &post.comments,
            ct,
            cc,
            now_ms(),
        )
    } else if p.target == "blog" {
        // 블로그는 댓글 전용 — 계정×블로그링크로 plan.blog(BlogTarget)를 조립한다(카페 미러).
        build_blog_publish_items(
            &p.assignments,
            &p.blog_links,
            &p.post_id,
            &plan_title,
            &effective_title,
            &post.body,
            mode,
            &post.comments,
            now_ms(),
        )
    } else if p.target == "clip" {
        // 클립도 댓글 전용 — 계정×클립링크로 plan.clip(ClipTarget) 최신 N개를 조립한다(블로그 미러).
        build_clip_publish_items(
            &p.assignments,
            &p.clip_links,
            &p.post_id,
            &plan_title,
            &effective_title,
            &post.body,
            mode,
            &post.comments,
            now_ms(),
        )
    } else if p.target == "band" {
        // 밴드는 글/댓글/글+댓글 전부 — 계정×밴드로 plan.band(BandTarget)를 조립한다(카페 미러).
        // 댓글 대상 모드/개수는 Admin 명령값 우선(최신/인기/특정글URL + 개수), 없으면 글 저장값 폴백.
        let (ct, cc) = resolve_comment_spec(
            &p.comment_mode,
            p.comment_count,
            post.comment_target,
            post.comment_count,
        );
        build_band_publish_items(
            &p.assignments,
            &p.band_targets,
            &p.post_id,
            &plan_title,
            &effective_title,
            &post.body,
            mode,
            &post.comments,
            ct,
            cc,
            now_ms(),
        )
    } else {
        build_publish_items(
            &p.assignments,
            &p.post_id,
            &plan_title,
            &effective_title,
            &post.body,
            mode,
            &post.comments,
            &p.comment_urls,
            p.forum_comment_distribute,
            p.comment_nickname_random,
            p.content_change.as_ref(),
            now_ms(),
        )
    };
    if new_items.is_empty() {
        return (
            "fail",
            "게시 대상(계정×종목/게시판 또는 댓글 URL)이 비었습니다".into(),
            None,
        );
    }
    let queue_count = new_items.len();
    let total_targets: usize = new_items
        .iter()
        .map(|i| {
            i.plan
                .as_ref()
                .map(|p| p.forum.len() + p.naver.len() + p.blog.len() + p.clip.len() + p.band.len())
                .unwrap_or(0)
        })
        .sum();
    let now = app.state::<JsonStore<QueueNowItem>>();
    now.mutate(move |mut items| {
        for it in new_items {
            items.push(as_fresh_now_item(it));
        }
        apply_priority_order(items)
    });
    let runner = app.state::<NowQueueRunner>();
    start_if_idle(runner.inner(), app.clone());
    (
        "ok",
        format!("게시 큐 {queue_count}개 적재(계정당 1큐, 총 {total_targets}종목)"),
        None,
    )
}

/// **계정당 큐 1개** 규칙으로 게시 큐 아이템 목록을 만든다(순수 함수 — 테스트 대상). assignment
/// 하나당 QueueNowItem 하나이며, 그 계정의 종목만 forum에 담는다. 종목이 빈 계정은 건너뛴다.
/// id는 `agent-publish-{now}-{idx}`로 같은 tick에도 유일(계정 증발 방지, #6).
#[allow(clippy::too_many_arguments)]
fn build_publish_items(
    assignments: &[PublishAssign],
    post_id: &str,
    plan_title: &str,
    effective_title: &str,
    body: &str,
    mode: ModeValue,
    comments: &[String],
    comment_urls: &[String],
    forum_comment_distribute: bool,
    comment_nickname_random: bool,
    content_change: Option<&ContentChange>,
    now: u128,
) -> Vec<QueueNowItem> {
    // 댓글 모드는 종토 "특정 게시글(URL)"만 지원한다(사용자 확정 2026-07-06: 종토 댓글=특정게시글).
    // 계정마다 입력된 URL들에 각각 댓글을 단다(엔진 기존 comment_url 경로 재사용). 글/글+댓글은
    // 기존과 동일하게 계정×종목으로 게시한다(글+댓글은 kind=Both라 글 게시 후 그 글에 댓글까지).
    let is_comment = matches!(mode, ModeValue::Comment);
    let urls: Vec<String> = comment_urls
        .iter()
        .map(|u| u.trim().to_owned())
        .filter(|u| !u.is_empty())
        .collect();
    // 댓글/글+댓글은 저장된 댓글 텍스트를 plan.comments로 실어 forum이 comments.first()로 쓴다.
    let plan_comments: Vec<String> = if matches!(mode, ModeValue::Comment | ModeValue::Both) {
        comments.to_vec()
    } else {
        vec![]
    };

    let mut items = Vec::new();

    // 나눠서 게시(#403): 댓글 모드에서만, 전체 계정×URL을 **단일 QueueNowItem**에 담고
    // forum_comment_distribute=true로 세팅한다. 엔진(plan_to_forum_requests)이 링크마다 댓글을
    // 계정에 1:1 분배하려면 모든 계정이 같은 plan.forum에 있어야 하므로 계정별로 쪼개면 안 된다.
    // 글/글+댓글 모드에선 distribute가 무의미 → 아래 계정별 경로로 폴백(false처럼 동작).
    if is_comment && forum_comment_distribute && !urls.is_empty() {
        let mut forum: Vec<ForumTarget> = Vec::new();
        let mut locs: Vec<QueueLocation> = Vec::new();
        for a in assignments {
            for url in &urls {
                forum.push(ForumTarget {
                    account_id: a.login_id.clone(),
                    name: "특정 게시글".to_string(),
                    code: String::new(),
                    comment_url: url.clone(),
                });
                locs.push(QueueLocation {
                    p: PlatformId::Forum,
                    name: "특정 게시글 댓글".to_string(),
                    code: None,
                });
            }
        }
        if !forum.is_empty() {
            items.push(QueueNowItem {
                id: format!("agent-publish-{now}-0"),
                title: plan_title.to_string(),
                kind: mode.clone(),
                state: QueueState::Waiting,
                batch_id: None,
                progress: None,
                locs,
                plan: Some(PublishPlan {
                    post_id: post_id.to_string(),
                    kind: mode.clone(),
                    title: effective_title.to_string(),
                    body_text: body.to_string(),
                    comments: plan_comments.clone(),
                    link_override: String::new(),
                    naver: vec![],
                    forum,
                    band: vec![],
                    blog: vec![],
                    clip: vec![],
                    login: None,
                    forum_comment_distribute: true,
                    comment_nickname_random,
                    content_change: content_change.cloned(),
                }),
                items: vec![],
            });
        }
        return items;
    }

    for (idx, a) in assignments.iter().enumerate() {
        let mut forum: Vec<ForumTarget> = Vec::new();
        let mut locs: Vec<QueueLocation> = Vec::new();
        if is_comment {
            for url in &urls {
                forum.push(ForumTarget {
                    account_id: a.login_id.clone(),
                    name: "특정 게시글".to_string(),
                    code: String::new(),
                    comment_url: url.clone(),
                });
                locs.push(QueueLocation {
                    p: PlatformId::Forum,
                    name: "특정 게시글 댓글".to_string(),
                    code: None,
                });
            }
        } else {
            if a.stocks.is_empty() {
                continue;
            }
            for s in &a.stocks {
                forum.push(ForumTarget {
                    account_id: a.login_id.clone(),
                    name: s.name.clone(),
                    code: s.code.clone(),
                    comment_url: String::new(),
                });
                locs.push(QueueLocation {
                    p: PlatformId::Forum,
                    name: s.name.clone(),
                    code: Some(s.code.clone()),
                });
            }
        }
        if forum.is_empty() {
            continue;
        }
        items.push(QueueNowItem {
            id: format!("agent-publish-{now}-{idx}"),
            title: plan_title.to_string(),
            kind: mode.clone(),
            state: QueueState::Waiting,
            batch_id: None,
            progress: None,
            locs,
            plan: Some(PublishPlan {
                post_id: post_id.to_string(),
                kind: mode.clone(),
                title: effective_title.to_string(),
                body_text: body.to_string(),
                comments: plan_comments.clone(),
                link_override: String::new(),
                naver: vec![],
                forum,
                band: vec![],
                blog: vec![],
                clip: vec![],
                login: None,
                forum_comment_distribute: false,
                comment_nickname_random,
                content_change: content_change.cloned(),
            }),
            items: vec![],
        });
    }
    items
}

/// 카페 게시판 표시 라벨(완료 로그·큐 표시용). 링크가 있으면 링크, 없으면 "카페 {id}".
fn cafe_label(b: &CafeBoardIn) -> String {
    if b.link.trim().is_empty() {
        format!("카페 {}", b.cafe_id)
    } else {
        b.link.clone()
    }
}

/// 카페 게시 대상 1건(NaverTarget) 조립. board_type은 게시 시점 백엔드가 menu_id로 해석하므로
/// 빈 문자열(데스크톱과 동일 — 쿠키 없이 게시판 목록을 못 받기 때문).
fn cafe_target(
    login_id: &str,
    b: &CafeBoardIn,
    menu_id: u64,
    comment_target: Option<CommentTargetSpec>,
) -> NaverTarget {
    NaverTarget {
        account_id: login_id.to_string(),
        cafe: b.cafe_id.to_string(),
        cafe_name: cafe_label(b),
        menu_id,
        board_type: String::new(),
        comment_target,
    }
}

/// 카페(네이버 카페) 게시 큐 아이템을 만든다(순수 — 테스트 대상). **계정 하나당 큐 1개**(종토와
/// 동일)이며, 그 계정이 선택한 게시판(들)에 글/댓글을 올린다(plan.naver). 종목(assignment.stocks)은
/// 카페에서 쓰지 않는다 — 대상은 게시판 링크다. 댓글 대상/개수는 글(commentTarget/commentCount)에
/// 동결된 값을 그대로 쓴다(데스크톱과 동일). 큐 러너가 게시 시점에 카페 로그인(id/pw)까지 수행한다.
#[allow(clippy::too_many_arguments)]
/// Admin 게시 명령의 댓글 대상 모드 문자열 → `CommentTarget`. 빈값/미인식은 `None`(글 저장값 폴백).
fn parse_comment_target(s: &str) -> Option<CommentTarget> {
    match s {
        "url" => Some(CommentTarget::Url),
        "latest" => Some(CommentTarget::Latest),
        "popular" => Some(CommentTarget::Popular),
        _ => None,
    }
}

/// 카페·밴드 댓글 대상/개수를 결정한다. **Admin 게시 명령의 값이 우선**(운영자가 게시 시점에 고른
/// 최신/인기/특정글 + 개수), 명령이 비어 있으면 글(LibraryPost)에 저장된 값으로 폴백한다(하위호환).
/// 데스크톱은 대상/개수를 글 템플릿에 동결하지만, Admin은 블로그·클립처럼 게시 명령에서 직접 고른다.
fn resolve_comment_spec(
    cmd_mode: &str,
    cmd_count: u32,
    post_target: Option<CommentTarget>,
    post_count: Option<u32>,
) -> (Option<CommentTarget>, Option<u32>) {
    let target = parse_comment_target(cmd_mode).or(post_target);
    let count = if cmd_count > 0 {
        Some(cmd_count)
    } else {
        post_count
    };
    (target, count)
}

fn build_cafe_publish_items(
    assignments: &[PublishAssign],
    cafe_boards: &[CafeBoardIn],
    post_id: &str,
    plan_title: &str,
    effective_title: &str,
    body: &str,
    mode: ModeValue,
    comments: &[String],
    comment_target: Option<CommentTarget>,
    comment_count: Option<u32>,
    now: u128,
) -> Vec<QueueNowItem> {
    if cafe_boards.is_empty() {
        return Vec::new();
    }
    let is_comment = matches!(mode, ModeValue::Comment);
    // 댓글/글+댓글은 저장된 댓글 텍스트를 plan.comments로 실어 엔진이 쓴다(글+댓글=자기 글에 댓글).
    let plan_comments: Vec<String> = if matches!(mode, ModeValue::Comment | ModeValue::Both) {
        comments.to_vec()
    } else {
        vec![]
    };
    // 댓글 대상 모드/개수는 글에 동결된 값(데스크톱 미러). 없으면 최신 1개.
    let ct_mode = comment_target.unwrap_or(CommentTarget::Latest);
    let count = comment_count.unwrap_or(1).max(1);

    let mut items = Vec::new();
    for (idx, a) in assignments.iter().enumerate() {
        let mut naver: Vec<NaverTarget> = Vec::new();
        let mut locs: Vec<QueueLocation> = Vec::new();
        if is_comment {
            // 댓글 전용: 글의 commentTarget으로 대상 해석(url=특정 글 / latest·popular=카페 최신·인기 N).
            match ct_mode {
                CommentTarget::Url => {
                    for b in cafe_boards.iter().filter(|b| b.article_id > 0) {
                        naver.push(cafe_target(
                            &a.login_id,
                            b,
                            0,
                            Some(CommentTargetSpec {
                                mode: CommentTarget::Url,
                                count: None,
                                cafe_id: Some(b.cafe_id),
                                article_id: Some(b.article_id),
                            }),
                        ));
                        locs.push(QueueLocation {
                            p: PlatformId::Naver,
                            name: "카페 특정 글 댓글".to_string(),
                            code: None,
                        });
                    }
                }
                CommentTarget::Latest | CommentTarget::Popular => {
                    // 최신=게시판 단위(링크의 menu_id로 해당 게시판 최신글만), 인기=카페 단위.
                    // 따라서 최신은 (카페,게시판) 조합으로, 인기는 카페 단위로 중복 제거한다.
                    let mut seen = std::collections::HashSet::new();
                    for b in cafe_boards.iter() {
                        let menu = match ct_mode {
                            CommentTarget::Latest => b.menu_id,
                            _ => 0,
                        };
                        if !seen.insert((b.cafe_id, menu)) {
                            continue;
                        }
                        naver.push(cafe_target(
                            &a.login_id,
                            b,
                            menu,
                            Some(CommentTargetSpec {
                                mode: ct_mode.clone(),
                                count: Some(count),
                                cafe_id: Some(b.cafe_id),
                                article_id: None,
                            }),
                        ));
                        locs.push(QueueLocation {
                            p: PlatformId::Naver,
                            name: "카페 최신/인기 댓글".to_string(),
                            code: None,
                        });
                    }
                }
            }
        } else {
            // 글/글+댓글: 게시판(menu_id)에 글을 올린다(both면 자기 글에 댓글까지 엔진이 처리).
            for b in cafe_boards.iter().filter(|b| b.menu_id > 0) {
                naver.push(cafe_target(&a.login_id, b, b.menu_id, None));
                locs.push(QueueLocation {
                    p: PlatformId::Naver,
                    name: cafe_label(b),
                    code: None,
                });
            }
        }
        if naver.is_empty() {
            continue;
        }
        items.push(QueueNowItem {
            id: format!("agent-publish-{now}-{idx}"),
            title: plan_title.to_string(),
            kind: mode.clone(),
            state: QueueState::Waiting,
            batch_id: None,
            progress: None,
            locs,
            plan: Some(PublishPlan {
                post_id: post_id.to_string(),
                kind: mode.clone(),
                title: effective_title.to_string(),
                body_text: body.to_string(),
                comments: plan_comments.clone(),
                link_override: String::new(),
                naver,
                forum: vec![],
                band: vec![],
                blog: vec![],
                clip: vec![],
                // 카페는 분배 때 로그인하지 않으므로(사용자 지침 — 카페만 로그인 없이 분배) 게시
                // 순간에 로그인해 쿠키를 확보해야 한다. 데스크톱 publish-modal(#225)처럼 이 계정의
                // 로그인 스펙을 동봉하면 러너의 prepare_group_login이 게시 직전 [IP회전→로그인→
                // 게시]를 원자 실행한다(카페 10004 회피). 그래야 최신/인기 글목록 조회·댓글이 그
                // 갓 로그인한 쿠키로 동작한다(저장 쿠키가 없어 NO_COOKIES로 죽던 문제 수정).
                login: Some(vec![LoginTarget {
                    account_id: a.login_id.clone(),
                    platform: PlatformId::Naver,
                    headless: false,
                    use_adb: true,
                    force: true,
                }]),
                forum_comment_distribute: false,
                comment_nickname_random: false,
                content_change: None,
            }),
            items: vec![],
        });
    }
    items
}

/// 블로그(네이버 블로그) 댓글 큐 아이템을 만든다(순수 — 테스트 대상). 블로그는 **댓글 전용**이라
/// 카페와 같은 네이버 쿠키를 재사용한다(별도 로그인 없음). **계정 하나당 큐 1개**(카페·종토와 동일)
/// 이며, 그 계정이 고른 블로그 링크(들)에 댓글을 단다(plan.blog=BlogTarget). 종목(assignment.stocks)은
/// 블로그에서 쓰지 않는다. 각 블로그 링크: log_no가 비어있지 않으면 특정 글 1개 댓글(count=None),
/// 비어있으면 최신 N개 댓글(count=Some(count.max(1)), category_no=Some). 댓글 본문은 plan.comments를
/// 그대로 실어 러너(run_blog_targets)가 cafe/band와 동일하게 이어붙여 쓴다.
#[allow(clippy::too_many_arguments)]
fn build_blog_publish_items(
    assignments: &[PublishAssign],
    blog_links: &[BlogLinkIn],
    post_id: &str,
    plan_title: &str,
    effective_title: &str,
    body: &str,
    mode: ModeValue,
    comments: &[String],
    now: u128,
) -> Vec<QueueNowItem> {
    if blog_links.is_empty() {
        return Vec::new();
    }
    let mut items = Vec::new();
    for (idx, a) in assignments.iter().enumerate() {
        let mut blog: Vec<BlogTarget> = Vec::new();
        let mut locs: Vec<QueueLocation> = Vec::new();
        for b in blog_links.iter() {
            let name = if b.blog_id.is_empty() {
                b.link.clone()
            } else {
                b.blog_id.clone()
            };
            // 특정 글(log_no 있음)=count None / 최신 N개(log_no 없음)=count Some(N), category_no Some.
            let (count, category_no) = if b.log_no.is_empty() {
                (Some(b.count.max(1)), Some(b.category_no))
            } else {
                (None, None)
            };
            blog.push(BlogTarget {
                account_id: a.login_id.clone(),
                name: name.clone(),
                blog_id: b.blog_id.clone(),
                log_no: b.log_no.clone(),
                link: b.link.clone(),
                count,
                category_no,
            });
            locs.push(QueueLocation {
                p: PlatformId::Blog,
                name: if b.log_no.is_empty() {
                    format!("블로그 최신글 댓글 · {name}")
                } else {
                    format!("블로그 글 댓글 · {name}")
                },
                code: None,
            });
        }
        if blog.is_empty() {
            continue;
        }
        items.push(QueueNowItem {
            id: format!("agent-publish-{now}-{idx}"),
            title: plan_title.to_string(),
            kind: mode.clone(),
            state: QueueState::Waiting,
            batch_id: None,
            progress: None,
            locs,
            plan: Some(PublishPlan {
                post_id: post_id.to_string(),
                kind: mode.clone(),
                title: effective_title.to_string(),
                body_text: body.to_string(),
                // 블로그는 댓글 전용 — 저장된 댓글 텍스트를 그대로 실어 러너가 쓴다.
                comments: comments.to_vec(),
                link_override: String::new(),
                naver: vec![],
                forum: vec![],
                band: vec![],
                blog,
                clip: vec![],
                login: None,
                forum_comment_distribute: false,
                comment_nickname_random: false,
                content_change: None,
            }),
            items: vec![],
        });
    }
    items
}

/// 클립(네이버 클립) 댓글 큐 아이템을 만든다(순수 — 테스트 대상). 클립은 **댓글 전용**이며 블로그와
/// 같은 네이버 쿠키를 재사용한다(별도 로그인 없음). **계정 하나당 큐 1개**(블로그·카페·종토와 동일)
/// 이며, 그 계정이 고른 창작자(들)의 **최신 N개** 미디어에 댓글을 단다(plan.clip=ClipTarget). 종목
/// (assignment.stocks)은 클립에서 쓰지 않는다. 클립은 최신 N개 단일 모드다(특정 영상·인기 정렬 없음).
/// media_type="video"면 영상만, 그 외/빈값이면 전체(None). 댓글 본문은 plan.comments를 그대로 실어
/// 러너(run_clip_targets)가 cafe/band/blog와 동일하게 이어붙여 쓴다.
#[allow(clippy::too_many_arguments)]
fn build_clip_publish_items(
    assignments: &[PublishAssign],
    clip_links: &[ClipLinkIn],
    post_id: &str,
    plan_title: &str,
    effective_title: &str,
    body: &str,
    mode: ModeValue,
    comments: &[String],
    now: u128,
) -> Vec<QueueNowItem> {
    if clip_links.is_empty() {
        return Vec::new();
    }
    let mut items = Vec::new();
    for (idx, a) in assignments.iter().enumerate() {
        let mut clip: Vec<ClipTarget> = Vec::new();
        let mut locs: Vec<QueueLocation> = Vec::new();
        for c in clip_links.iter() {
            let name = if c.handle.is_empty() {
                c.link.clone()
            } else {
                c.handle.clone()
            };
            // media_type은 "video"만 영상 전용, 그 외/빈값은 전체(None → 엔진 기본=전체).
            let media_type = if c.media_type == "video" {
                Some("video".to_string())
            } else {
                None
            };
            clip.push(ClipTarget {
                account_id: a.login_id.clone(),
                name: name.clone(),
                handle: c.handle.clone(),
                link: c.link.clone(),
                count: Some(c.count.max(1)),
                media_type,
            });
            locs.push(QueueLocation {
                p: PlatformId::Clip,
                name: format!("클립 최신글 댓글 · @{name}"),
                code: None,
            });
        }
        if clip.is_empty() {
            continue;
        }
        items.push(QueueNowItem {
            id: format!("agent-publish-{now}-{idx}"),
            title: plan_title.to_string(),
            kind: mode.clone(),
            state: QueueState::Waiting,
            batch_id: None,
            progress: None,
            locs,
            plan: Some(PublishPlan {
                post_id: post_id.to_string(),
                kind: mode.clone(),
                title: effective_title.to_string(),
                body_text: body.to_string(),
                // 클립은 댓글 전용 — 저장된 댓글 텍스트를 그대로 실어 러너가 쓴다.
                comments: comments.to_vec(),
                link_override: String::new(),
                naver: vec![],
                forum: vec![],
                band: vec![],
                blog: vec![],
                clip,
                login: None,
                forum_comment_distribute: false,
                comment_nickname_random: false,
                content_change: None,
            }),
            items: vec![],
        });
    }
    items
}

/// 밴드(band.us) 게시 큐 아이템을 만든다(순수 — 테스트 대상). 밴드는 **글/댓글/글+댓글 전부** 지원
/// 하며 band.us 쿠키를 쓴다(분배 시 로그인해 확보). **계정 하나당 큐 1개**(카페·종토와 동일)이며,
/// 그 계정이 고른 밴드(들)에 글/댓글을 올린다(plan.band=BandTarget). 종목(assignment.stocks)은 밴드에서
/// 쓰지 않는다. 글/글+댓글=comment_target None(새 글, both면 자기 글에 댓글까지 엔진이). 댓글=글에
/// 동결된 commentTarget(최신/인기/특정글URL)/commentCount를 CommentTargetSpec으로 실어 러너가 처리
/// (url이면 band_comment_on_post가 link의 특정 글에, latest/popular면 band_comment가 최신/인기 N개에).
#[allow(clippy::too_many_arguments)]
fn build_band_publish_items(
    assignments: &[PublishAssign],
    band_targets: &[BandTargetIn],
    post_id: &str,
    plan_title: &str,
    effective_title: &str,
    body: &str,
    mode: ModeValue,
    comments: &[String],
    comment_target: Option<CommentTarget>,
    comment_count: Option<u32>,
    now: u128,
) -> Vec<QueueNowItem> {
    if band_targets.is_empty() {
        return Vec::new();
    }
    let is_comment = matches!(mode, ModeValue::Comment);
    // 댓글/글+댓글은 저장된 댓글 텍스트를 plan.comments로 실어 엔진이 쓴다(글+댓글=자기 글에 댓글).
    let plan_comments: Vec<String> = if matches!(mode, ModeValue::Comment | ModeValue::Both) {
        comments.to_vec()
    } else {
        vec![]
    };
    // 댓글 대상 모드/개수는 글에 동결된 값(카페 미러). 없으면 최신 1개.
    let ct_mode = comment_target.unwrap_or(CommentTarget::Latest);
    let count = comment_count.unwrap_or(1).max(1);

    let mut items = Vec::new();
    for (idx, a) in assignments.iter().enumerate() {
        let mut band: Vec<BandTarget> = Vec::new();
        let mut locs: Vec<QueueLocation> = Vec::new();
        for b in band_targets.iter() {
            let name = if b.band_no.is_empty() {
                b.link.clone()
            } else {
                format!("밴드 {}", b.band_no)
            };
            // 댓글 전용이면 글에 동결된 대상 모드/개수를 실어 러너가 최신/인기/특정글URL로 해석한다.
            // 글/글+댓글이면 comment_target None(새 글 게시).
            let target_spec = if is_comment {
                Some(CommentTargetSpec {
                    mode: ct_mode.clone(),
                    count: Some(count),
                    cafe_id: None,
                    article_id: None,
                })
            } else {
                None
            };
            band.push(BandTarget {
                account_id: a.login_id.clone(),
                name: name.clone(),
                link: b.link.clone(),
                comment_target: target_spec,
            });
            locs.push(QueueLocation {
                p: PlatformId::Band,
                name: if is_comment {
                    format!("밴드 댓글 · {name}")
                } else {
                    format!("밴드 글 · {name}")
                },
                code: None,
            });
        }
        if band.is_empty() {
            continue;
        }
        items.push(QueueNowItem {
            id: format!("agent-publish-{now}-{idx}"),
            title: plan_title.to_string(),
            kind: mode.clone(),
            state: QueueState::Waiting,
            batch_id: None,
            progress: None,
            locs,
            plan: Some(PublishPlan {
                post_id: post_id.to_string(),
                kind: mode.clone(),
                title: effective_title.to_string(),
                body_text: body.to_string(),
                comments: plan_comments.clone(),
                link_override: String::new(),
                naver: vec![],
                forum: vec![],
                band,
                blog: vec![],
                clip: vec![],
                // 밴드도 카페처럼 게시 순간 로그인(옛 코드는 login:None 이라 저장 쿠키가 없으면
                // NO_COOKIES로 죽었다). 이 계정의 밴드 로그인 스펙을 동봉하면 러너의
                // prepare_group_login이 게시 직전 [유효·신선 쿠키면 재로그인 생략 / 아니면 회전→
                // 로그인]으로 쿠키를 확보한다 — 최신 밴드 로직(쿠키재사용·reCAPTCHA 회피)과 일치.
                login: Some(vec![LoginTarget {
                    account_id: a.login_id.clone(),
                    platform: PlatformId::Band,
                    headless: false,
                    use_adb: true,
                    force: true,
                }]),
                forum_comment_distribute: false,
                comment_nickname_random: false,
                content_change: None,
            }),
            items: vec![],
        });
    }
    items
}

/// 로컬 글(LibraryPost) 로드 결과. 카페 댓글은 대상(commentTarget)/개수(commentCount)가 글에
/// 동결돼 있어(데스크톱과 동일) 함께 꺼낸다. 없으면 전부 기본값(best-effort).
#[derive(Default)]
struct LoadedPost {
    title: String,
    body: String,
    comments: Vec<String>,
    comment_target: Option<CommentTarget>,
    comment_count: Option<u32>,
}

/// 로컬 글(LibraryPost) 본문 로드(제목·본문·댓글 + 댓글대상/개수). 없으면 빈 값. 댓글은 댓글/글+댓글
/// 모드에서 forum/naver가 `comments.first()`로 쓴다(엔진 기존 동작 재사용).
fn load_post<R: Runtime>(app: &AppHandle<R>, post_id: &str) -> LoadedPost {
    app.state::<JsonStore<crate::ipc::posts::LibraryPost>>()
        .snapshot()
        .into_iter()
        .find(|p| p.id == post_id)
        .map(|p| LoadedPost {
            title: p.title,
            body: p.body.unwrap_or_default(),
            comments: p.comments.unwrap_or_default(),
            comment_target: p.comment_target,
            comment_count: p.comment_count,
        })
        .unwrap_or_default()
}

/// 게시 명령의 mode 문자열("post"|"comment"|"both", 빈값=post)을 ModeValue로.
fn mode_from_str(s: &str) -> ModeValue {
    match s {
        "comment" => ModeValue::Comment,
        "both" => ModeValue::Both,
        _ => ModeValue::Post,
    }
}

/// 반환: (IPC 스토어에 새로 추가된 수, 그중 로그인 엔진(accounts.json)이 볼 수 있는 수).
fn add_accounts<R: Runtime>(app: &AppHandle<R>, accounts: &[AccountIn]) -> (usize, usize) {
    let store = app.state::<JsonStore<Account>>();
    let mut added = 0usize;
    store.mutate(|mut list| {
        for a in accounts {
            if list.iter().any(|x| x.login_id == a.login_id) {
                continue;
            }
            list.push(Account {
                id: a.login_id.clone(),
                platform: platform_from_str(&a.platform),
                login_id: a.login_id.clone(),
                pw: a.pw.clone(),
                status: AccountStatus::New,
                status_msg: None,
                status_trace: None,
                last: "—".into(),
                tags: vec![],
            });
            added += 1;
        }
        list
    });

    // ⚠️ 로그인 엔진은 위 IPC 스토어가 아니라 *별도 파일* accounts.json(auth::Account)을 읽는다
    // (auth::load_accounts_file). 분배된 계정을 거기에 안 쓰면 자동 로그인이 "account not found"로
    // 떨어지고, 그 실패가 자동삭제까지 이어져 멀쩡한 계정이 파괴된다. 그래서 프론트 save_accounts와
    // 동일하게 같은 계정을 accounts.json에도 기록한다(save_accounts_file이 id로 병합: 기존 갱신·신규 추가).
    let auth_accounts: Vec<crate::auth::Account> = accounts
        .iter()
        .map(|a| crate::auth::Account {
            id: a.login_id.clone(),
            password: a.pw.clone(),
            label: a.login_id.clone(),
        })
        .collect();
    let visible = match crate::auth::save_accounts_file(&auth_accounts) {
        Ok(merged) => {
            // 진단: 방금 등록한 계정이 로그인 엔진이 읽는 파일에서 실제로 보이는지 확인.
            let visible = auth_accounts
                .iter()
                .filter(|a| merged.iter().any(|m| m.id == a.id))
                .count();
            tracing::info!(
                ipc_added = added,
                login_visible = visible,
                total = auth_accounts.len(),
                "[AGENT] 분배 계정 등록 — IPC 스토어 + accounts.json(로그인 엔진) 양쪽 기록"
            );
            visible
        }
        Err(error) => {
            tracing::warn!(
                "[AGENT] accounts.json 기록 실패 — 자동 로그인이 'account not found'로 실패할 수 있음: {error}"
            );
            0
        }
    };

    (added, visible)
}

/// 전체 계정을 (loginId, 로그인엔진 플랫폼)로 반환(재로그인용). 밴드=Band, 그 외=Naver.
fn all_logins<R: Runtime>(app: &AppHandle<R>) -> Vec<(String, PlatformId)> {
    app.state::<JsonStore<Account>>()
        .snapshot()
        .into_iter()
        .map(|a| (a.login_id, login_platform_for(&a.platform)))
        .collect()
}

fn delete_by_login_ids<R: Runtime>(app: &AppHandle<R>, login_ids: &[String]) -> usize {
    let store = app.state::<JsonStore<Account>>();
    let mut removed = 0usize;
    store.mutate(|list| {
        let before = list.len();
        let kept: Vec<Account> = list
            .into_iter()
            .filter(|a| !login_ids.contains(&a.login_id))
            .collect();
        removed = before - kept.len();
        kept
    });
    removed
}

/// 선택 로그인 큐 아이템 1개를 만들어 기존 now 큐에 적재 + 러너 기동. 큐 아이템 id 반환. 각 계정은
/// (loginId, 로그인엔진 플랫폼) 쌍으로 오며, 러너가 LoginTarget.platform으로 네이버/밴드를 분기한다.
fn enqueue_login<R: Runtime>(
    app: &AppHandle<R>,
    logins: &[(String, PlatformId)],
) -> Option<String> {
    if logins.is_empty() {
        return None;
    }
    let login: Vec<LoginTarget> = logins
        .iter()
        .map(|(id, platform)| LoginTarget {
            account_id: id.clone(),
            platform: platform.clone(),
            headless: false,
            use_adb: true,
            force: true,
        })
        .collect();
    let locs: Vec<QueueLocation> = logins
        .iter()
        .map(|(id, _)| QueueLocation {
            p: PlatformId::Forum,
            name: id.clone(),
            code: None,
        })
        .collect();
    let title = format!("계정 로그인 {}건", logins.len());
    let id = format!("agent-login-{}", now_ms());
    let item = QueueNowItem {
        id: id.clone(),
        title: title.clone(),
        kind: ModeValue::Post,
        state: QueueState::Waiting,
        batch_id: None,
        progress: None,
        locs,
        plan: Some(PublishPlan {
            post_id: String::new(),
            kind: ModeValue::Post,
            title,
            body_text: String::new(),
            comments: vec![],
            link_override: String::new(),
            naver: vec![],
            forum: vec![],
            band: vec![],
            blog: vec![],
            clip: vec![],
            login: Some(login),
            forum_comment_distribute: false,
            comment_nickname_random: false,
            content_change: None,
        }),
        items: vec![],
    };
    let now = app.state::<JsonStore<QueueNowItem>>();
    now.mutate(|mut items| {
        items.push(as_fresh_now_item(item));
        apply_priority_order(items)
    });
    let runner = app.state::<NowQueueRunner>();
    start_if_idle(runner.inner(), app.clone());
    Some(id)
}

// ───────────────────────── §10-4 로그인 결과 보고 + 실패 자동삭제 ─────────────────────────

/// 분류 결과.
struct Tally {
    success: usize,
    onhold: Vec<(String, String, String)>, // (loginId, pw, 보류사유)
    timedout: Vec<(String, String)>,       // (loginId, pw)
    // (loginId, pw, 실패사유, trace) — trace는 "자세히 보기"용 백트레이스(없으면 None).
    failed: Vec<(String, String, String, Option<String>)>,
}

/// 큐 아이템이 끝날(Done) 때까지 기다렸다가 계정 상태로 §10-4 분류 → 보고 + 실패 자동삭제 + 누적 갱신.
async fn report_login_results<R: Runtime>(
    app: AppHandle<R>,
    client: reqwest::Client,
    cfg: AgentConfig,
    command_id: String,
    f: Followup,
) {
    // 큐 아이템이 Done 될 때까지 폴링(최대 30분 안전장치). 사라지면(치워짐) 완료로 간주.
    let deadline = std::time::Instant::now() + Duration::from_secs(30 * 60);
    loop {
        let done = {
            let items = app.state::<JsonStore<QueueNowItem>>().snapshot();
            match items.iter().find(|i| i.id == f.queue_id) {
                Some(i) => matches!(i.state, QueueState::Done),
                None => true, // 큐에서 제거됨 → 완료로 봄
            }
        };
        if done || std::time::Instant::now() >= deadline {
            break;
        }
        tokio::time::sleep(Duration::from_secs(5)).await;
    }

    // 완료된 계정 상태를 §10-4 4분류로(동기 스냅샷).
    let tally = classify_accounts(&app, &f.login_ids);
    let received = f.login_ids.len();
    let cum = ledger_add(received, &tally);
    let report = format_report(&tally, received, &cum);
    let _ = net::post_result(
        &client,
        &cfg.server_url,
        &cfg.device_token,
        &command_id,
        "ok",
        &report,
    )
    .await;
    // 구조화 로그인 결과도 보고(§10-4-1) → 결과보고 '로그인 결과' 탭이 실데이터로 렌더.
    let body = login_report_body(
        &command_id,
        &tally,
        &cum,
        f.registered,
        f.registered_visible,
    );
    let _ = net::post_login_report(&client, &cfg.server_url, &cfg.device_token, &body).await;

    // 실패 계정 자동삭제(§10-1 (4)) — 단, *비밀번호 오류(BadCredentials)*처럼 계정 자체가 무효인
    // 경우만 삭제한다. account not found·네트워크·타임아웃·추가인증·차단 같은 일시적·인프라성
    // 실패까지 삭제하면 멀쩡한 계정이 사라진다(사용자 지시 2026-06-30). 삭제 대상 status를 지금
    // 스냅샷에서 다시 확인해 BadCredentials만 고르고, 나머지 실패는 보존하고 로그로 남긴다.
    if !tally.failed.is_empty() {
        let snapshot = app.state::<JsonStore<Account>>().snapshot();
        let failed_ids: Vec<String> = tally
            .failed
            .iter()
            .map(|(id, _, _, _)| id.clone())
            .collect();
        let (delete_ids, retained_ids) = partition_auto_delete(&failed_ids, |id| {
            snapshot
                .iter()
                .find(|a| a.login_id == id)
                .map(|a| a.status.clone())
        });

        if !retained_ids.is_empty() {
            tracing::info!(
                retained = ?retained_ids,
                "[AGENT] 실패했지만 보존 — 일시적·인프라성 실패(비번오류 아님)는 자동삭제하지 않음"
            );
        }
        if !delete_ids.is_empty() {
            let removed = delete_by_login_ids(&app, &delete_ids);
            let del_msg = format!(
                "delete_accounts(계정 삭제) {removed}건(비밀번호 오류만) → {}",
                tally
                    .failed
                    .iter()
                    .filter(|(id, _, _, _)| delete_ids.contains(id))
                    .map(|(id, pw, why, _)| format!("{id}/{pw} (사유: {why})"))
                    .collect::<Vec<_>>()
                    .join(", ")
            );
            let _ = net::post_result(
                &client,
                &cfg.server_url,
                &cfg.device_token,
                &command_id,
                "info",
                &del_msg,
            )
            .await;
        }
    }
}

/// 실패 계정 중 *자동삭제 대상*(비밀번호 오류 = 계정 자체가 무효)과 *보존 대상*(나머지: account
/// not found·네트워크·타임아웃·추가인증·차단 같은 일시적·인프라성 실패)을 가른다. 순수 함수.
/// 반환 = (삭제할 id, 보존할 id). status_of가 None(스토어에 없음)이면 보존한다(account not found
/// 류는 일시적이라 삭제하지 않는다 — 사용자 지시 2026-06-30).
fn partition_auto_delete(
    failed_ids: &[String],
    status_of: impl Fn(&str) -> Option<AccountStatus>,
) -> (Vec<String>, Vec<String>) {
    failed_ids
        .iter()
        .cloned()
        .partition(|id| matches!(status_of(id), Some(AccountStatus::BadCredentials)))
}

fn classify_accounts<R: Runtime>(app: &AppHandle<R>, login_ids: &[String]) -> Tally {
    let snapshot = app.state::<JsonStore<Account>>().snapshot();
    let mut t = Tally {
        success: 0,
        onhold: vec![],
        timedout: vec![],
        failed: vec![],
    };
    for id in login_ids {
        let Some(acct) = snapshot.iter().find(|a| &a.login_id == id) else {
            // 보낸 계정이 스토어에 없음(중복 스킵·삭제 등). 조용히 빼면 "보낸 N개"와 "보고된 N개"가
            // 어긋난다(사용자: 2개 보냈는데 1개만 나옴). 빼지 말고 실패로 명시해 누락 0을 보장한다.
            t.failed.push((
                id.clone(),
                String::new(),
                "계정이 스토어에 없음(중복/삭제 추정)".to_string(),
                None,
            ));
            continue;
        };
        let pw = acct.pw.clone();
        let why = acct.status_msg.clone().unwrap_or_default();
        match acct.status {
            AccountStatus::Active => t.success += 1,
            AccountStatus::OnHold => {
                let reason = if why.is_empty() {
                    "보류".to_string()
                } else {
                    why
                };
                t.onhold.push((id.clone(), pw, reason));
            }
            AccountStatus::TimedOut => t.timedout.push((id.clone(), pw)),
            // 미시도/게시쿨다운 등 로그인 결과 아님 — 삭제·집계 제외.
            AccountStatus::New | AccountStatus::Waiting => {}
            // 비번오류·추가인증·차단·재로그인(세션만료)·에러 = 실패(§10-4). Relogin은 master가
            // 추가한 상태로, 앱 전반(is_problem_status·status_activity_type)에서 실패/오류군으로
            // 묶이므로 여기서도 실패로 본다(자동삭제는 BadCredentials만이라 Relogin은 보존됨).
            AccountStatus::BadCredentials
            | AccountStatus::Challenge
            | AccountStatus::Blocked
            | AccountStatus::Relogin
            | AccountStatus::Error => {
                let reason = if why.is_empty() {
                    format!("{:?}", acct.status)
                } else {
                    why
                };
                t.failed
                    .push((id.clone(), pw, reason, acct.status_trace.clone()));
            }
        }
    }
    t
}

/// §10-4 보고 본문(통신 로그에 ID/PW 평문 — §10-5). 성공은 개수만, 나머지는 ID/PW(+사유).
fn format_report(t: &Tally, received: usize, cum: &Cumulative) -> String {
    let mut s = format!(
        "성공 {} / 보류 {} / 대기초과 {} / 실패 {}",
        t.success,
        t.onhold.len(),
        t.timedout.len(),
        t.failed.len()
    );
    for (id, pw, why) in &t.onhold {
        s.push_str(&format!("\n  보류  {id} / {pw}  사유: {why}"));
    }
    for (id, pw) in &t.timedout {
        s.push_str(&format!("\n  대기초과  {id} / {pw}"));
    }
    for (id, pw, why, _trace) in &t.failed {
        // 통신로그 텍스트엔 사유 한 줄만(백트레이스는 구조화 보고의 trace로 가서 "자세히 보기"에 노출).
        s.push_str(&format!("\n  실패  {id} / {pw}  사유: {why}"));
    }
    s.push_str(&format!(
        "\n총 받은 계정 {} · 성공 {} / 보류 {} / 대기초과 {} / 실패 {}",
        cum.received, cum.success, cum.onhold, cum.timedout, cum.failed
    ));
    let _ = received;
    s
}

/// §10-4-1 구조화 로그인 결과 본문(서버 `LoginReportReq` 모양). 성공은 개수만, 보류/실패는
/// ID/PW+사유, 대기초과는 ID/PW만. 누적 합계 동봉. 순수함수(테스트 대상).
fn login_report_body(
    command_id: &str,
    t: &Tally,
    cum: &Cumulative,
    registered: usize,
    registered_visible: usize,
) -> serde_json::Value {
    let line3 = |v: &[(String, String, String)]| -> Vec<serde_json::Value> {
        v.iter()
            .map(|(id, pw, why)| serde_json::json!({ "loginId": id, "pw": pw, "reason": why }))
            .collect()
    };
    let line2 = |v: &[(String, String)]| -> Vec<serde_json::Value> {
        v.iter()
            .map(|(id, pw)| serde_json::json!({ "loginId": id, "pw": pw }))
            .collect()
    };
    // 실패 줄은 사유(reason) + 백트레이스(trace, 게시 결과와 동일하게 "자세히 보기"용)를 함께 싣는다.
    let failed: Vec<serde_json::Value> = t
        .failed
        .iter()
        .map(|(id, pw, why, trace)| {
            serde_json::json!({ "loginId": id, "pw": pw, "reason": why, "trace": trace })
        })
        .collect();
    serde_json::json!({
        "commandId": command_id,
        "registered": registered,
        "registeredVisible": registered_visible,
        "batch": {
            "success": t.success,
            "onhold": line3(&t.onhold),
            "timedout": line2(&t.timedout),
            "failed": failed,
        },
        "cumulative": {
            "received": cum.received,
            "success": cum.success,
            "onhold": cum.onhold,
            "timedout": cum.timedout,
            "failed": cum.failed,
        }
    })
}

// ── 누적 ledger(§10-4) — 작은 json으로 영속화 ──
#[derive(Default, Serialize, Deserialize, Clone)]
struct Cumulative {
    received: usize,
    success: usize,
    onhold: usize,
    timedout: usize,
    failed: usize,
}

fn ledger_path() -> Option<std::path::PathBuf> {
    crate::auth::app_data_root()
        .ok()
        .map(|r| r.join("agent-ledger.json"))
}

fn ledger_add(received: usize, t: &Tally) -> Cumulative {
    let mut c: Cumulative = ledger_path()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();
    c.received += received;
    c.success += t.success;
    c.onhold += t.onhold.len();
    c.timedout += t.timedout.len();
    c.failed += t.failed.len();
    if let Some(p) = ledger_path() {
        if let Ok(s) = serde_json::to_string_pretty(&c) {
            let _ = std::fs::write(p, s);
        }
    }
    c
}

// ───────────────────────── §10-4-2 게시 결과 보고 루프 ─────────────────────────
//
// 하위는 게시가 끝날 때마다 자기 로컬 게시 완료 로그(`LogBatch`)를 이미 만들어 둔다
// (데스크톱 앱과 동일, `queue_runner.rs::store_log_batch`). 에이전트는 그 스토어를 폴링해
// **아직 안 올린 완료 배치**를 그대로 서버에 보고한다 → Admin '게시 결과' 탭이 같은 모델로 렌더.
// 로그인 결과(§10-4)와 달리 commandId·명령에 묶이지 않는 별도 흐름이다(게시는 로컬 큐가 돌림).

/// 보고 완료한 배치 id(중복 방지). 스토어는 최대 MAX_LOG_BATCHES(500)건만 유지하므로 이 목록은
/// 그보다 넉넉히만 들고 있으면 된다(스토어에서 빠진 배치는 다시 안 보임).
const MAX_REPORTED_IDS: usize = 2000;

fn reported_path() -> Option<std::path::PathBuf> {
    crate::auth::app_data_root()
        .ok()
        .map(|r| r.join("agent-reported-batches.json"))
}

fn load_reported() -> Vec<String> {
    reported_path()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn save_reported(ids: &[String]) {
    if let Some(p) = reported_path() {
        if let Ok(s) = serde_json::to_string(ids) {
            let _ = std::fs::write(p, s);
        }
    }
}

/// 스토어 배치 중 **아직 안 올린 완료 배치**를 오래된 것부터(시간순) 고른다. 스토어는 최신순
/// (insert(0))이라 뒤집고, 진행 중(state=running)은 제외, 이미 보고한 id는 제외(순수함수).
fn unreported_oldest_first(batches: &[LogBatch], reported: &[String]) -> Vec<LogBatch> {
    batches
        .iter()
        .rev()
        .filter(|b| b.state.is_none() && !reported.contains(&b.id))
        .cloned()
        .collect()
}

/// 보고 완료 id를 누적하되 상한(MAX_REPORTED_IDS)을 넘으면 오래된 것부터 버린다(순수함수).
fn push_reported(reported: &mut Vec<String>, id: String) {
    reported.push(id);
    if reported.len() > MAX_REPORTED_IDS {
        let drop = reported.len() - MAX_REPORTED_IDS;
        reported.drain(0..drop);
    }
}

/// 게시 완료 로그(`LogBatch`) 스토어를 폴링해 미보고 완료 배치를 서버에 올린다(§10-4-2).
async fn post_report_loop<R: Runtime>(app: AppHandle<R>) {
    let client = reqwest::Client::new();
    let mut reported: Vec<String> = load_reported();
    loop {
        tokio::time::sleep(Duration::from_secs(5)).await;
        let Some(cfg) = config::load() else {
            continue; // 미등록이면 보고 안 함(단독 동작 무영향)
        };
        let batches = app.state::<JsonStore<LogBatch>>().snapshot();
        for batch in unreported_oldest_first(&batches, &reported) {
            let Ok(body) = serde_json::to_value(&batch) else {
                continue;
            };
            match net::post_report(&client, &cfg.server_url, &cfg.device_token, &body).await {
                Ok(()) => {
                    push_reported(&mut reported, batch.id.clone());
                    save_reported(&reported);
                }
                Err(e) => {
                    // 서버 미연결 등 → 다음 틱에 재시도(보고 안 됨으로 남김).
                    tracing::warn!("[AGENT] 게시 결과 보고 실패(batch={}): {e}", batch.id);
                    break; // 연결 문제면 이번 틱 나머지도 어차피 실패 → 다음 틱에.
                }
            }
        }
    }
}

// ───────────────────────── 하트비트 + 상태 보고 루프 ─────────────────────────

async fn heartbeat_loop() {
    let client = reqwest::Client::new();
    loop {
        if let Some(cfg) = config::load() {
            let ip = crate::auth::fetch_external_ip().await;
            let ip_opt = if ip.starts_with('(') {
                None
            } else {
                Some(ip.as_str())
            };
            let _ = net::heartbeat(
                &client,
                &cfg.server_url,
                &cfg.device_token,
                ip_opt,
                "online",
            )
            .await;
        }
        tokio::time::sleep(Duration::from_secs(30)).await;
    }
}

/// adb.rs가 보낸 상태신호를 서버로 전달(§4). rotating=상태 전이, online=하트비트(바뀐 IP).
async fn state_report_loop(mut rx: mpsc::UnboundedReceiver<(String, Option<String>)>) {
    let client = reqwest::Client::new();
    while let Some((state, ip)) = rx.recv().await {
        let Some(cfg) = config::load() else { continue };
        if state == "online" {
            let _ = net::heartbeat(
                &client,
                &cfg.server_url,
                &cfg.device_token,
                ip.as_deref(),
                "online",
            )
            .await;
        } else {
            let _ = net::post_state(&client, &cfg.server_url, &cfg.device_token, &state).await;
        }
    }
}

// ===================== Tauri 명령(하위 등록 화면 §6-2) =====================

#[tauri::command]
pub async fn agent_register(server_url: String, code: String) -> Result<AgentStatus, String> {
    let base = server_url.trim().trim_end_matches('/').to_string();
    if base.is_empty() || code.trim().is_empty() {
        return Err("서버 주소와 기기코드를 입력하세요".into());
    }
    let client = reqwest::Client::new();
    let resp = net::register(&client, &base, code.trim(), None).await?;
    let device_name = format!(
        "하위-{}",
        resp.device_id.chars().take(4).collect::<String>()
    );
    config::save(&AgentConfig {
        server_url: base.clone(),
        device_token: resp.device_token,
        device_name: device_name.clone(),
    })?;
    Ok(AgentStatus {
        configured: true,
        server_url: base,
        device_name,
    })
}

#[tauri::command]
pub fn agent_status() -> AgentStatus {
    match config::load() {
        Some(c) => AgentStatus {
            configured: true,
            server_url: c.server_url,
            device_name: c.device_name,
        },
        None => AgentStatus {
            configured: false,
            server_url: String::new(),
            device_name: String::new(),
        },
    }
}

#[tauri::command]
pub fn agent_unregister() -> Result<(), String> {
    config::clear()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ipc::log_batches::BatchState;

    fn batch(id: &str, running: bool) -> LogBatch {
        LogBatch {
            id: id.into(),
            title: "게시".into(),
            body: None,
            comment: None,
            kind: ModeValue::Post,
            at: 1,
            state: if running {
                Some(BatchState::Running)
            } else {
                None
            },
            items: vec![],
        }
    }

    #[test]
    fn unreported_skips_running_and_already_reported_oldest_first() {
        // 스토어는 최신순(insert(0)): [c(최신), b, a(오래된)]. b는 진행 중, a는 이미 보고됨.
        let batches = vec![batch("c", false), batch("b", true), batch("a", false)];
        let reported = vec!["a".to_string()];
        let picked: Vec<String> = unreported_oldest_first(&batches, &reported)
            .into_iter()
            .map(|b| b.id)
            .collect();
        // a=보고됨 제외, b=진행 중 제외 → c만, 그리고 오래된 것부터(여기선 c 하나).
        assert_eq!(picked, vec!["c".to_string()]);
    }

    #[test]
    fn unreported_returns_oldest_first_order() {
        // 완료·미보고 둘: 스토어 [y(최신), x(오래된)] → 시간순 [x, y]로 보고해야 함.
        let batches = vec![batch("y", false), batch("x", false)];
        let picked: Vec<String> = unreported_oldest_first(&batches, &[])
            .into_iter()
            .map(|b| b.id)
            .collect();
        assert_eq!(picked, vec!["x".to_string(), "y".to_string()]);
    }

    #[test]
    fn push_reported_caps_at_max_dropping_oldest() {
        let mut reported: Vec<String> = (0..MAX_REPORTED_IDS).map(|i| format!("b{i}")).collect();
        push_reported(&mut reported, "new".into());
        assert_eq!(reported.len(), MAX_REPORTED_IDS);
        assert_eq!(reported.last().unwrap(), "new"); // 새 id는 남고
        assert_eq!(reported.first().unwrap(), "b1"); // 가장 오래된 b0은 밀려남
    }

    #[test]
    fn login_report_body_shapes_batch_and_cumulative() {
        let t = Tally {
            success: 3,
            onhold: vec![("aaa".into(), "pw1".into(), "캡차".into())],
            timedout: vec![("bbb".into(), "pw2".into())],
            failed: vec![(
                "ccc".into(),
                "pw3".into(),
                "연결 실패".into(),
                Some("at x.rs:1:1\n\nframe0".into()),
            )],
        };
        let cum = Cumulative {
            received: 20,
            success: 6,
            onhold: 3,
            timedout: 5,
            failed: 6,
        };
        let v = login_report_body("c-1", &t, &cum, 2, 2);
        assert_eq!(v["commandId"], "c-1");
        assert_eq!(v["batch"]["success"], 3);
        // 보류·실패는 ID/PW+사유, 대기초과는 사유 없음.
        assert_eq!(v["batch"]["onhold"][0]["loginId"], "aaa");
        assert_eq!(v["batch"]["onhold"][0]["reason"], "캡차");
        assert!(v["batch"]["timedout"][0].get("reason").is_none());
        assert_eq!(v["batch"]["failed"][0]["reason"], "연결 실패");
        // 실패 줄은 "자세히 보기"용 trace를 함께 싣는다(게시 결과와 동일).
        assert_eq!(v["batch"]["failed"][0]["trace"], "at x.rs:1:1\n\nframe0");
        // 등록 정보(§10-1 등록 확인)도 동봉.
        assert_eq!(v["registered"], 2);
        assert_eq!(v["registeredVisible"], 2);
        // 누적 합계 동봉.
        assert_eq!(v["cumulative"]["received"], 20);
        assert_eq!(v["cumulative"]["failed"], 6);
    }

    #[test]
    fn build_publish_items_one_queue_per_account() {
        // "나눠서 즉시" 결과처럼 계정별로 종목이 나뉘어 온다(5→여기선 2계정+빈계정). 각 계정당 큐 1개,
        // 그 계정 종목만 담기고, 빈 계정은 큐를 안 만들며, id는 유일해야 한다(계정 증발 방지).
        let assignments = vec![
            PublishAssign {
                login_id: "acc_a".into(),
                stocks: vec![
                    PublishStockIn {
                        code: "005930".into(),
                        name: "삼성전자".into(),
                    },
                    PublishStockIn {
                        code: "000660".into(),
                        name: "SK하이닉스".into(),
                    },
                ],
            },
            PublishAssign {
                login_id: "acc_b".into(),
                stocks: vec![PublishStockIn {
                    code: "035420".into(),
                    name: "NAVER".into(),
                }],
            },
            PublishAssign {
                login_id: "acc_empty".into(),
                stocks: vec![],
            },
        ];
        let items = build_publish_items(
            &assignments,
            "p1",
            "제목",
            "제목",
            "본문",
            ModeValue::Post,
            &[],
            &[],
            false,
            false,
            None,
            1234,
        );

        // 빈 계정 제외 → 큐 2개(계정당 1개).
        assert_eq!(items.len(), 2);
        // id 유일(같은 now라도 idx로 구분).
        assert_eq!(items[0].id, "agent-publish-1234-0");
        assert_eq!(items[1].id, "agent-publish-1234-1");
        // 큐0 = acc_a의 2종목만.
        let f0 = &items[0].plan.as_ref().unwrap().forum;
        assert_eq!(f0.len(), 2);
        assert!(f0.iter().all(|t| t.account_id == "acc_a"));
        // 큐1 = acc_b의 1종목만(계정끼리 안 섞임).
        let f1 = &items[1].plan.as_ref().unwrap().forum;
        assert_eq!(f1.len(), 1);
        assert_eq!(f1[0].account_id, "acc_b");
        assert_eq!(f1[0].code, "035420");
        // 게시 전용(로그인 잡 아님).
        assert!(items[0].plan.as_ref().unwrap().login.is_none());
    }

    #[test]
    fn build_publish_items_comment_distribute_makes_single_item() {
        // 나눠서 게시(#403): 댓글 모드 + distribute=true면 계정별로 쪼개지 말고 **단일 큐**에
        // 전체 계정×URL을 담고 forum_comment_distribute=true여야 한다(엔진이 링크마다 분배).
        let assignments = vec![
            PublishAssign {
                login_id: "acc_a".into(),
                stocks: vec![],
            },
            PublishAssign {
                login_id: "acc_b".into(),
                stocks: vec![],
            },
        ];
        let items = build_publish_items(
            &assignments,
            "p1",
            "제목",
            "제목",
            "본문",
            ModeValue::Comment,
            &["댓글1".into(), "댓글2".into()],
            &["https://u/1".into()],
            true,
            false,
            None,
            1234,
        );

        // 계정이 2개여도 큐는 1개(단일 아이템).
        assert_eq!(items.len(), 1);
        let plan = items[0].plan.as_ref().unwrap();
        // 엔진 분배 스위치 ON.
        assert!(plan.forum_comment_distribute);
        // forum = 전체 계정(2) × URL(1) = 2건, 두 계정이 같은 plan에 함께 있다.
        assert_eq!(plan.forum.len(), 2);
        let accts: std::collections::BTreeSet<_> =
            plan.forum.iter().map(|f| f.account_id.as_str()).collect();
        assert_eq!(accts.len(), 2);
        assert!(accts.contains("acc_a") && accts.contains("acc_b"));
        // 저장된 댓글 풀이 plan.comments로 실린다(엔진이 계정에 1개씩 분배).
        assert_eq!(
            plan.comments,
            vec!["댓글1".to_string(), "댓글2".to_string()]
        );
    }

    #[test]
    fn build_publish_items_honors_nickname_random_and_content_change() {
        // 닉네임 랜덤·게시 후 내용변경은 payload에서 온 값을 그대로 plan에 실어야 한다(하드코딩 해제,
        // 15-기타명령 §3·§4). 엔진(회전·edit)은 무손상 — 여기선 plan에 값이 흐르는지만 검증한다.
        let assignments = vec![PublishAssign {
            login_id: "acc_a".into(),
            stocks: vec![PublishStockIn {
                code: "005930".into(),
                name: "삼성전자".into(),
            }],
        }];
        let cc = ContentChange {
            title: "새 제목".into(),
            body: "새 본문".into(),
            delay_sec: 30,
        };
        let items = build_publish_items(
            &assignments,
            "p1",
            "제목",
            "제목",
            "본문",
            ModeValue::Post,
            &[],
            &[],
            false,
            true,        // comment_nickname_random
            Some(&cc),   // content_change
            1234,
        );
        assert_eq!(items.len(), 1);
        let plan = items[0].plan.as_ref().unwrap();
        assert!(plan.comment_nickname_random, "닉네임 랜덤 flag가 실려야 한다");
        assert_eq!(
            plan.content_change.as_ref().map(|c| (c.title.as_str(), c.body.as_str(), c.delay_sec)),
            Some(("새 제목", "새 본문", 30)),
            "내용변경 값이 그대로 실려야 한다"
        );
    }

    #[test]
    fn build_publish_items_defaults_leave_features_off() {
        // 기본(payload 미지정): 닉네임 랜덤 off·내용변경 None(하위호환).
        let assignments = vec![PublishAssign {
            login_id: "acc_a".into(),
            stocks: vec![PublishStockIn {
                code: "005930".into(),
                name: "삼성전자".into(),
            }],
        }];
        let items = build_publish_items(
            &assignments, "p1", "제목", "제목", "본문", ModeValue::Post, &[], &[], false, false,
            None, 1234,
        );
        let plan = items[0].plan.as_ref().unwrap();
        assert!(!plan.comment_nickname_random);
        assert!(plan.content_change.is_none());
    }

    #[test]
    fn build_cafe_publish_items_post_maps_accounts_to_boards() {
        // 카페 글: 계정당 큐 1개, 그 계정이 선택 게시판(들)에 글을 올린다(plan.naver, forum은 빔).
        let assignments = vec![
            PublishAssign {
                login_id: "acc_a".into(),
                stocks: vec![],
            },
            PublishAssign {
                login_id: "acc_b".into(),
                stocks: vec![],
            },
        ];
        let boards = vec![
            CafeBoardIn {
                cafe_id: 100,
                menu_id: 5,
                article_id: 0,
                link: "L1".into(),
            },
            CafeBoardIn {
                cafe_id: 200,
                menu_id: 7,
                article_id: 0,
                link: "L2".into(),
            },
        ];
        let items = build_cafe_publish_items(
            &assignments,
            &boards,
            "p1",
            "제목",
            "제목",
            "본문",
            ModeValue::Post,
            &[],
            None,
            None,
            1234,
        );
        assert_eq!(items.len(), 2);
        let n0 = &items[0].plan.as_ref().unwrap().naver;
        assert_eq!(n0.len(), 2, "acc_a × 게시판 2개");
        assert!(n0.iter().all(|t| t.account_id == "acc_a"));
        assert_eq!(n0[0].cafe, "100");
        assert_eq!(n0[0].menu_id, 5);
        assert!(n0[0].comment_target.is_none());
        // 카페 경로라 forum은 비어야 한다.
        assert!(items[0].plan.as_ref().unwrap().forum.is_empty());
    }

    #[test]
    fn build_cafe_publish_items_comment_uses_frozen_latest_target() {
        // 카페 댓글(글에 동결된 대상=Latest·개수=3): 최신은 게시판 단위 →
        // 같은 카페라도 게시판(menu_id)이 다르면 각각 대상이 된다.
        let assignments = vec![PublishAssign {
            login_id: "acc_a".into(),
            stocks: vec![],
        }];
        let boards = vec![
            CafeBoardIn {
                cafe_id: 100,
                menu_id: 5,
                article_id: 0,
                link: "L1".into(),
            },
            CafeBoardIn {
                cafe_id: 100,
                menu_id: 6,
                article_id: 0,
                link: "L2".into(),
            },
        ];
        let items = build_cafe_publish_items(
            &assignments,
            &boards,
            "p1",
            "제목",
            "제목",
            "",
            ModeValue::Comment,
            &["댓글1".to_string()],
            Some(CommentTarget::Latest),
            Some(3),
            9,
        );
        assert_eq!(items.len(), 1);
        let n = &items[0].plan.as_ref().unwrap().naver;
        assert_eq!(n.len(), 2, "게시판이 다르면 최신은 게시판별로 대상 생성");
        for t in n {
            let ct = t.comment_target.as_ref().unwrap();
            assert!(matches!(ct.mode, CommentTarget::Latest));
            assert_eq!(ct.count, Some(3));
            assert_eq!(ct.cafe_id, Some(100));
        }
        // 대상은 게시판(menu_id) 단위로 최신글을 가져오도록 menu_id가 각각 실려야 한다.
        let mut menus: Vec<u64> = n.iter().map(|t| t.menu_id).collect();
        menus.sort_unstable();
        assert_eq!(menus, vec![5, 6], "각 게시판의 menu_id가 대상에 실려야 함");
        assert_eq!(
            items[0].plan.as_ref().unwrap().comments,
            vec!["댓글1".to_string()]
        );
    }

    #[test]
    fn build_cafe_publish_items_comment_url_uses_article() {
        // 카페 댓글(글에 동결된 대상=Url): 글 링크(article_id)로 그 글에 직접 댓글.
        let assignments = vec![PublishAssign {
            login_id: "acc_a".into(),
            stocks: vec![],
        }];
        let boards = vec![CafeBoardIn {
            cafe_id: 100,
            menu_id: 0,
            article_id: 555,
            link: "A1".into(),
        }];
        let items = build_cafe_publish_items(
            &assignments,
            &boards,
            "p1",
            "제목",
            "제목",
            "",
            ModeValue::Comment,
            &["c".to_string()],
            Some(CommentTarget::Url),
            None,
            9,
        );
        let n = &items[0].plan.as_ref().unwrap().naver;
        assert_eq!(n.len(), 1);
        let ct = n[0].comment_target.as_ref().unwrap();
        assert!(matches!(ct.mode, CommentTarget::Url));
        assert_eq!(ct.article_id, Some(555));
        assert_eq!(ct.cafe_id, Some(100));
    }

    #[test]
    fn build_cafe_publish_items_attaches_publish_time_login() {
        // 카페는 분배 때 로그인 안 함 → 게시 순간 로그인이 필요. plan.login에 그 계정의 네이버
        // 로그인 스펙(force+use_adb)이 동봉돼야 러너 prepare_group_login이 로그인→쿠키 확보→
        // 최신/인기 글목록 조회가 된다(NO_COOKIES 회귀 방지, 데스크톱 #225 미러).
        let assignments = vec![PublishAssign {
            login_id: "acc_a".into(),
            stocks: vec![],
        }];
        let boards = vec![CafeBoardIn {
            cafe_id: 100,
            menu_id: 0,
            article_id: 0,
            link: "https://cafe.naver.com/f-e/cafes/100".into(),
        }];
        let items = build_cafe_publish_items(
            &assignments,
            &boards,
            "p1",
            "제목",
            "제목",
            "",
            ModeValue::Comment,
            &["c".to_string()],
            Some(CommentTarget::Latest),
            Some(20),
            9,
        );
        let plan = items[0].plan.as_ref().unwrap();
        let login = plan
            .login
            .as_ref()
            .expect("카페 plan은 게시 순간 로그인 동봉");
        assert_eq!(login.len(), 1);
        assert_eq!(login[0].account_id, "acc_a");
        assert!(matches!(login[0].platform, PlatformId::Naver));
        assert!(login[0].force, "force 재로그인이어야 쿠키를 새로 확보");
        assert!(login[0].use_adb, "게시 IP=로그인 IP(카페 10004 회피)");
    }

    #[test]
    fn build_blog_publish_items_one_queue_per_account() {
        // 블로그 댓글: 계정당 큐 1개, 그 계정이 고른 블로그 링크(들)에 댓글을 단다(plan.blog).
        // 특정 글(logNo 있음)=count None, 최신 N개(logNo 없음)=count Some·category_no Some.
        let assignments = vec![
            PublishAssign {
                login_id: "acc_a".into(),
                stocks: vec![],
            },
            PublishAssign {
                login_id: "acc_b".into(),
                stocks: vec![],
            },
        ];
        let links = vec![
            // 특정 글: log_no 있음 → count None.
            BlogLinkIn {
                blog_id: "press02".into(),
                log_no: "224311392458".into(),
                category_no: 0,
                count: 0,
                link: "https://blog.naver.com/press02/224311392458".into(),
            },
            // 최신 N개: log_no 없음 → count Some(N), category_no Some.
            BlogLinkIn {
                blog_id: "cho41004".into(),
                log_no: String::new(),
                category_no: 7,
                count: 5,
                link: "https://blog.naver.com/cho41004?categoryNo=7".into(),
            },
        ];
        let items = build_blog_publish_items(
            &assignments,
            &links,
            "p1",
            "제목",
            "제목",
            "",
            ModeValue::Comment,
            &["댓글1".to_string()],
            1234,
        );
        // 계정당 큐 1개(빈 계정 없음).
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].id, "agent-publish-1234-0");
        assert_eq!(items[1].id, "agent-publish-1234-1");
        // 큐0 = acc_a의 블로그 대상 2건, 계정끼리 안 섞임.
        let b0 = &items[0].plan.as_ref().unwrap().blog;
        assert_eq!(b0.len(), 2);
        assert!(b0.iter().all(|t| t.account_id == "acc_a"));
        // 특정 글: log_no 채워지고 count None.
        assert_eq!(b0[0].blog_id, "press02");
        assert_eq!(b0[0].log_no, "224311392458");
        assert_eq!(b0[0].count, None);
        assert_eq!(b0[0].category_no, None);
        // 최신 N개: log_no 빈값, count Some(5), category_no Some(7).
        assert_eq!(b0[1].blog_id, "cho41004");
        assert!(b0[1].log_no.is_empty());
        assert_eq!(b0[1].count, Some(5));
        assert_eq!(b0[1].category_no, Some(7));
        // 댓글 본문은 그대로 실린다(블로그=댓글 전용).
        assert_eq!(
            items[0].plan.as_ref().unwrap().comments,
            vec!["댓글1".to_string()]
        );
        // 블로그 경로라 forum/naver는 비어야 한다.
        assert!(items[0].plan.as_ref().unwrap().forum.is_empty());
        assert!(items[0].plan.as_ref().unwrap().naver.is_empty());
        // 게시 전용(로그인 잡 아님).
        assert!(items[0].plan.as_ref().unwrap().login.is_none());
        // count=0 입력은 최신 1개로 방어(max(1)).
        let items2 = build_blog_publish_items(
            &[PublishAssign {
                login_id: "acc_a".into(),
                stocks: vec![],
            }],
            &[BlogLinkIn {
                blog_id: "x".into(),
                log_no: String::new(),
                category_no: 0,
                count: 0,
                link: "https://blog.naver.com/x".into(),
            }],
            "p1",
            "제목",
            "제목",
            "",
            ModeValue::Comment,
            &[],
            9,
        );
        assert_eq!(items2[0].plan.as_ref().unwrap().blog[0].count, Some(1));
    }

    #[test]
    fn build_clip_publish_items_one_queue_per_account_latest_only() {
        // 클립 댓글: 계정당 큐 1개, 그 계정이 고른 창작자(들)의 최신 N개에 댓글(plan.clip). 전체/영상.
        let assignments = vec![
            PublishAssign {
                login_id: "acc_a".into(),
                stocks: vec![],
            },
            PublishAssign {
                login_id: "acc_b".into(),
                stocks: vec![],
            },
        ];
        let links = vec![
            ClipLinkIn {
                handle: "dongzzi_chef".into(),
                media_type: String::new(), // 전체
                count: 5,
                link: "https://clip.naver.com/@dongzzi_chef".into(),
            },
            ClipLinkIn {
                handle: "mugidaebackgwa".into(),
                media_type: "video".into(), // 영상만
                count: 0,                   // 방어 → 최신 1개
                link: "https://clip.naver.com/@mugidaebackgwa?tab=video".into(),
            },
        ];
        let items = build_clip_publish_items(
            &assignments,
            &links,
            "p1",
            "제목",
            "제목",
            "",
            ModeValue::Comment,
            &["댓글1".to_string()],
            1234,
        );
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].id, "agent-publish-1234-0");
        let c0 = &items[0].plan.as_ref().unwrap().clip;
        assert_eq!(c0.len(), 2);
        assert!(c0.iter().all(|t| t.account_id == "acc_a"));
        // 전체=media_type None, count Some(5).
        assert_eq!(c0[0].handle, "dongzzi_chef");
        assert_eq!(c0[0].media_type, None);
        assert_eq!(c0[0].count, Some(5));
        // 영상만=media_type Some("video"), count=0 방어 → Some(1).
        assert_eq!(c0[1].media_type, Some("video".to_string()));
        assert_eq!(c0[1].count, Some(1));
        // 댓글 본문 실림, 클립 경로라 forum/naver/blog/band 빈다.
        assert_eq!(
            items[0].plan.as_ref().unwrap().comments,
            vec!["댓글1".to_string()]
        );
        let p0 = items[0].plan.as_ref().unwrap();
        assert!(
            p0.forum.is_empty() && p0.naver.is_empty() && p0.blog.is_empty() && p0.band.is_empty()
        );
        assert!(p0.login.is_none());
    }

    #[test]
    fn build_band_publish_items_post_none_target_and_comment_frozen_target() {
        let assignments = vec![
            PublishAssign {
                login_id: "acc_a".into(),
                stocks: vec![],
            },
            PublishAssign {
                login_id: "acc_b".into(),
                stocks: vec![],
            },
        ];
        let bands = vec![
            BandTargetIn {
                band_no: "103043410".into(),
                link: "https://band.us/band/103043410".into(),
            },
            BandTargetIn {
                band_no: String::new(),
                link: "https://band.us/band/200/post/9".into(),
            },
        ];
        // 글 모드: comment_target None(새 글 게시), 계정당 큐 1개.
        let posts = build_band_publish_items(
            &assignments,
            &bands,
            "p1",
            "제목",
            "제목",
            "본문",
            ModeValue::Post,
            &[],
            None,
            None,
            1,
        );
        assert_eq!(posts.len(), 2);
        let pb = &posts[0].plan.as_ref().unwrap().band;
        assert_eq!(pb.len(), 2);
        assert!(pb.iter().all(|t| t.comment_target.is_none()));
        assert_eq!(pb[0].link, "https://band.us/band/103043410");
        // 댓글 모드: 글에 동결된 대상(인기 3개)을 CommentTargetSpec으로 실어 보낸다.
        let comments = build_band_publish_items(
            &assignments,
            &bands,
            "p1",
            "제목",
            "제목",
            "",
            ModeValue::Comment,
            &["댓글1".to_string()],
            Some(CommentTarget::Popular),
            Some(3),
            2,
        );
        let cb = &comments[0].plan.as_ref().unwrap().band;
        let spec = cb[0]
            .comment_target
            .as_ref()
            .expect("댓글 모드는 대상 있음");
        assert_eq!(spec.mode, CommentTarget::Popular);
        assert_eq!(spec.count, Some(3));
        assert_eq!(
            comments[0].plan.as_ref().unwrap().comments,
            vec!["댓글1".to_string()]
        );
    }

    #[test]
    fn parse_comment_target_maps_admin_modes() {
        assert_eq!(parse_comment_target("url"), Some(CommentTarget::Url));
        assert_eq!(parse_comment_target("latest"), Some(CommentTarget::Latest));
        assert_eq!(
            parse_comment_target("popular"),
            Some(CommentTarget::Popular)
        );
        // 빈값·미인식은 None(글 저장값으로 폴백).
        assert_eq!(parse_comment_target(""), None);
        assert_eq!(parse_comment_target("bogus"), None);
    }

    #[test]
    fn resolve_comment_spec_prefers_command_over_post() {
        // 명령이 인기 5개를 고르면 글 저장값(최신 1)을 무시하고 명령값을 쓴다.
        let (t, c) = resolve_comment_spec("popular", 5, Some(CommentTarget::Latest), Some(1));
        assert_eq!(t, Some(CommentTarget::Popular));
        assert_eq!(c, Some(5));
    }

    #[test]
    fn resolve_comment_spec_falls_back_to_post_when_command_empty() {
        // 명령이 비면(빈 모드·개수0) 글에 저장된 값으로 폴백(하위호환).
        let (t, c) = resolve_comment_spec("", 0, Some(CommentTarget::Popular), Some(3));
        assert_eq!(t, Some(CommentTarget::Popular));
        assert_eq!(c, Some(3));
    }

    #[test]
    fn resolve_comment_spec_partial_command_override() {
        // 모드만 명령(url), 개수는 명령 없음(0) → 개수는 글값 폴백.
        let (t, c) = resolve_comment_spec("url", 0, Some(CommentTarget::Latest), Some(2));
        assert_eq!(t, Some(CommentTarget::Url));
        assert_eq!(c, Some(2));
        // 둘 다 없으면 둘 다 None(build 함수가 최신 1개로 기본 처리).
        let (t2, c2) = resolve_comment_spec("", 0, None, None);
        assert_eq!(t2, None);
        assert_eq!(c2, None);
    }

    #[test]
    fn login_platform_maps_band_only() {
        assert_eq!(login_platform_from_str("band"), PlatformId::Band);
        assert_eq!(login_platform_from_str("clip"), PlatformId::Naver);
        assert_eq!(login_platform_from_str("blog"), PlatformId::Naver);
        assert_eq!(login_platform_from_str("forum"), PlatformId::Naver);
        assert_eq!(login_platform_for(&PlatformId::Band), PlatformId::Band);
        assert_eq!(login_platform_for(&PlatformId::Clip), PlatformId::Naver);
    }

    #[test]
    fn inventory_body_lists_posts_and_only_active_accounts() {
        let posts = vec![
            (
                "p1".to_string(),
                "급등주 분석".to_string(),
                "post",
                "외국인 순매수 유입".to_string(),
                0u32,
            ),
            (
                "p2".to_string(),
                "제목 없음".to_string(),
                "comment",
                "오늘 흐름 좋네요 👍".to_string(),
                3u32,
            ),
        ];
        let acct = |login: &str, status: AccountStatus| Account {
            id: login.into(),
            platform: PlatformId::Forum,
            login_id: login.into(),
            pw: "pw".into(),
            status,
            status_msg: None,
            status_trace: None,
            last: "—".into(),
            tags: vec![],
        };
        let accounts = vec![
            acct("ok_a", AccountStatus::Active),
            acct("blocked_b", AccountStatus::Blocked),
            acct("new_c", AccountStatus::New),
            acct("ok_d", AccountStatus::Active),
        ];
        let v = inventory_body(&posts, &accounts);
        // 글은 id/title/kind/excerpt 전부. 댓글은 제목 대신 내용을 보여주도록 excerpt를 싣는다.
        assert_eq!(v["posts"].as_array().unwrap().len(), 2);
        assert_eq!(v["posts"][0]["id"], "p1");
        assert_eq!(v["posts"][1]["title"], "제목 없음");
        assert_eq!(v["posts"][1]["excerpt"], "오늘 흐름 좋네요 👍");
        // commentCount(≥2 게이트용) — 작성한 댓글 수를 그대로 싣는다(글=0, 댓글글=3).
        assert_eq!(v["posts"][0]["commentCount"], 0);
        assert_eq!(v["posts"][1]["commentCount"], 3);
        // 계정은 성공(Active)만 — 차단/신규는 빠진다.
        let accts: Vec<&str> = v["accounts"]
            .as_array()
            .unwrap()
            .iter()
            .map(|a| a.as_str().unwrap())
            .collect();
        assert_eq!(accts, vec!["ok_a", "ok_d"]);
    }

    #[test]
    fn partition_auto_delete_removes_only_bad_credentials() {
        use std::collections::HashMap;
        // 비번오류만 삭제 대상, 차단·에러·추가인증·"스토어에 없음(account not found)"은 보존.
        let status: HashMap<&str, AccountStatus> = HashMap::from([
            ("bad", AccountStatus::BadCredentials),
            ("blocked", AccountStatus::Blocked),
            ("error", AccountStatus::Error),
            ("challenge", AccountStatus::Challenge),
        ]);
        let failed = vec![
            "bad".to_string(),
            "blocked".to_string(),
            "error".to_string(),
            "challenge".to_string(),
            "gone".to_string(), // 스토어에 없음 → status_of None
        ];
        let (delete_ids, retained_ids) =
            partition_auto_delete(&failed, |id| status.get(id).cloned());
        assert_eq!(delete_ids, vec!["bad"], "비밀번호 오류만 삭제");
        assert_eq!(
            retained_ids,
            vec!["blocked", "error", "challenge", "gone"],
            "차단·에러·추가인증·account not found는 보존(삭제 금지)"
        );
    }

    // ───────── 계정 상태/플랫폼 원격 편집(14-계정상태-관리) ─────────
    fn meta_acct(login: &str, platform: PlatformId, status: AccountStatus) -> Account {
        Account {
            id: login.into(),
            platform,
            login_id: login.into(),
            pw: "pw".into(),
            status,
            status_msg: Some("이전 사유".into()),
            status_trace: Some("이전 trace".into()),
            last: "—".into(),
            tags: vec![],
        }
    }

    #[test]
    fn apply_account_meta_platform_only_keeps_status() {
        let accounts = vec![meta_acct("a", PlatformId::Forum, AccountStatus::Waiting)];
        let updates = vec![AccountMetaUpdate {
            login_id: "a".into(),
            platform: Some("blog".into()),
            status: None,
        }];
        let out = apply_account_meta(accounts, &updates);
        assert_eq!(out[0].platform, PlatformId::Blog);
        // status 미지정 → 그대로(사유/trace도 안 건드림).
        assert_eq!(out[0].status, AccountStatus::Waiting);
        assert_eq!(out[0].status_msg.as_deref(), Some("이전 사유"));
    }

    #[test]
    fn apply_account_meta_status_only_resets_reason() {
        let accounts = vec![meta_acct("a", PlatformId::Forum, AccountStatus::Waiting)];
        let updates = vec![AccountMetaUpdate {
            login_id: "a".into(),
            platform: None,
            status: Some("active".into()),
        }];
        let out = apply_account_meta(accounts, &updates);
        assert_eq!(out[0].platform, PlatformId::Forum); // 플랫폼 그대로
        assert_eq!(out[0].status, AccountStatus::Active);
        // 사람이 되돌림 → 사유/trace None으로 초기화(apply_status_by_login_id 재사용).
        assert_eq!(out[0].status_msg, None);
        assert_eq!(out[0].status_trace, None);
    }

    #[test]
    fn apply_account_meta_both_fields() {
        let accounts = vec![meta_acct("a", PlatformId::Forum, AccountStatus::OnHold)];
        let updates = vec![AccountMetaUpdate {
            login_id: "a".into(),
            platform: Some("naver".into()),
            status: Some("onHold".into()),
        }];
        let out = apply_account_meta(accounts, &updates);
        assert_eq!(out[0].platform, PlatformId::Naver);
        assert_eq!(out[0].status, AccountStatus::OnHold);
    }

    #[test]
    fn apply_account_meta_ignores_unmatched_login_id() {
        let accounts = vec![meta_acct("a", PlatformId::Forum, AccountStatus::Active)];
        let updates = vec![AccountMetaUpdate {
            login_id: "does_not_exist".into(),
            platform: Some("clip".into()),
            status: Some("waiting".into()),
        }];
        let out = apply_account_meta(accounts, &updates);
        // 미매칭 → 원본 불변.
        assert_eq!(out[0].platform, PlatformId::Forum);
        assert_eq!(out[0].status, AccountStatus::Active);
    }

    #[test]
    fn apply_account_meta_ignores_disallowed_status() {
        let accounts = vec![meta_acct("a", PlatformId::Forum, AccountStatus::Active)];
        // blocked/badCredentials 등 워커 판정값은 사람이 못 되돌림 → status 변경 무시.
        let updates = vec![AccountMetaUpdate {
            login_id: "a".into(),
            platform: Some("band".into()),
            status: Some("blocked".into()),
        }];
        let out = apply_account_meta(accounts, &updates);
        assert_eq!(out[0].platform, PlatformId::Band); // 플랫폼은 적용
        assert_eq!(out[0].status, AccountStatus::Active); // status는 불변
    }

    #[test]
    fn status_from_str_allows_only_human_reversible() {
        assert_eq!(status_from_str("active"), Some(AccountStatus::Active));
        assert_eq!(status_from_str("waiting"), Some(AccountStatus::Waiting));
        assert_eq!(status_from_str("onHold"), Some(AccountStatus::OnHold));
        assert_eq!(status_from_str("blocked"), None);
        assert_eq!(status_from_str("badCredentials"), None);
        assert_eq!(status_from_str("timedOut"), None);
    }

    // ───────── 기타 명령(15-기타명령 §2) ─────────

    #[test]
    fn command_parses_etc_payload() {
        // Admin → SSE로 내려온 좋아요 명령이 links/loginIds로 역직렬화되는지.
        let json = r#"{"type":"like_posts","commandId":"c-1",
            "etc":{"links":["l1","l2"],"loginIds":["a","b","c"],"repeats":0}}"#;
        let cmd: Command = serde_json::from_str(json).unwrap();
        assert_eq!(cmd.kind, "like_posts");
        let etc = cmd.etc.unwrap();
        assert_eq!(etc.links, vec!["l1", "l2"]);
        assert_eq!(etc.login_ids, vec!["a", "b", "c"]);
        assert_eq!(etc.repeats, 0);
    }

    #[test]
    fn command_parses_boost_view_repeats() {
        let cmd: Command =
            serde_json::from_str(r#"{"type":"boost_view","etc":{"links":["l1"],"repeats":30}}"#)
                .unwrap();
        let etc = cmd.etc.unwrap();
        assert_eq!(etc.repeats, 30);
        assert!(etc.login_ids.is_empty());
    }

    #[test]
    fn command_rotate_ip_needs_no_etc() {
        let cmd: Command = serde_json::from_str(r#"{"type":"rotate_ip"}"#).unwrap();
        assert_eq!(cmd.kind, "rotate_ip");
        assert!(cmd.etc.is_none());
    }

    #[test]
    fn etc_report_kind_tags_each_action() {
        // §6-3: 결과 보고 목록에서 게시와 구분할 종류 태그.
        assert_eq!(etc_report_kind("like_posts"), "좋아요");
        assert_eq!(etc_report_kind("dislike_posts"), "싫어요");
        assert_eq!(etc_report_kind("boost_view"), "조회수");
        assert_eq!(etc_report_kind("rotate_ip"), "IP");
    }

    #[test]
    fn etc_ack_level_and_message() {
        let etc = EtcCmd {
            links: vec!["l1".into(), "l2".into()],
            login_ids: vec!["a".into()],
            repeats: 0,
        };
        assert_eq!(etc_ack_level("like_posts"), "info");
        assert_eq!(etc_ack_level("rotate_ip"), "info");
        assert!(etc_ack_message("like_posts", &etc).contains("링크 2개 × 계정 1개"));
        let v = EtcCmd {
            links: vec!["l1".into()],
            login_ids: vec![],
            repeats: 30,
        };
        assert!(etc_ack_message("boost_view", &v).contains("링크 1개 × 30회"));
    }

    #[test]
    fn etc_report_body_carries_kind_tag_and_items() {
        // 회신 본문(post-report)은 종류 태그(kind)와 PostItemDto 모양 items를 싣는다.
        let items = vec![etc_item("forum", "l1", "a", true, "좋아요 완료")];
        let body = etc_report_body("like_posts", "좋아요 — 링크 1개 × 계정 1개", items);
        assert_eq!(body["kind"], "좋아요");
        assert_eq!(body["title"], "좋아요 — 링크 1개 × 계정 1개");
        assert_eq!(body["items"][0]["platform"], "forum");
        assert_eq!(body["items"][0]["target"], "l1");
        assert_eq!(body["items"][0]["loginId"], "a");
        assert_eq!(body["items"][0]["status"], "success");
        assert!(body["at"].is_i64());
    }

    #[test]
    fn etc_item_maps_failure_to_fail_status() {
        let it = etc_item("", "IP", "-", false, "IP 변경 실패: 폰 없음");
        assert_eq!(it["status"], "fail");
        assert_eq!(it["target"], "IP");
        assert_eq!(it["msg"], "IP 변경 실패: 폰 없음");
    }

    // ── 블로그 새 글 발행(16-블로그새글) ──

    #[test]
    fn blog_write_settings_maps_open_type_and_flags() {
        use crate::naver_blog::OpenType;
        let s = BlogWriteSettings {
            open_type: 2,
            comment_yn: false,
            search_yn: false,
            tags: "첫글 인생".into(),
        };
        let out = blog_write_settings(&s);
        assert_eq!(out.open_type, OpenType::MutualNeighbor);
        assert!(!out.comment_yn);
        assert!(!out.search_yn);
        assert_eq!(out.tags, "첫글 인생");
    }

    #[test]
    fn blog_write_settings_defaults_to_public_on_unknown_open_type() {
        use crate::naver_blog::OpenType;
        let s = BlogWriteSettings {
            open_type: 9,
            ..BlogWriteSettings::default()
        };
        let out = blog_write_settings(&s);
        assert_eq!(out.open_type, OpenType::Public);
        assert!(out.comment_yn, "기본 댓글 허용");
        assert!(out.search_yn, "기본 검색 허용");
    }

    // Admin 원격 미디어: 원본 블록 분류. imageUpload/fileUpload/oglinkUrl만 미디어 입력으로 잡고,
    // 나머지(text/sticker/이미 해결된 image 등)는 None → 기존 파싱 경로 무손상.
    #[test]
    fn blog_media_input_classifies_image_upload() {
        let v = serde_json::json!({"type":"imageUpload","fileName":"a.png","dataBase64":"AAAA"});
        assert_eq!(
            blog_media_input(&v),
            Some(BlogMediaInput::Image {
                file_name: "a.png".into(),
                data_base64: "AAAA".into()
            })
        );
    }

    #[test]
    fn blog_media_input_classifies_file_and_oglink() {
        let f = serde_json::json!({"type":"fileUpload","fileName":"b.pdf","dataBase64":"Qk0="});
        assert_eq!(
            blog_media_input(&f),
            Some(BlogMediaInput::File {
                file_name: "b.pdf".into(),
                data_base64: "Qk0=".into()
            })
        );
        let o = serde_json::json!({"type":"oglinkUrl","link":"https://naver.com"});
        assert_eq!(
            blog_media_input(&o),
            Some(BlogMediaInput::Oglink {
                link: "https://naver.com".into()
            })
        );
    }

    #[test]
    fn blog_media_input_none_for_non_media_blocks() {
        // 기존 블록들은 None → 호출부가 지금과 똑같이 Block으로 역직렬화(무손상).
        for v in [
            serde_json::json!({"type":"text","text":"hi"}),
            serde_json::json!({"type":"sticker","packCode":"cafe_001","seq":1}),
            serde_json::json!({"type":"image","src":"x","path":"y"}), // 이미 해결된 image
            serde_json::json!({"type":"oglinkUrl"}),                  // link 없음 → None(불완전)
            serde_json::json!({}),                                    // type 없음
        ] {
            assert_eq!(blog_media_input(&v), None, "value={v}");
        }
    }

    #[test]
    fn remap_oglink_meta_moves_url_to_link_and_tags_oglink() {
        let meta = serde_json::json!({
            "url":"https://naver.com/x","title":"T","domain":"naver.com",
            "thumbnailSrc":"t","oglinkSign":"sig","description":"d"
        });
        let b = remap_oglink_meta_to_block(meta);
        assert_eq!(b["type"], "oglink");
        assert_eq!(b["link"], "https://naver.com/x"); // url → link
        assert!(b.get("url").is_none(), "url 키는 제거돼야 한다");
        assert_eq!(b["oglinkSign"], "sig"); // 나머지 필드 보존
        assert_eq!(b["title"], "T");
    }

    #[test]
    fn blog_write_item_success_carries_posted_url() {
        let it = blog_write_item(
            "acc",
            "press02",
            "제목",
            true,
            "발행 성공",
            Some("https://blog.naver.com/PostView.naver?logNo=1"),
        );
        assert_eq!(it["platform"], "blog");
        assert_eq!(it["target"], "press02");
        assert_eq!(it["loginId"], "acc");
        assert_eq!(it["status"], "success");
        assert_eq!(it["posted"]["title"], "제목");
        assert_eq!(
            it["posted"]["url"],
            "https://blog.naver.com/PostView.naver?logNo=1"
        );
    }

    #[test]
    fn blog_write_item_failure_has_no_posted() {
        let it = blog_write_item("acc", "press02", "제목", false, "쿠키 없음", None);
        assert_eq!(it["status"], "fail");
        assert_eq!(it["msg"], "쿠키 없음");
        assert!(it.get("posted").is_none());
    }

    #[test]
    fn blog_write_report_body_tags_as_publish() {
        let items = vec![blog_write_item("acc", "b", "제목", true, "ok", Some("u"))];
        let body = blog_write_report_body("제목", items);
        assert_eq!(body["kind"], "게시");
        assert_eq!(body["title"], "제목");
        assert_eq!(body["items"][0]["loginId"], "acc");
        assert!(body["at"].is_i64());
    }

    #[test]
    fn blog_write_cmd_parses_blocks_as_naver_blog_blocks() {
        // 프론트 blocks.ts 모양(id 포함)이 naver_blog::Block으로 파싱되는지 — id 등 미지 필드는 무시.
        let cmd: BlogWriteCmd = serde_json::from_value(serde_json::json!({
            "title": "새 글",
            "blocks": [
                { "id": "blk-1", "type": "text", "text": "본문", "align": "left" },
                { "id": "blk-2", "type": "code", "code": "let x = 1;" }
            ],
            "settings": { "openType": 3, "commentYn": false, "searchYn": true, "tags": "태그" },
            "targets": [ { "loginId": "acc", "blogId": "press02" } ]
        }))
        .unwrap();
        assert_eq!(cmd.title, "새 글");
        assert_eq!(cmd.targets.len(), 1);
        assert_eq!(cmd.targets[0].blog_id, "press02");
        assert_eq!(cmd.settings.open_type, 3);
        let blocks: Vec<crate::naver_blog::Block> =
            serde_json::from_value(serde_json::Value::Array(cmd.blocks.clone())).unwrap();
        assert_eq!(blocks.len(), 2);
    }
}
