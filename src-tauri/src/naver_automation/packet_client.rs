use std::collections::BTreeMap;
use std::process;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use reqwest::blocking::Client;
use reqwest::header::{
    HeaderMap, HeaderValue, ACCEPT, ACCEPT_LANGUAGE, CONTENT_TYPE, COOKIE, LOCATION, ORIGIN,
    REFERER, RETRY_AFTER, SET_COOKIE, USER_AGENT,
};
use serde_json::{json, Value};
use url::form_urlencoded::Serializer;

use super::types::{DiscussionSelection, NaverLoginProfile};
use super::{AutomationError, AutomationResult};

const STOCK_ORIGIN: &str = "https://stock.naver.com";
const M_STOCK_ORIGIN: &str = "https://m.stock.naver.com";
const CBOX_ORIGIN: &str = "https://apis.naver.com";
const STATIC_NID_ORIGIN: &str = "https://static.nid.naver.com";
// 각 요청의 대상 호스트(쿠키를 호스트별로 스코핑하는 데 쓴다).
const STOCK_HOST: &str = "stock.naver.com";
const M_STOCK_HOST: &str = "m.stock.naver.com";
const CBOX_HOST: &str = "apis.naver.com";
const STATIC_NID_HOST: &str = "static.nid.naver.com";
// 네이버페이 금융서비스 가입(동의하기) 시작 URL. 동의 4종(nf/naver_personalized_service·광고마이데이터·
// 머니스토리)을 **전부 Y**로 보낸다 — 성공한 브라우저 캡처(`npay 약관동의`)와 100% 동일(사용자 지시
// 2026-07-01). 예전엔 전부 N이었는데, N이면 미가입 fresh 계정의 가입이 필수약관(termcd=40)에서 완료
// 안 돼 `commonTermAgree`에 갇히고 이후 `/profile/users/status`가 500으로 막혔다(추정). 필수 약관은
// 리다이렉트 체인의 commonTermAgree에서 처리한다(3xx 아님 → JS 콜백 이동, financial_join_follow가 rurl로
// 따라감). 성공 시 토론 페이지로, 실패 시 약관 페이지로 보낸다.
const FINANCIAL_JOIN_URL: &str = "https://member-web.pay.naver.com/financial-service/join?from_pc=Y&nf_personalized_service_consent=Y&naver_personalized_service_consent=Y&optional_ads_and_mydata_usage_consent=Y&moneystory_subscription_consent=Y&join_success_url=https://stock.naver.com/discussion&join_fail_url=https://member.pay.naver.com/financial-member/agreement";
// npay 약관동의 흐름의 시작 페이지(금융서비스 약관 페이지). 실측 패킷(`동의+프로필까지`)에서 join·
// commonTermAgree 요청의 referer가 정확히 이 페이지다. nid는 referer로 "정상 가입 흐름에서 온
// 요청"인지 검사하는데, 우리가 referer를 안 보내 commonTermAgree가 nidlogin.login으로 튕겼다.
const NPAY_AGREEMENT_REFERER: &str =
    "https://member.pay.naver.com/financial-member/agreement?rurl=https://stock.naver.com/discussion";
const DEFAULT_REFERER: &str = "https://stock.naver.com/discussion";

/// 실명인증(본인인증) 미완 계정이 게시 시 브라우저가 튕겨가는 nid 실명확인 페이지(실측 2026-07-03,
/// 패킷 캡처 `실명인증`). 프로필 생성 500(Failed to create profile user) 뒤 이 URL을 쿠키 달아 GET해
/// 실명확인 폼이 오면 "실명인증 필요"로 확정한다. 응답 원문은 캡처에 미포함 → 본문 마커로 best-effort.
const REAL_NAME_CHECK_URL: &str =
    "https://nid.naver.com/user2/help/realNameCheck.nhn?type=1&rurl=&surl=";
const NID_HOST: &str = "nid.naver.com";
/// 실명인증 미완 실패에 붙이는 사유 접두(호출부 queue_runner가 이 마커로 로그인실패 동일 처리한다).
/// 차단 마커("권한없음/로그인만료" 등)를 안 담아 `is_blocking_failure`에 안 걸리게 한다 → 상태 Error.
const REAL_NAME_NOT_VERIFIED_MARKER: &str = "실명인증 미완(본인인증 필요) — ";
// 실측 브라우저 referer 1:1: 프로필 상태(/status)는 루트, getProfile은 네이버 홈에서 온다.
const STOCK_ROOT_REFERER: &str = "https://stock.naver.com/";
const NAVER_HOME_REFERER: &str = "https://www.naver.com/";
const DEFAULT_PROFILE_INTRODUCTION: &str = "2222";
// 신규 계정 프로필 생성 시 기본 아바타(성공 캡처에서 브라우저가 보낸 값).
const DEFAULT_PROFILE_AVATAR: &str =
    "https://ssl.pstatic.net/imgstock/fn/real/_front/image/profile/avatar-12.png";

// 글쓰기 form(txId)·add 가 다종목 연속 게시 때 간헐적으로 429를 반환하므로
// 일시적 실패(429·5xx)에 한해 70초 대기 후 재시도한다. 네이버 레이트리밋 창이
// 종목 간 대기(60초)보다 길어, 사수 요청대로 백오프를 70초로 고정한다. 최대 4회(=3회 재시도).
const POST_RETRY_MAX_ATTEMPTS: u32 = 4;
const POST_RETRY_BASE: Duration = Duration::from_secs(70);
const POST_RETRY_MAX_DELAY: Duration = Duration::from_secs(70);

// 전송 계층(연결/DNS/타임아웃) 실패 재시도용. POST의 70초 백오프와 달리, 일시적 망 끊김(IP
// 교체 직후 등)은 곧 복구되므로 짧게(0.5→1→2초, 상한 3초) 몇 번만 다시 보낸다.
const TRANSPORT_RETRY_MAX_ATTEMPTS: u32 = 4;
const TRANSPORT_RETRY_BASE: Duration = Duration::from_millis(500);
const TRANSPORT_RETRY_MAX_DELAY: Duration = Duration::from_secs(3);

// Chrome에서 수거한 쿠키 한 개(도메인까지 보존). 이름만으로 합치면 서브도메인별
// host-scoped 동일 이름 쿠키(NNB, 서비스별 세션/CSRF 등)가 last-write-wins로 뭉개져
// 호스트 간에 누출되므로, (domain, name)으로 구분해 둔다.
#[derive(Clone)]
struct NaverCookie {
    domain: String,
    name: String,
    value: String,
}

pub(super) struct NaverPacketClient {
    client: Client,
    cookies: Vec<NaverCookie>,
    user_agent: String,
}

/// npay 금융서비스 가입(동의) 시도의 최종 판정(2026-07-01). 이후 프로필 상태가 500나면, 그게
/// "계정 보호조치(nid 인증 거부)" 때문인지 "약관 미완료" 때문인지 가르는 데 쓴다.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum NpayJoinStatus {
    /// 가입 완료(성공 콜백/토론 페이지로 착지) — 이미 가입됐거나 방금 완료.
    Completed,
    /// 필수약관 미완료(commonTermAgree에 멈춤) — 로그인 시점 브라우저 가입(#364)이 필요한 상태.
    TermsPending,
    /// nid가 로그인 페이지로 튕김(nidlogin.login) — 세션 무효/계정 보호조치 추정. 이 계정은 게시 불가.
    LoginRequired,
    /// 전송 실패 등 판정 불가.
    Unknown,
}

/// 좋아요가 막힌 계정의 **제재 상태 원문 판정**(2026-07-03). `probe_restriction_raw`가
/// `/api/community/profile/users/form` 응답 **원문 전체**를 보고 돌려준다.
pub(super) enum RestrictionVerdict {
    /// 차단/제재 확정 — 본문에 UMON 밴/아이디 잠금/보호조치/이용제한/글쓰기 금지/비정상적인 활동.
    Blocked(String),
    /// 로그인 안 됨(세션 만료) — 로그인 페이지/비로그인 응답.
    Expired(String),
    /// 정상(비차단·로그인됨) — 좋아요 실패는 계정 아닌 대상(글 삭제 등) 문제.
    Healthy,
    /// 전송 실패 등 판정 불가.
    Unknown(String),
}

/// 게시글의 현재 반응(좋아요/싫어요) 상태. `GET /posts/reactions?postIds=` 응답에서 뽑는다.
/// `reaction_id`가 있으면 내가 이미 어떤 반응을 눌러 둔 것이고(변경은 PUT), 없으면 최초(POST)다.
pub(super) struct PostReaction {
    /// 내가 이 글에 "좋아요"(recommend)를 눌러 둔 상태인지.
    recommended: bool,
    /// 내가 이 글에 "싫어요"(notRecommend)를 눌러 둔 상태인지(싫어요 멱등 판정에 사용).
    not_recommended: bool,
    /// 내가 눌러 둔 기존 반응의 id(없으면 `None` — 최초 반응이라 POST로 생성).
    reaction_id: Option<String>,
}

struct DiscussionTarget {
    discussion_type: String,
    item_code: String,
}

pub(super) struct PacketDiscussionRoom {
    pub selection: DiscussionSelection,
    pub discussion_url: String,
}

pub(super) struct PacketDiscussionPost {
    pub post_url: String,
}

struct StockCandidate {
    item_code: String,
    item_name: String,
    rank: String,
}

struct PostCandidate {
    post_id: String,
}

impl NaverPacketClient {
    /// 저장된 로그인 쿠키 파일만으로 패킷 클라이언트를 만든다(계정 ID 기준) — **Chrome 없이 API
    /// 전용**. 종목토론방 게시는 예전엔 Chrome에 쿠키를 주입→`getAllCookies`로 되뽑아 만들었지만,
    /// 카페 경로처럼 저장 쿠키(`read_account_cookies`)를 곧바로 로드하면 Chrome이 전혀 필요 없다.
    /// host-only 쿠키(NID_JST 등)도 저장 파일에 그대로 있어 전량 로드된다(도메인 필터·세션(NID_AUT/
    /// NID_SES) 검증·데스크톱 UA는 [`from_storage_state`]에 위임 — 좋아요 경로와 100% 동일).
    pub(super) fn from_saved_cookies(account_id: &str) -> AutomationResult<Self> {
        let storage = crate::auth::read_account_cookies(account_id)
            .map_err(|error| {
                AutomationError::new(format!("계정 쿠키 파일을 읽지 못했습니다: {error}"))
            })?
            .ok_or_else(|| {
                AutomationError::new(format!(
                    "계정 '{account_id}'의 유효한 로그인 쿠키가 없습니다. 먼저 로그인 자동화를 실행해 쿠키를 저장하세요."
                ))
            })?;
        Self::from_storage_state(&storage)
    }

    /// 저장된 로그인 쿠키(storageState JSON)만으로 패킷 클라이언트를 만든다 — **Chrome 없이 API
    /// 전용**. 좋아요처럼 페이지 렌더링이 전혀 필요 없는 기능에서 쓴다(사수 지시: 페이지 이동
    /// 없이 API로만). 쿠키는 카페 경로와 동일하게 파일에서 읽으며(naver.com/pstatic.net 도메인만),
    /// 네이버 세션 쿠키(NID_AUT/NID_SES)가 없으면 로그인 만료로 보고 명시적으로 실패한다.
    pub(super) fn from_storage_state(storage: &Value) -> AutomationResult<Self> {
        let mut cookies: Vec<NaverCookie> = Vec::new();
        if let Some(arr) = storage.get("cookies").and_then(Value::as_array) {
            for cookie in arr {
                let (Some(domain), Some(name), Some(value)) = (
                    cookie.get("domain").and_then(Value::as_str),
                    cookie.get("name").and_then(Value::as_str),
                    cookie.get("value").and_then(Value::as_str),
                ) else {
                    continue;
                };
                if domain.contains("naver.com") || domain.contains("pstatic.net") {
                    cookies.push(NaverCookie {
                        domain: domain.to_owned(),
                        name: name.to_owned(),
                        value: value.to_owned(),
                    });
                }
            }
        }
        let has = |name: &str| cookies.iter().any(|c| c.name == name);
        if !has("NID_AUT") || !has("NID_SES") {
            return Err(AutomationError::new(
                "저장된 로그인 쿠키에 네이버 세션(NID_AUT/NID_SES)이 없습니다. 계정을 다시 로그인하세요.",
            ));
        }
        let client = Client::builder()
            .timeout(Duration::from_secs(20))
            .redirect(reqwest::redirect::Policy::limited(10))
            .build()
            .map_err(|error| {
                AutomationError::new(format!("Rust HTTP 클라이언트 생성 실패: {error}"))
            })?;
        Ok(Self {
            client,
            cookies,
            // Chrome이 없어 navigator.userAgent를 못 읽으므로, 카페 경로와 동일한 데스크톱 크롬 UA를
            // 재사용한다(네이버 JSON API는 이 UA로 정상 응답).
            user_agent: crate::naver_cafe::post::client::BROWSER_USER_AGENT.to_owned(),
        })
    }

    // Wireshark에서 확인한 static.nid.naver.com getProfile 패킷을 Rust HTTP 요청으로 재현하는 함수입니다.
    pub(super) fn read_login_profile(&self) -> AutomationResult<NaverLoginProfile> {
        let callback = format!("pstmacroProfile_{}", timestamp_nanos());
        let url = format!("{STATIC_NID_ORIGIN}/getProfile?svc=my&callback={callback}");
        let response_text = self
            .get_with_transport_retry(
                &url,
                self.static_headers(STATIC_NID_HOST, NAVER_HOME_REFERER)?,
                "getProfile",
            )
            .and_then(|response| response_text(response, "getProfile"))?;
        let json_text = strip_jsonp(&response_text)?;
        let value = parse_json(json_text, "getProfile")?;

        Ok(NaverLoginProfile {
            logged_in: value
                .get("rtn_cd")
                .and_then(Value::as_str)
                .map(|code| code == "0")
                .unwrap_or(false),
            nickname: value
                .get("nick_name")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned),
            image_url: value
                .get("image_url")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned),
            message: value
                .get("rtn_msg")
                .or_else(|| value.get("rtn_cd"))
                .and_then(Value::as_str)
                .unwrap_or("unknown")
                .to_owned(),
        })
    }

    // Wireshark에서 확인한 랭킹/시세 API를 호출해 랜덤 종목 토론방을 선택하는 함수입니다.
    pub(super) fn select_random_discussion_room(&self) -> AutomationResult<PacketDiscussionRoom> {
        let categories = [
            (
                "토론급상승",
                "/api/community/discussion/rankings?nationType=KOR&page=1&size=10&postType=HOT",
            ),
            (
                "상승",
                "/api/domestic/market/stock/default?tradeType=KRX&marketType=ALL&orderType=up&startIdx=0&pageSize=10",
            ),
            (
                "하락",
                "/api/domestic/market/stock/default?tradeType=KRX&marketType=ALL&orderType=down&startIdx=0&pageSize=10",
            ),
            (
                "거래량",
                "/api/domestic/market/stock/default?tradeType=KRX&marketType=ALL&orderType=quantTop&startIdx=0&pageSize=10",
            ),
        ];
        let seed = selection_seed();
        let start = pseudo_index_with_seed(categories.len(), seed, 0xCA7E);

        for offset in 0..categories.len() {
            let (category, path) = categories[(start + offset) % categories.len()];
            let value = self.get_stock_json(path, DEFAULT_REFERER, category)?;
            let candidates = collect_stock_candidates(&value);

            if candidates.is_empty() {
                continue;
            }

            let picked =
                &candidates[pseudo_index_with_seed(candidates.len(), seed, 0x51 + offset as u128)];
            let discussion_url = discussion_url_for("domesticStock", &picked.item_code, None);

            return Ok(PacketDiscussionRoom {
                selection: DiscussionSelection {
                    category: category.to_owned(),
                    rank: picked.rank.clone(),
                    item_text: format!("{} ({})", picked.item_name, picked.item_code),
                    method: "packet-api".to_owned(),
                },
                discussion_url,
            });
        }

        Err(AutomationError::new(
            "패킷 API 응답에서 선택 가능한 랜덤 종목을 찾지 못했습니다.",
        ))
    }

    // Wireshark에서 확인한 posts/by-item API를 호출해 랜덤 토론글 URL을 선택하는 함수입니다.
    pub(super) fn select_random_discussion_post(
        &self,
        page_url: &str,
    ) -> AutomationResult<PacketDiscussionPost> {
        let target = discussion_target_from_url(page_url)?;
        let attempts = [
            format!(
                "/api/community/discussion/posts/by-item?discussionType={}&itemCode={}&isHolderOnly=false&excludesItemNews=false&isItemNewsOnly=false&isCleanbotPassedOnly=true&pageSize=10",
                target.discussion_type, target.item_code
            ),
            format!(
                "/api/community/discussion/posts/by-item?discussionType={}&itemCode={}&isHolderOnly=false&excludesItemNews=false&isItemNewsOnly=false&isCleanbotPassedOnly=false&pageSize=30",
                target.discussion_type, target.item_code
            ),
        ];

        for path in attempts {
            let value = self.get_stock_json(&path, page_url, "토론글 목록")?;
            let posts = collect_post_candidates(&value);

            if posts.is_empty() {
                continue;
            }

            let picked = &posts[pseudo_index_with_seed(posts.len(), selection_seed(), 0xB057)];
            let post_url = discussion_url_for(
                &target.discussion_type,
                &target.item_code,
                Some(&picked.post_id),
            );

            return Ok(PacketDiscussionPost { post_url });
        }

        Err(AutomationError::new(
            "패킷 API 응답에서 선택 가능한 랜덤 토론글을 찾지 못했습니다.",
        ))
    }

    // Wireshark 성공 캡처에서 확인한 status/form/validate/PUT 패킷으로 프로필 소개를 설정하는 함수입니다.
    pub(super) fn ensure_profile_intro_setup(&self, referer: &str) -> AutomationResult<bool> {
        // referer를 실측 브라우저와 1:1로 맞춘다: 프로필 상태(/status)는 stock 루트, 나머지(/form·닉네임
        // 추천·소개 검증·생성/수정 POST)는 stock.naver.com/discussion. 호출부가 넘긴 종목 URL은 안 쓴다.
        let _ = referer;
        let referer = DEFAULT_REFERER;
        // npay 가입 직후엔 네이버 서버 반영 지연으로 이 status가 잠깐 500("Failed to fetch profile
        // user status")을 준다 — 가입 실패가 아니라 반영 대기다(실측 2026-07-01: 방금 가입한 fresh
        // 계정 첫 조회 500 → 잠시 후 200). 짧은 백오프로 재시도해 반영될 때까지 기다린다.
        let status = self.read_profile_status_with_retry(STOCK_ROOT_REFERER)?;
        let status_text = status
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or_default();

        if status_text == "existent" {
            return Ok(false);
        }

        // 신규 계정은 status="nonExistent", profileId=null 이다(성공 캡처 확인). 이때 브라우저는
        // POST /users 로 프로필을 새로 만든다(만들 id가 없으니 PUT /{id} 가 아니다). profileId가
        // 이미 있는(부분 생성된) 계정은 기존 PUT 경로를 그대로 유지한다.
        let profile_id = status.get("profileId").and_then(Value::as_str);
        let response_text = match profile_id {
            // 신규 계정 — 프로필 생성(POST). form이 아직 없으므로 추천 닉네임과 기본 아바타를 쓴다.
            None => {
                let nickname = self.recommend_profile_nickname(referer)?;
                self.validate_profile_introduction(referer)?;
                let payload = json!({
                    "nickname": nickname,
                    "introduction": DEFAULT_PROFILE_INTRODUCTION,
                    "imageUrl": DEFAULT_PROFILE_AVATAR,
                    "danglingImages": [],
                });
                self.client
                    .post(format!("{STOCK_ORIGIN}/api/community/profile/users"))
                    .headers(self.stock_json_headers(STOCK_HOST, referer)?)
                    .json(&payload)
                    .send_traced(&self.client)
                    .map_err(|error| {
                        AutomationError::new(format!("프로필 생성 POST 패킷 전송 실패: {error}"))
                    })
                    .and_then(|response| response_text(response, "프로필 생성 POST"))
                    .map_err(|error| {
                        // create 실패(주로 500 "Failed to create profile user")면 nid realNameCheck를
                        // 한 번 호출해 **실명인증 필요**를 확정한다(isNiConnected는 성공 계정도 false라
                        // 무용 → 폐기, 2026-07-03). 확정되면 마커를 붙여 호출부(queue_runner)가 로그인
                        // 실패와 동일 처리(상태 Error·"권한이 없거나 로그인이 만료되었습니다"·목록숨김)하게
                        // 한다. 원문은 뒤에 보존. 미확정이면 원래 오류 그대로(오탐 방지).
                        if error.to_string().contains("Failed to create profile user")
                            && self.real_name_verification_required()
                        {
                            AutomationError::new(format!("{REAL_NAME_NOT_VERIFIED_MARKER}{error}"))
                        } else {
                            error
                        }
                    })?
            }
            // 기존(부분 생성) 프로필 — 기존 PUT 경로 그대로(닉네임/이미지는 form 값을 쓴다).
            Some(profile_id) => {
                let profile_id = profile_id.to_owned();
                let form = self.get_stock_json(
                    "/api/community/profile/users/form",
                    referer,
                    "프로필 form",
                )?;
                let nickname = form
                    .get("nickname")
                    .and_then(Value::as_str)
                    .filter(|value| !value.trim().is_empty())
                    .map(ToOwned::to_owned)
                    .map(Ok)
                    .unwrap_or_else(|| self.recommend_profile_nickname(referer))?;

                self.validate_profile_introduction(referer)?;

                let payload = json!({
                    "nickname": nickname,
                    "introduction": DEFAULT_PROFILE_INTRODUCTION,
                    "imageUrl": form.get("imageUrl").cloned().unwrap_or(Value::Null),
                    "danglingImages": [],
                });
                self.client
                    .put(format!(
                        "{STOCK_ORIGIN}/api/community/profile/users/{profile_id}"
                    ))
                    .headers(self.stock_json_headers(STOCK_HOST, referer)?)
                    .json(&payload)
                    .send_traced(&self.client)
                    .map_err(|error| {
                        AutomationError::new(format!("프로필 저장 PUT 패킷 전송 실패: {error}"))
                    })
                    .and_then(|response| response_text(response, "프로필 저장 PUT"))?
            }
        };

        if !response_text.trim().is_empty() {
            let _ = parse_json(&response_text, "프로필 저장");
        }

        let updated = self.get_stock_json(
            "/api/community/profile/users/status",
            STOCK_ROOT_REFERER,
            "프로필 상태 재확인",
        )?;
        let updated_status = updated
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or_default();

        if updated_status != "existent" {
            return Err(AutomationError::new(format!(
                "프로필 저장 후 상태가 existent가 아닙니다: {updated_status}"
            )));
        }

        Ok(true)
    }

    // 네이버페이 금융서비스 가입(= 종목토론방 "동의하기")을 패킷으로 보장한다. #344가 브라우저
    // 이동을 없애며 빠진 단계의 패킷 복원이다. 패킷 분석상 "동의하기"는 체크박스가 아니라 가입
    // URL로 가는 GET 리다이렉트 체인(join?consent=N → 302 → 약관동의 termcd=40 → 콜백 → 가입
    // 완료)이라, 로그인 쿠키를 든 이 클라이언트로 그 URL을 GET(리다이렉트 최대 10회 추종)하면
    // 가입이 끝난다. 선택 동의(마케팅/마이데이터/머니스토리)는 전부 N으로 거절한다. 이미 가입된
    // 계정은 성공 콜백으로 리다이렉트되어 무해(멱등). 비치명적 — 전송이 실패해도 글쓰기는
    // 시도하게 두고(이미 가입돼 있으면 글쓰기는 성공), 최종 URL·status를 로그로 남겨 가입 완료
    // 여부를 사용자가 로그에서 확인할 수 있게 한다.
    pub(super) fn ensure_npay_financial_join(&mut self) -> NpayJoinStatus {
        tracing::info!(
            api = "GET /financial-service/join",
            "실제 API 호출 label=\"네이버페이 가입(동의하기)\""
        );
        let (status, final_url, rotated_cookies) = match self.financial_join_follow() {
            Ok(result) => result,
            Err(error) => {
                // 전송 실패는 비치명적: 이미 가입된 계정이면 뒤의 글쓰기는 그대로 성공한다.
                tracing::warn!(
                    "네이버페이 가입(동의하기) 전송 실패 — 건너뜀(글쓰기는 계속): {error}"
                );
                return NpayJoinStatus::Unknown;
            }
        };
        // npay 가입이 회전시킨 세션 쿠키(NID_AUT/NID_SES 재발급 등)를 본 클라이언트에 반영한다.
        // 이걸 안 하면 이후 프로필 status/게시가 로그인 시점의 옛 토큰으로 나가, 서버가 "금융회원
        // 미반영 세션"으로 보고 프로필 status 500을 준다(실측 2026-07-01 yeh: npay는 됐는데 status만 500).
        let peek_c = |cookies: &[NaverCookie], name: &str| -> String {
            cookies
                .iter()
                .find(|c| c.name == name)
                .map(|c| c.value.chars().take(12).collect::<String>())
                .unwrap_or_else(|| "(없음)".to_owned())
        };
        let aut_before = peek_c(&self.cookies, "NID_AUT");
        let ses_before = peek_c(&self.cookies, "NID_SES");
        self.cookies = rotated_cookies;
        let aut_after = peek_c(&self.cookies, "NID_AUT");
        let ses_after = peek_c(&self.cookies, "NID_SES");
        tracing::info!(
            rotated = aut_before != aut_after || ses_before != ses_after,
            nid_aut_before = %aut_before,
            nid_aut_after = %aut_after,
            nid_ses_before = %ses_before,
            nid_ses_after = %ses_after,
            "[npay] 가입 후 회전된 세션 쿠키를 클라이언트에 반영"
        );
        if financial_join_completed(&final_url) {
            tracing::info!(
                status,
                final_url = %final_url,
                "네이버페이 가입(동의하기) 완료 — 가입 콜백으로 리다이렉트됨 ✅"
            );
            NpayJoinStatus::Completed
        } else if final_url.contains("nidlogin.login") {
            // nid가 로그인 페이지로 튕김 = 이 계정의 nid 인증을 거부 = 세션 무효/계정 보호조치 추정.
            // 이 계정은 프로필 상태 조회도 500나고 글도 전부 실패하므로, 호출부가 "재로그인 필요"
            // 차단으로 다뤄 남은 글을 건너뛰게 한다(실측 2026-07-01: 보호조치 계정 kkch****).
            tracing::warn!(
                status,
                final_url = %final_url,
                "네이버페이 가입(동의하기) — nid 로그인 페이지로 튕김. 계정 보호조치/세션 무효 추정 → 재로그인 필요"
            );
            NpayJoinStatus::LoginRequired
        } else {
            tracing::warn!(
                status,
                final_url = %final_url,
                "네이버페이 가입(동의하기) 미완료 추정 — 최종 URL이 약관 페이지. 미가입 계정이면 로그인 시점 브라우저 가입(#364)이 필요"
            );
            NpayJoinStatus::TermsPending
        }
    }

    /// **좋아요 전용** npay 금융서비스 가입 시도. [`Self::ensure_npay_financial_join`]과 달리 가입이
    /// 실제 **완료(Completed)됐을 때만** 회전 쿠키를 반영하고, nid 로그인 페이지로 튕기거나(세션 만료)
    /// 약관 페이지에 멈추면 **기존 로그인 쿠키를 보존한다**(덮어쓰지 않음).
    ///
    /// 게시 경로는 회전 쿠키를 항상 반영해야 프로필 status가 통과하지만(#364 맥락), 좋아요 경로는
    /// 튕김 시 쿠키가 빈 값으로 덮어써지면 이어지는 반응 생성이 400Z01("[NidHeader] Nid-No is
    /// required")로 막힌다 — 밤새 방치된 세션이 가입에서 로그인으로 튕겨 NID_AUT가 비워지고 좋아요가
    /// 전량 실패한 실측(2026-07-02 야간 배치)의 원인이다. 그래서 좋아요 경로만 쿠키를 보존해 만료
    /// 계정을 훼손 없이 그대로 "재로그인 필요"로 넘긴다. 반환은 [`NpayJoinStatus`].
    pub(super) fn try_npay_join_preserving_cookies(&mut self) -> NpayJoinStatus {
        tracing::info!(
            api = "GET /financial-service/join",
            "실제 API 호출 label=\"네이버페이 가입(동의하기)\" mode=좋아요-쿠키보존"
        );
        self.log_auth_cookies_raw("좋아요-npay-시도전");
        let (status, final_url, rotated_cookies) = match self.financial_join_follow() {
            Ok(result) => result,
            Err(error) => {
                // 전송 실패는 비치명적: 쿠키를 건드리지 않고 그대로 넘겨 호출부가 좋아요를 재시도한다.
                tracing::warn!("네이버페이 가입(동의하기) 전송 실패 — 건너뜀(쿠키 보존): {error}");
                return NpayJoinStatus::Unknown;
            }
        };
        if financial_join_completed(&final_url) {
            // 가입이 실제 완료된 경우에만 회전 쿠키(재발급 NID_AUT/NID_SES 등)를 반영한다.
            self.cookies = rotated_cookies;
            self.log_auth_cookies_raw("좋아요-npay-완료-회전후");
            tracing::info!(
                status,
                final_url = %final_url,
                "네이버페이 가입(동의하기) 완료 — 회전 쿠키 반영 ✅ (좋아요-쿠키보존)"
            );
            NpayJoinStatus::Completed
        } else if final_url.contains("nidlogin.login") {
            // 로그인 페이지로 튕김 = 세션 무효/만료. 회전된(빈) NID_AUT를 **반영하지 않아** 기존
            // 로그인 쿠키를 지키고, 호출부가 "재로그인 필요"로 처리하게 한다. 반영하지 않는 그 회전
            // 쿠키(빈 NID_AUT 등)도 원문 그대로 남겨 무엇 때문에 튕겼는지 대조할 수 있게 한다.
            tracing::warn!(
                rotated = %auth_cookies_raw_line(&rotated_cookies),
                "[좋아요][쿠키원문] npay 튕김으로 받은 회전 쿠키(반영 안 함, 원문 그대로)"
            );
            tracing::warn!(
                status,
                final_url = %final_url,
                "네이버페이 가입(동의하기) — nid 로그인 페이지로 튕김. 세션 만료 추정 → 쿠키 보존, 재로그인 필요"
            );
            NpayJoinStatus::LoginRequired
        } else {
            // 약관 페이지에 멈춤(미가입 추정) — 쿠키를 보존한 채 호출부가 좋아요를 재시도한다.
            tracing::warn!(
                status,
                final_url = %final_url,
                "네이버페이 가입(동의하기) 미완료(약관 페이지) — 쿠키 보존, 미가입 계정은 로그인 시점 브라우저 가입(#364) 필요"
            );
            NpayJoinStatus::TermsPending
        }
    }

    /// 현재 클라이언트가 들고 있는 네이버 인증 쿠키(NID_AUT/NID_SES/BUC)를 **원문 그대로** 로그에
    /// 남긴다. 좋아요 경로에서만 호출한다(사용자 요청 2026-07-03). ⚠️ 살아있는 세션 토큰이라 로그
    /// 파일은 민감정보.
    pub(super) fn log_auth_cookies_raw(&self, tag: &str) {
        tracing::info!(
            tag,
            cookies = %auth_cookies_raw_line(&self.cookies),
            "[좋아요][쿠키원문] 현재 인증 쿠키(원문 그대로)"
        );
    }

    /// 가입 GET의 리다이렉트 체인을 **직접** 따라가며, 매 홉마다 그 홉의 호스트 쿠키를 붙인다.
    ///
    /// reqwest의 자동 리다이렉트 추종은 보안상 **크로스-호스트 리다이렉트에서 Cookie 헤더를 제거**한다.
    /// 그래서 `member-web.pay.naver.com/join` → `nid.naver.com/commonTermAgree`(필수 약관) 리다이렉트
    /// 에서 우리 로그인 쿠키가 사라져, nid가 인증 실패로 보고 `nidlogin.login`(로그인 페이지)으로
    /// 튕겼다 — 미가입 계정 가입 실패의 실제 원인(실측 패킷·로그 2026-07-01). 브라우저는 쿠키 자를
    /// 써서 호스트마다 그 호스트 쿠키를 자동 첨부하고 **홉마다 Set-Cookie를 누적**하므로 통과한다.
    /// 여기서도 자동 추종을 끄고(`redirect::none`), 로그인 쿠키에서 출발한 jar에 홉마다 `merge_set_cookies`
    /// 로 응답 Set-Cookie를 병합해(회전된 BUC·재발급된 NID_AUT/NID_SES) 다음 홉에 `navigation_headers`
    /// 로 실어 따라간다 — 이 누적이 미가입 계정의 commonTermAgree를 200으로 통과시키는 핵심이다(실측
    /// 패킷 `동의+프로필까지`). 최종 (status, url)을 돌려준다. best-effort — 호출부가 실패를 삼키고
    /// 글쓰기를 계속한다.
    // 반환: (최종 status, 최종 url, 홉마다 누적된 쿠키 jar). jar에는 npay 가입이 회전시킨 세션
    // 쿠키(NID_AUT/NID_SES 재발급 등)가 들어있어, 호출부가 self.cookies에 반영해야 이후 프로필
    // status/게시가 "금융회원 반영된 새 세션"으로 나간다(안 하면 옛 토큰으로 status 500 — 실측 yeh).
    fn financial_join_follow(&self) -> AutomationResult<(u16, String, Vec<NaverCookie>)> {
        const MAX_HOPS: u32 = 15;
        let client = Client::builder()
            .timeout(Duration::from_secs(20))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|error| {
                AutomationError::new(format!("가입 HTTP 클라이언트 생성 실패: {error}"))
            })?;
        // 브라우저처럼 홉마다 Set-Cookie를 이어받는 쿠키 자(jar). 로그인 쿠키에서 출발해, 가입
        // 리다이렉트 체인이 중간에 회전시키는 쿠키(실측 패킷 `동의+프로필까지` 2026-07-01: 홉 사이
        // BUC 회전, commonTermAgree가 200으로 재발급하는 NID_AUT/NID_SES)를 누적해 다음 홉에 실어
        // 보낸다. 이 누적이 없으면 미가입 계정의 commonTermAgree가 200이 아니라 nidlogin.login으로
        // 302 튕겨(옛 로그의 실패 원인) 가입이 안 끝났다 — 브라우저 쿠키 자 동작을 페이지 이동 없이
        // 순수 GET으로 재현한다.
        let mut jar = self.cookies.clone();
        let mut url = FINANCIAL_JOIN_URL.to_string();
        // referer는 브라우저와 1:1로 맞춘다: 흐름은 약관 페이지에서 시작하고, 302 리다이렉트는 원래
        // referer를 그대로 유지하며(실측: join·commonTermAgree 모두 약관 페이지 referer), JS 콜백
        // 이동(commonTermAgree 200 → callback)만 referer를 그 commonTermAgree 페이지로 바꾼다.
        let mut referer = NPAY_AGREEMENT_REFERER.to_string();
        for _hop in 0..MAX_HOPS {
            let host = url::Url::parse(&url)
                .ok()
                .and_then(|parsed| parsed.host_str().map(ToOwned::to_owned))
                .unwrap_or_default();
            let headers = self.navigation_headers(&jar, &host, &referer)?;
            // [진단] nid 홉(commonTermAgree)에 실제로 나가는 Cookie에 로그인 세션 쿠키(NID_JST 등 nid
            // host-only)가 실렸는지 한 줄로 남긴다 — nidlogin.login 튕김이 "쿠키 누락"인지 "계정
            // 보호조치"인지 가른다(실측 성공패킷: nid 요청만 NID_JST/nid_buk/nid_slevel 3개를 더 실음).
            if host.contains("nid.naver.com") {
                let cookie_line = build_cookie_header(&jar, &host);
                tracing::info!(
                    hop = %host,
                    has_nid_jst = cookie_line.contains("NID_JST"),
                    has_nid_buk = cookie_line.contains("nid_buk"),
                    has_nid_slevel = cookie_line.contains("nid_slevel"),
                    "[npay] nid 요청 쿠키 점검 — NID_JST 유무"
                );
            }
            let response = client
                .get(&url)
                .headers(headers)
                .send_traced(&client)
                .map_err(|error| {
                    AutomationError::new(format!("가입 GET 전송 실패({host}): {error}"))
                })?;
            // 이 홉이 준 Set-Cookie를 jar에 병합해 다음 홉이 회전된 쿠키를 쓰게 한다(브라우저와 동일).
            merge_set_cookies(
                &mut jar,
                response.headers().get_all(SET_COOKIE).iter(),
                &host,
            );
            let status = response.status();
            // [진단·로컬테스트] 가입 GET 체인을 홉마다 한 줄씩 추적한다 — 우리 체인이 성공 캡쳐
            // (agreement→join→commonTermAgree→callback→success)와 어디서 갈리는지 눈으로 확인용.
            // status/Location을 남겨, ①agreement 누락 여부·③commonTermAgree 200 여부·④callback의
            // 리다이렉트 대상을 실측한다.
            let hop_location = response
                .headers()
                .get(LOCATION)
                .and_then(|value| value.to_str().ok())
                .unwrap_or("")
                .to_owned();
            tracing::info!(
                hop = _hop,
                host = %host,
                status = status.as_u16(),
                location = %hop_location,
                url = %url,
                "[npay][hop] 가입 체인 추적"
            );
            if !status.is_redirection() {
                // 필수약관 페이지(commonTermAgree termcd=40)는 HTTP 3xx가 아니라 200 HTML을 주고,
                // 그 안의 JS `location.href = Base64.decode(<콜백URL>)`로 약관동의 콜백으로 이동한다
                // (실측 패킷 `npay 약관동의`). reqwest는 JS를 못 도니 여기서 멈춰, 미가입 fresh 계정이
                // 가입 미완료로 갇히고 이후 `/profile/users/status`가 500난다(2026-07-01 zip****, 추정).
                // 그 콜백 URL은 commonTermAgree의 `rurl` 쿼리에 그대로 들어있으므로(= 브라우저가 JS로
                // 가던 그 주소), 이어서 GET 하면 콜백이 302로 가입을 완료시킨다.
                if let Some(callback) = term_agree_callback_url(&url) {
                    // commonTermAgree 200 HTML의 JS가 콜백으로 이동 → 콜백 요청의 referer는 이
                    // commonTermAgree 페이지가 된다(실측 패킷 8593의 referer).
                    referer = url.clone();
                    url = callback;
                    continue;
                }
                // [사수 지시: 네이버 실제 응답 그대로 로그 / 로컬 진단] 가입 리다이렉트가 멈춘 최종
                // 페이지(예: nidlogin.login=보호조치·재로그인 요구, commonTermAgree=약관, discussion=성공)의
                // **원본 body를 그대로** 남긴다 — 우리 "보호조치 추정" 해석이 아니라 네이버가 실제로 준
                // 내용을 눈으로 확인하기 위함. 계정이 막혔으면 이 body에 네이버의 실제 안내 문구가 있다.
                let final_status = status.as_u16();
                let body = response.text().unwrap_or_default();
                tracing::warn!(
                    status = final_status,
                    final_url = %url,
                    naver_body = %log_snippet(&body),
                    "[npay] 계정상태 확인 — 네이버 최종 응답 원문(가입 리다이렉트가 멈춘 지점)"
                );
                return Ok((final_status, url, jar));
            }
            // 리다이렉트: Location을 절대/상대 모두 처리해 다음 홉 URL로 삼는다.
            let Some(location) = response
                .headers()
                .get(LOCATION)
                .and_then(|value| value.to_str().ok())
                .map(ToOwned::to_owned)
            else {
                // 3xx인데 Location이 없으면 더 따라갈 수 없다 — 현재 URL을 최종으로 본다.
                return Ok((status.as_u16(), url, jar));
            };
            url = url::Url::parse(&url)
                .and_then(|base| base.join(&location))
                .map(|joined| joined.to_string())
                .unwrap_or(location);
        }
        // 리다이렉트 상한 초과 — 미완료로 판정되게 현재 URL을 돌려준다(status는 0으로 표시).
        Ok((0, url, jar))
    }

    // 약관/가입 페이지가 아닌 곳(가입 성공 콜백·토론 페이지)으로 리다이렉트됐으면 가입 완료로 본다.
    // 리다이렉트 추종 후의 최종 URL로 판정한다(member.pay.naver.com/.../agreement, financial-service/join
    // 에 머물러 있으면 미완료).

    // 네이버페이 가입(동의하기) GET — 톱레벨 내비게이션처럼 보이는 헤더를 만든다(JSON API 헤더와
    // 달리 ORIGIN/CORS가 아니라 sec-fetch navigate/document). 쿠키는 host 기준으로 .naver.com
    // 로그인 쿠키(NID_AUT/NID_SES)가 붙는다.
    fn navigation_headers(
        &self,
        cookies: &[NaverCookie],
        host: &str,
        referer: &str,
    ) -> AutomationResult<HeaderMap> {
        let mut headers = HeaderMap::new();
        headers.insert(USER_AGENT, header_value(&self.user_agent, "user-agent")?);
        headers.insert(
            COOKIE,
            header_value(&build_cookie_header(cookies, host), "cookie")?,
        );
        // accept / accept-language 는 실측 브라우저 요청과 정확히 일치시킨다(1:1 대조, 2026-07-01).
        headers.insert(
            ACCEPT,
            HeaderValue::from_static(
                "text/html,application/xhtml+xml,application/xml;q=0.9,image/avif,image/webp,image/apng,*/*;q=0.8,application/signed-exchange;v=b3;q=0.7",
            ),
        );
        headers.insert(
            ACCEPT_LANGUAGE,
            HeaderValue::from_static("ko-KR,ko;q=0.9,en-US;q=0.8,en;q=0.7,ja;q=0.6"),
        );
        // 브라우저는 약관 페이지에서 시작한 흐름이라 referer를 싣는다 — nid가 이 referer로 정상 흐름을
        // 확인한다. referer가 있으니 sec-fetch-site도 none이 아니라 same-site(같은 naver.com 서브도메인)다.
        headers.insert(REFERER, header_value(referer, "referer")?);
        headers.insert("sec-fetch-site", HeaderValue::from_static("same-site"));
        headers.insert("sec-fetch-mode", HeaderValue::from_static("navigate"));
        headers.insert("sec-fetch-dest", HeaderValue::from_static("document"));
        headers.insert("sec-fetch-user", HeaderValue::from_static("?1"));
        // 실측 브라우저 commonTermAgree 요청(frame 8567)의 **모든 client-hints를 하나도 빠짐없이** 채운다
        // (1:1 완전 대조, 2026-07-01). sec-ch-ua 3종은 insert_client_hints가, 나머지 고엔트로피/네트워크
        // 힌트는 아래에서 채운다. 하나라도 빠지면 봇 지문이 안 맞아 nid가 로그인 페이지로 튕긴다.
        self.insert_client_hints(&mut headers);
        let major = chrome_major_from_user_agent(&self.user_agent);
        if let Ok(v) = HeaderValue::from_str(&format!("\"{major}.0.0.0\"")) {
            headers.insert("sec-ch-ua-full-version", v);
        }
        if let Ok(v) = HeaderValue::from_str(&format!(
            "\"Google Chrome\";v=\"{major}.0.0.0\", \"Chromium\";v=\"{major}.0.0.0\", \"Not)A;Brand\";v=\"24.0.0.0\""
        )) {
            headers.insert("sec-ch-ua-full-version-list", v);
        }
        headers.insert("sec-ch-ua-arch", HeaderValue::from_static("\"x86\""));
        headers.insert(
            "sec-ch-ua-platform-version",
            HeaderValue::from_static("\"19.0.0\""),
        );
        headers.insert("sec-ch-ua-model", HeaderValue::from_static("\"\""));
        headers.insert("device-memory", HeaderValue::from_static("8"));
        headers.insert("dpr", HeaderValue::from_static("1"));
        headers.insert("viewport-width", HeaderValue::from_static("1920"));
        headers.insert("rtt", HeaderValue::from_static("50"));
        headers.insert("downlink", HeaderValue::from_static("10"));
        headers.insert("ect", HeaderValue::from_static("4g"));
        headers.insert("priority", HeaderValue::from_static("u=0, i"));
        headers.insert("upgrade-insecure-requests", HeaderValue::from_static("1"));
        Ok(headers)
    }

    // Rust HTTP 클라이언트로 글쓰기 form 패킷에서 txId를 받고 add 패킷으로 글을 등록하는 함수입니다.
    pub(super) fn submit_post(
        &self,
        page_url: &str,
        title: &str,
        body: &str,
    ) -> AutomationResult<String> {
        let target = discussion_target_from_url(page_url)?;
        let tx_id = self.issue_post_tx_id(page_url, &target)?;
        let payload = build_post_payload(title, body, &target, &tx_id);
        let response_text = self.post_with_retry(
            &format!("{M_STOCK_ORIGIN}/front-api/discussion/add"),
            self.json_headers(M_STOCK_HOST, DEFAULT_REFERER)?,
            Some(&payload),
            "글쓰기 add",
        )?;
        let value = parse_json(&response_text, "글쓰기 add")?;

        if !value
            .get("isSuccess")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            // 실제 네이버 API(/front-api/discussion/add)의 *실제 실패 응답*을 로그 파일에 그대로
            // 남긴다(사수 지시: 내가 만든 요약이 아니라 원본 API 성공/실패가 로그에 있어야 함).
            tracing::warn!(
                api = "POST /front-api/discussion/add",
                response = %log_snippet(&response_text),
                "글쓰기 add API 실패(isSuccess=false)"
            );
            return Err(AutomationError::new(format!(
                "글쓰기 add 패킷 API 실패: {}",
                packet_error_message(&value, &response_text)
            )));
        }

        let post_id = value
            .pointer("/result/id")
            .and_then(Value::as_i64)
            .map(|value| value.to_string())
            .or_else(|| {
                value
                    .pointer("/result/id")
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned)
            })
            .unwrap_or_default();
        // 실제 API가 글을 받았다는 *원본 성공 응답*을 로그에 남긴다(작성 글 id 포함).
        tracing::info!(
            api = "POST /front-api/discussion/add",
            post_id = %post_id,
            "글쓰기 add API 성공(isSuccess=true)"
        );
        Ok(post_id)
    }

    // 글쓰기 add 응답의 post_id를 현재 토론방 기준 토론글 URL로 바꾸는 함수입니다.
    pub(super) fn post_url_from_id(
        &self,
        page_url: &str,
        post_id: &str,
    ) -> AutomationResult<String> {
        if post_id.trim().is_empty() {
            return Err(AutomationError::new(
                "글쓰기 add 응답에서 작성 글 ID를 찾지 못했습니다.",
            ));
        }

        let target = discussion_target_from_url(page_url)?;

        Ok(discussion_url_for(
            &target.discussion_type,
            &target.item_code,
            Some(post_id),
        ))
    }

    // 기존 글의 제목·본문을 수정한다(PUT /front-api/discussion/edit?id=). 글쓰기 add와 달리 txId·form
    // 발급이 필요 없고 쿠키만 쓴다(패킷 실측 2026-07-09). 본문 contentJson(SE 문서)은 글쓰기와 완전히
    // 동일하게 만들어야 하므로 build_post_payload의 document 빌더를 그대로 재사용한다.
    pub(super) fn edit_post(
        &mut self,
        post_id: &str,
        title: &str,
        content_text: &str,
    ) -> AutomationResult<()> {
        // 글쓰기 add 본문을 만들어(target·txId는 edit body에 안 들어가므로 빈 값) 그 안의
        // contentJson.document만 뽑아 쓴다. 본문 SE 문서 구성 로직을 중복 없이 재사용하기 위함.
        let add_payload = build_post_payload(
            title,
            content_text,
            &DiscussionTarget {
                discussion_type: String::new(),
                item_code: String::new(),
            },
            "",
        );
        let document = add_payload
            .pointer("/contentJson/document")
            .cloned()
            .unwrap_or(Value::Null);
        let payload = json!({
            "title": title,
            "contentJson": { "document": document },
            "documentId": "",
            "isCleanbotDisabled": false,
            "danglingImages": [],
        });

        let response_text = self
            .client
            .put(format!(
                "{M_STOCK_ORIGIN}/front-api/discussion/edit?id={post_id}"
            ))
            .headers(self.json_headers(M_STOCK_HOST, DEFAULT_REFERER)?)
            .json(&payload)
            .send_traced(&self.client)
            .map_err(|error| AutomationError::new(format!("글 수정 edit 패킷 전송 실패: {error}")))
            .and_then(|response| response_text(response, "글 수정 edit"))?;
        let value = parse_json(&response_text, "글 수정 edit")?;

        if !value
            .get("isSuccess")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            // 글쓰기 add와 동일 스타일 — 네이버 원본 실패 응답을 그대로 로그에 남긴다.
            tracing::warn!(
                api = "PUT /front-api/discussion/edit",
                response = %log_snippet(&response_text),
                "글 수정 edit API 실패(isSuccess=false)"
            );
            return Err(AutomationError::new(format!(
                "글 수정 edit 패킷 API 실패: {}",
                packet_error_message(&value, &response_text)
            )));
        }

        tracing::info!(
            api = "PUT /front-api/discussion/edit",
            post_id,
            "글 수정 edit API 성공(isSuccess=true)"
        );
        Ok(())
    }

    // ---- 좋아요/싫어요(reactions) — 패킷 캡처(2026-07-01)로 재현 ----

    /// 게시글의 현재 반응 상태를 조회한다(`GET /posts/reactions?postIds=`). 내가 좋아요/싫어요를
    /// 눌러 뒀는지와 기존 reactionId를 돌려준다 — 최초면 POST, 있으면 PUT으로 분기하기 위함.
    pub(super) fn read_post_reaction(&self, post_id: &str) -> AutomationResult<PostReaction> {
        let path = format!("/api/community/discussion/posts/reactions?postIds={post_id}");
        let value = self.get_stock_json(&path, DEFAULT_REFERER, "반응 조회")?;
        // 응답은 배열(요청 postIds 수만큼). postId 1건만 물었으므로 첫 원소를 본다.
        let entry = value.as_array().and_then(|arr| arr.first());
        Ok(PostReaction {
            recommended: entry
                .and_then(|e| e.get("recommended"))
                .and_then(Value::as_bool)
                .unwrap_or(false),
            not_recommended: entry
                .and_then(|e| e.get("notRecommended"))
                .and_then(Value::as_bool)
                .unwrap_or(false),
            reaction_id: entry
                .and_then(|e| e.get("reactionId"))
                .and_then(Value::as_str)
                .filter(|value| !value.is_empty())
                .map(ToOwned::to_owned),
        })
    }

    /// 반응을 새로 생성한다(`POST /posts/{id}/reactions`). `reaction_type`: `"good"`=좋아요,
    /// `"bad"`=싫어요(패킷 캡처 값).
    fn create_reaction(&self, post_id: &str, reaction_type: &str) -> AutomationResult<()> {
        let url = format!("{STOCK_ORIGIN}/api/community/discussion/posts/{post_id}/reactions");
        let body = json!({ "reactionType": reaction_type });
        self.post_with_retry(
            &url,
            self.json_headers(STOCK_HOST, DEFAULT_REFERER)?,
            Some(&body),
            "반응 생성",
        )?;
        Ok(())
    }

    /// 기존 반응을 변경한다(`PUT /posts/{id}/reactions/{reactionId}`). 좋아요↔싫어요 전환이며,
    /// 실동작상 **마지막 누른 상태**가 글에 표시된다.
    fn update_reaction(
        &self,
        post_id: &str,
        reaction_id: &str,
        reaction_type: &str,
    ) -> AutomationResult<()> {
        let url = format!(
            "{STOCK_ORIGIN}/api/community/discussion/posts/{post_id}/reactions/{reaction_id}"
        );
        let body = json!({ "reactionType": reaction_type });
        self.put_with_retry(
            &url,
            self.json_headers(STOCK_HOST, DEFAULT_REFERER)?,
            &body,
            "반응 변경",
        )?;
        Ok(())
    }

    /// 게시글 URL에 반응을 남긴다 — **좋아요(reaction_type="good")·싫어요("bad") 공용**.
    /// 패킷 실측(2026-07-01)상 URL·메서드·헤더가 전부 동일하고 요청 바디의 `reactionType`만 다르므로
    /// 좋아요/싫어요가 이 함수 하나를 공유한다. 이미 같은 반응이면 멱등 성공, 무반응이면 최초=POST,
    /// 다른 반응이 있으면=PUT으로 전환(마지막 누른 상태가 표시된다). postId는 기존
    /// [`object_id_from_url`]로 파싱(댓글 경로와 동일 규칙 재사용).
    pub(super) fn react_post(&self, post_url: &str, reaction_type: &str) -> AutomationResult<()> {
        let is_dislike = reaction_type == "bad";
        let label = if is_dislike { "싫어요" } else { "좋아요" };
        let post_id = object_id_from_url(post_url)?;
        let current = self.read_post_reaction(&post_id)?;
        let already = if is_dislike {
            current.not_recommended
        } else {
            current.recommended
        };
        if already {
            tracing::info!(post_id = %post_id, "이미 {label} 상태 — 건너뜀");
            return Ok(());
        }
        match current.reaction_id {
            Some(reaction_id) => self.update_reaction(&post_id, &reaction_id, reaction_type)?,
            None => self.create_reaction(&post_id, reaction_type)?,
        }
        tracing::info!(post_id = %post_id, "{label} 완료");
        Ok(())
    }

    /// 좋아요가 막힌 계정의 **제재 상태를 원문으로 확정**한다(사용자 요청 2026-07-03: 추측 말고
    /// 네이버가 실제로 뱉은 원문으로 차단/만료를 가른다). `/api/community/profile/users/form`을 저장
    /// 쿠키만으로 GET해 응답 **status·body 전체를 자르지 않고** 로그에 남기고, 그 원문으로 판정한다.
    /// (실측 07-02: 차단 계정은 이 form이 403 `UMON_*_BANNED`, 정상은 200 `status:existent`.)
    pub(super) fn probe_restriction_raw(&self) -> RestrictionVerdict {
        const PATH: &str = "/api/community/profile/users/form";
        tracing::info!(
            api = "GET /api/community/profile/users/form",
            "실제 API 호출 label=\"좋아요-제재확인 프로필 form\""
        );
        let headers = match self.stock_get_headers(STOCK_HOST, DEFAULT_REFERER) {
            Ok(headers) => headers,
            Err(error) => {
                return RestrictionVerdict::Unknown(format!("form 헤더 생성 실패: {error}"))
            }
        };
        let response = match self
            .client
            .get(format!("{STOCK_ORIGIN}{PATH}"))
            .headers(headers)
            .send_traced(&self.client)
        {
            Ok(response) => response,
            Err(error) => {
                tracing::warn!(error = %error, "좋아요-제재확인 form 전송 실패");
                return RestrictionVerdict::Unknown(format!("form 전송 실패: {error}"));
            }
        };
        let status = response.status().as_u16();
        let body = response.text().unwrap_or_default();
        // 원문 전체를 자르지 않고 그대로 남긴다(사용자 지시: 원문으로 판단).
        tracing::warn!(
            status,
            body = %body,
            "[좋아요][원문] 프로필 form 응답 전체(제재 판정용, 자르지 않음)"
        );
        classify_restriction_body(status, &body)
    }

    // stock.naver.com JSON API를 공통 헤더로 호출하고 JSON으로 파싱하는 함수입니다.
    fn get_stock_json(&self, path: &str, referer: &str, label: &str) -> AutomationResult<Value> {
        // 실제로 어떤 API를 호출하는지 경로째 로그에 남긴다(사수 지시: 실제 API 호출이 보여야 함).
        tracing::info!(label, api = %format!("GET {path}"), "실제 API 호출");
        let response_text = self
            .client
            .get(format!("{STOCK_ORIGIN}{path}"))
            .headers(self.stock_get_headers(STOCK_HOST, referer)?)
            .send_traced(&self.client)
            .map_err(|error| {
                // 응답 자체가 오지 않은 전송 계층 실패(연결 끊김·타임아웃 등)도 그대로 남긴다.
                tracing::warn!(label, api = %format!("GET {path}"), error = %error, "실제 API 전송 실패");
                AutomationError::new(format!("{label} GET 패킷 전송 실패: {error}"))
            })
            .and_then(|response| response_text(response, label))?;

        parse_json(&response_text, label)
    }

    /// create-500(Failed to create profile user) 뒤 **실명인증 필요**를 확정하는 확인 호출(2026-07-03).
    /// 실명인증 미완 계정은 브라우저가 nid `realNameCheck`(실명확인 폼)로 튕긴다(실측 패킷). 우리도 그
    /// URL을 쿠키 달아 GET해서, 응답 본문이 실명확인 폼(실명확인·본인확인·certify·realNameCheck 마커)이면
    /// true. best-effort — 전송실패·불명확하면 false(오탐 방지). 게시 흐름은 안 바꾸고 확인만 한다.
    fn real_name_verification_required(&self) -> bool {
        let mut headers = match self.base_headers(NID_HOST, NAVER_HOME_REFERER, "same-site", false)
        {
            Ok(h) => h,
            Err(_) => return false,
        };
        headers.insert(
            ACCEPT,
            HeaderValue::from_static("text/html,application/xhtml+xml,*/*"),
        );
        tracing::info!(
            api = "GET /user2/help/realNameCheck.nhn",
            "실명인증 확인 호출(create-500 후)"
        );
        let resp = match self
            .client
            .get(REAL_NAME_CHECK_URL)
            .headers(headers)
            .send_traced(&self.client)
        {
            Ok(r) => r,
            Err(_) => return false,
        };
        let final_url = resp.url().as_str().to_ascii_lowercase();
        let body = resp.text().unwrap_or_default();
        // 실명확인 폼 마커가 본문에 있으면 실명인증 필요. 인증된 계정이면 이 폼 대신 다른 페이지로 가서
        // 마커가 없다. (응답 원문 캡처 미확보라 실기기 검증 필요 — 안 맞으면 마커만 조정.)
        let hit = body.contains("실명확인")
            || body.contains("본인확인")
            || body.contains("실명인증")
            || body.contains("realNameCheck")
            || body.contains("certify");
        tracing::info!(real_name_required = hit, final_url = %final_url, "실명인증 확인 결과(realNameCheck)");
        hit
    }

    // 프로필 상태 조회를 짧은 백오프로 재시도한다. npay 가입 직후 네이버 서버 반영 지연으로 나는
    // 일시적 500("Failed to fetch profile user status")을 넘기기 위함 — 가입은 됐는데 조회만 아직
    // 500인 상황(실측 2026-07-01 yeajuyun). 성공하면 즉시 반환하고, 끝까지 실패하면 마지막 오류를 낸다.
    fn read_profile_status_with_retry(&self, referer: &str) -> AutomationResult<Value> {
        const MAX_ATTEMPTS: u32 = 5;
        const BACKOFF: Duration = Duration::from_millis(500);
        // [진단·1:1] 프로필 status에 실제로 실어보내는 세션 쿠키(NID_AUT/NID_SES) 앞부분을 찍는다.
        // npay가 회전시킨 새 토큰이 반영됐는지 캡쳐(성공본: NID_AUT=lBTtey…, NID_SES=AAABjb01…)와
        // 대조하기 위함. 500이 계속 나면 여기 값이 npay-후 값인지 로그인-시점 옛 값인지로 원인이 갈린다.
        // 프로필 status에 실제로 나가는 쿠키를 전부(stock.naver.com 적용분, 값 앞 8자) 찍는다 —
        // 성공 캡쳐의 status 요청 쿠키(NID_AUT/NID_SES/BUC + NNB/NAC/nid_inf…)와 1:1로 대조하기 위함.
        // 회전 토큰이 반영됐는지, 캡쳐에 있던 쿠키 중 우리가 빠뜨린 게 있는지 한 줄로 드러난다.
        let stock_cookies: Vec<String> = self
            .cookies
            .iter()
            .filter(|c| cookie_applies_to_host(&c.domain, STOCK_HOST))
            .map(|c| format!("{}={}", c.name, c.value.chars().take(8).collect::<String>()))
            .collect();
        tracing::info!(
            cookies = %stock_cookies.join(" "),
            "[진단] 프로필 status 요청 쿠키 전체(stock.naver.com 적용분, 값 앞 8자)"
        );
        let mut last_err: Option<AutomationError> = None;
        for attempt in 1..=MAX_ATTEMPTS {
            match self.get_stock_json(
                "/api/community/profile/users/status",
                referer,
                "프로필 상태",
            ) {
                Ok(value) => {
                    if attempt > 1 {
                        tracing::info!(
                            attempt,
                            "프로필 상태 조회 재시도 성공 — npay 가입 서버 반영 확인"
                        );
                    }
                    return Ok(value);
                }
                Err(error) => {
                    tracing::warn!(
                        attempt,
                        max = MAX_ATTEMPTS,
                        error = %error.message(),
                        "프로필 상태 조회 실패 — 백오프 후 재시도(npay 가입 반영 대기)"
                    );
                    last_err = Some(error);
                    if attempt < MAX_ATTEMPTS {
                        std::thread::sleep(BACKOFF);
                    }
                }
            }
        }
        Err(last_err.unwrap_or_else(|| AutomationError::new("프로필 상태 조회 실패(재시도 소진)")))
    }

    // 프로필 form에 nickname이 없을 때 네이버 추천 닉네임 패킷을 호출하는 함수입니다.
    fn recommend_profile_nickname(&self, referer: &str) -> AutomationResult<String> {
        let response_text = self
            .client
            .post(format!(
                "{STOCK_ORIGIN}/api/community/profile/users/nickname/recommend"
            ))
            .headers(self.stock_json_headers(STOCK_HOST, referer)?)
            .json(&json!({ "unusedNickname": "" }))
            .send_traced(&self.client)
            .map_err(|error| AutomationError::new(format!("닉네임 추천 패킷 전송 실패: {error}")))
            .and_then(|response| response_text(response, "닉네임 추천"))?;
        let value = parse_json(&response_text, "닉네임 추천")?;

        value
            .get("recommendedNickname")
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .map(ToOwned::to_owned)
            .ok_or_else(|| {
                AutomationError::new("닉네임 추천 응답에서 recommendedNickname을 찾지 못했습니다.")
            })
    }

    // 추천 닉네임을 받아 `used`에 없는 새 닉네임으로 프로필 닉네임을 바꾼다(계정 안에서 댓글마다 다른
    // 닉네임을 쓰기 위한 중복 회피). 추천이 겹치면 최대 NICKNAME_RETRY_MAX회 재추천하고, 다 겹치면
    // 마지막 추천값을 그대로 쓴다(warn 로그). 성공 시 고른 닉네임을 돌려주므로 호출부가 used에 넣어
    // 다음 호출이 또 다른 닉네임을 고르게 한다. 소개·이미지 등 나머지 필드는 기존 프로필 값을 유지한다.
    pub(super) fn change_nickname_avoiding(
        &mut self,
        used: &std::collections::HashSet<String>,
    ) -> AutomationResult<String> {
        const NICKNAME_RETRY_MAX: usize = 10;
        let referer = DEFAULT_REFERER;

        // used에 없는 추천이 나오면 즉시 멈춘다(불필요한 추천 호출 방지). 다 겹치면 후보 전체를
        // choose_unused_nickname에 넘겨 마지막 값을 고르게 한다.
        let mut candidates: Vec<String> = Vec::new();
        for _ in 0..NICKNAME_RETRY_MAX {
            let candidate = self.recommend_profile_nickname(referer)?;
            let is_unused = !used.contains(&candidate);
            candidates.push(candidate);
            if is_unused {
                break;
            }
        }
        let nickname = choose_unused_nickname(&candidates, used).ok_or_else(|| {
            AutomationError::new("닉네임 추천을 한 번도 받지 못해 닉네임을 바꿀 수 없습니다.")
        })?;
        if used.contains(&nickname) {
            tracing::warn!(
                max = NICKNAME_RETRY_MAX,
                nickname = %nickname,
                "닉네임 중복 회피 실패 — 마지막 추천 닉네임을 그대로 사용"
            );
        }

        // profileId는 프로필 상태 응답에서 얻는다(프로필 생성/PUT 경로가 profileId를 얻는 방식과 동일).
        let status = self.read_profile_status_with_retry(STOCK_ROOT_REFERER)?;
        let profile_id = status
            .get("profileId")
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| {
                AutomationError::new("프로필 상태에 profileId가 없어 닉네임을 바꿀 수 없습니다.")
            })?
            .to_owned();

        // 닉네임만 교체하고 소개·이미지는 기존 form 값을 유지한다(프로필 PUT 경로와 동일 필드).
        let form =
            self.get_stock_json("/api/community/profile/users/form", referer, "프로필 form")?;
        let introduction = form
            .get("introduction")
            .cloned()
            .unwrap_or_else(|| Value::String(DEFAULT_PROFILE_INTRODUCTION.to_owned()));
        let image_url = form.get("imageUrl").cloned().unwrap_or(Value::Null);

        let payload = json!({
            "nickname": nickname,
            "introduction": introduction,
            "imageUrl": image_url,
            "danglingImages": [],
        });
        self.client
            .put(format!(
                "{STOCK_ORIGIN}/api/community/profile/users/{profile_id}"
            ))
            .headers(self.stock_json_headers(STOCK_HOST, referer)?)
            .json(&payload)
            .send_traced(&self.client)
            .map_err(|error| {
                AutomationError::new(format!("닉네임 변경 PUT 패킷 전송 실패: {error}"))
            })
            .and_then(|response| response_text(response, "닉네임 변경 PUT"))?;

        Ok(nickname)
    }

    /// 닉네임 변경 잔여 횟수(remainingEditCount)를 조회한다(설계서 §2, 닉네임 랜덤 UI용). 프로필
    /// form 응답의 `remainingEditCount`(5회 상한 중 남은 횟수)를 그대로 돌려준다. 필드가 없으면
    /// None. change_nickname_avoiding이 쓰는 것과 같은 form GET을 재사용한다.
    pub(super) fn nickname_remaining_edit_count(&self) -> AutomationResult<Option<i64>> {
        let form = self.get_stock_json(
            "/api/community/profile/users/form",
            DEFAULT_REFERER,
            "프로필 form(닉네임 잔여 횟수)",
        )?;
        Ok(form.get("remainingEditCount").and_then(Value::as_i64))
    }

    // 프로필 소개 2222가 저장 가능한 값인지 검증 패킷으로 확인하는 함수입니다.
    fn validate_profile_introduction(&self, referer: &str) -> AutomationResult<()> {
        let response_text = self
            .client
            .post(format!(
                "{STOCK_ORIGIN}/api/community/profile/users/introduction/validate"
            ))
            .headers(self.stock_json_headers(STOCK_HOST, referer)?)
            .json(&json!({ "targetValue": DEFAULT_PROFILE_INTRODUCTION }))
            .send_traced(&self.client)
            .map_err(|error| {
                AutomationError::new(format!("프로필 소개 검증 패킷 전송 실패: {error}"))
            })
            .and_then(|response| response_text(response, "프로필 소개 검증"))?;
        let value = parse_json(&response_text, "프로필 소개 검증")?;

        if value
            .get("isValid")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            return Ok(());
        }

        Err(AutomationError::new(format!(
            "프로필 소개 2222 검증 실패: {}",
            packet_error_message(&value, &response_text)
        )))
    }

    // Rust HTTP 클라이언트로 cbox 토큰 발급 패킷과 댓글 생성 패킷을 차례대로 호출하는 함수입니다.
    pub(super) fn submit_comment(&self, page_url: &str, body: &str) -> AutomationResult<String> {
        let object_id = object_id_from_url(page_url)?;
        let object_url = page_url.split('#').next().unwrap_or(page_url);
        let cbox_token = self.issue_cbox_token(&object_id, object_url, page_url)?;
        let form_body = build_comment_form(&object_id, object_url, body, &cbox_token);
        let response_text = self
            .client
            .post(format!(
                "{CBOX_ORIGIN}/commentBox/cbox/web_naver_create_json.json?ticket=finance&templateId=community&pool=cbox12&_cv="
            ))
            .headers(self.form_headers(CBOX_HOST, page_url)?)
            .body(form_body)
            .send_traced(&self.client)
            .map_err(|error| AutomationError::new(format!("댓글 생성 패킷 전송 실패: {error}")))?
            .error_for_status()
            .map_err(|error| AutomationError::new(format!("댓글 생성 패킷 HTTP 실패: {error}")))?
            .text()
            .map_err(|error| AutomationError::new(format!("댓글 생성 응답 읽기 실패: {error}")))?;
        let value = parse_json(&response_text, "댓글 생성")?;
        let created = value
            .get("success")
            .and_then(Value::as_bool)
            .unwrap_or(false)
            || value.pointer("/result/comment").is_some()
            || value.pointer("/result/commentList").is_some();

        if !created {
            // 실제 cbox 댓글 생성 API의 *원본 실패 응답*을 로그에 남긴다(사수 지시).
            tracing::warn!(
                api = "POST cbox web_naver_create_json",
                response = %log_snippet(&response_text),
                "댓글 생성 API 실패(success=false)"
            );
            return Err(AutomationError::new(format!(
                "댓글 생성 패킷 API 실패: {}",
                packet_error_message(&value, &response_text)
            )));
        }

        tracing::info!(
            api = "POST cbox web_naver_create_json",
            "댓글 생성 API 성공"
        );
        Ok(value
            .pointer("/result/comment/commentNo")
            .and_then(Value::as_i64)
            .map(|value| value.to_string())
            .or_else(|| {
                value
                    .pointer("/result/comment/commentNo")
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned)
            })
            .unwrap_or_default())
    }

    // 글쓰기 add 패킷에 필요한 txId를 form 패킷으로 발급받는 함수입니다.
    // page_url은 시그니처 일관성(issue_cbox_token 등과 동일 호출부)상 받지만, form 요청은
    // DEFAULT_REFERER를 쓰므로 이 함수에선 사용하지 않는다(의도적 미사용 → `_` 접두).
    fn issue_post_tx_id(
        &self,
        _page_url: &str,
        target: &DiscussionTarget,
    ) -> AutomationResult<String> {
        let form_url = format!(
            "{M_STOCK_ORIGIN}/front-api/discussion/form?discussionType={}&itemCode={}",
            target.discussion_type, target.item_code
        );
        let response_text = self.post_with_retry(
            &form_url,
            self.json_headers(M_STOCK_HOST, DEFAULT_REFERER)?,
            None,
            "글쓰기 form",
        )?;
        let value = parse_json(&response_text, "글쓰기 form")?;

        if !value
            .get("isSuccess")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            return Err(AutomationError::new(format!(
                "글쓰기 form 패킷 API 실패: {}",
                packet_error_message(&value, &response_text)
            )));
        }

        value
            .pointer("/result/txId")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned)
            .ok_or_else(|| AutomationError::new("글쓰기 form 응답에서 txId를 찾지 못했습니다."))
    }

    // 댓글 생성에 필요한 cbox_token을 토큰 발급 패킷으로 가져오는 함수입니다.
    fn issue_cbox_token(
        &self,
        object_id: &str,
        object_url: &str,
        page_url: &str,
    ) -> AutomationResult<String> {
        let query = Serializer::new(String::new())
            .append_pair("ticket", "finance")
            .append_pair("templateId", "community")
            .append_pair("pool", "cbox12")
            .append_pair("_cv", "")
            .append_pair("lang", "ko")
            .append_pair("pageType", "more")
            .append_pair("country", "")
            .append_pair("objectId", object_id)
            .append_pair("categoryId", "")
            .append_pair("pageSize", "10")
            .append_pair("indexSize", "10")
            .append_pair("groupId", "")
            .append_pair("listType", "OBJECT")
            .append_pair("clientType", "web-pc")
            .append_pair("objectUrl", object_url)
            .finish();
        let response_text = self
            .client
            .get(format!(
                "{CBOX_ORIGIN}/commentBox/cbox/web_naver_token_json.json?{query}"
            ))
            .headers(self.json_headers(CBOX_HOST, page_url)?)
            .send_traced(&self.client)
            .map_err(|error| AutomationError::new(format!("댓글 토큰 패킷 전송 실패: {error}")))?
            .error_for_status()
            .map_err(|error| AutomationError::new(format!("댓글 토큰 패킷 HTTP 실패: {error}")))?
            .text()
            .map_err(|error| AutomationError::new(format!("댓글 토큰 응답 읽기 실패: {error}")))?;
        let value = parse_json(&response_text, "댓글 토큰")?;

        value
            .pointer("/result/cbox_token")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned)
            .ok_or_else(|| {
                AutomationError::new(format!(
                    "댓글 토큰 응답에서 cbox_token을 찾지 못했습니다: {}",
                    packet_error_message(&value, &response_text)
                ))
            })
    }

    // 대상 호스트에 적용되는 쿠키만 골라 Cookie 헤더를 만드는 함수입니다.
    fn cookie_header_for(&self, host: &str) -> String {
        build_cookie_header(&self.cookies, host)
    }

    // m.stock.naver.com JSON 요청에 사용하는 공통 헤더를 만드는 함수입니다.
    fn json_headers(&self, host: &str, referer: &str) -> AutomationResult<HeaderMap> {
        let mut headers = self.base_headers(host, referer, "same-site", true)?;
        headers.insert(
            ACCEPT,
            HeaderValue::from_static("application/json, text/plain, */*"),
        );
        Ok(headers)
    }

    // stock.naver.com JSON API 요청에 사용하는 공통 헤더를 만드는 함수입니다.
    fn stock_json_headers(&self, host: &str, referer: &str) -> AutomationResult<HeaderMap> {
        let mut headers = self.base_headers(host, referer, "same-origin", true)?;
        headers.insert(ACCEPT, HeaderValue::from_static("*/*"));
        Ok(headers)
    }

    /// stock.naver.com **GET** 조회(프로필 상태/폼 등)용 헤더 — POST와 달리 Origin을 붙이지 않는다.
    /// 브라우저는 같은 출처 GET에 Origin을 안 보내며(실측 패킷), 우리가 붙이면 봇탐지로 403/500난다.
    fn stock_get_headers(&self, host: &str, referer: &str) -> AutomationResult<HeaderMap> {
        let mut headers = self.base_headers(host, referer, "same-origin", false)?;
        headers.insert(ACCEPT, HeaderValue::from_static("*/*"));
        Ok(headers)
    }

    // static.nid.naver.com getProfile 요청 헤더. 실측 브라우저와 1:1: JSONP <script> 로드라 origin을
    // 안 보내고 sec-fetch-mode=no-cors·dest=script이며 priority는 안 붙는다(다른 XHR과 다름).
    fn static_headers(&self, host: &str, referer: &str) -> AutomationResult<HeaderMap> {
        let mut headers = self.base_headers(host, referer, "same-site", false)?;
        headers.insert(ACCEPT, HeaderValue::from_static("*/*"));
        headers.insert("sec-fetch-mode", HeaderValue::from_static("no-cors"));
        headers.insert("sec-fetch-dest", HeaderValue::from_static("script"));
        headers.remove("priority");
        Ok(headers)
    }

    // apis.naver.com 댓글 form-urlencoded 요청에 사용하는 공통 헤더를 만드는 함수입니다.
    fn form_headers(&self, host: &str, referer: &str) -> AutomationResult<HeaderMap> {
        let mut headers = self.json_headers(host, referer)?;
        headers.insert(
            CONTENT_TYPE,
            HeaderValue::from_static("application/x-www-form-urlencoded; charset=UTF-8"),
        );
        Ok(headers)
    }

    // User-Agent, Cookie, Referer 등 패킷 재현에 공통으로 필요한 헤더를 조립하는 함수입니다.
    fn base_headers(
        &self,
        host: &str,
        referer: &str,
        sec_fetch_site: &'static str,
        send_origin: bool,
    ) -> AutomationResult<HeaderMap> {
        let mut headers = HeaderMap::new();
        // Origin은 쓰기(POST/PUT)·CORS 요청에만 붙인다. 브라우저는 같은 출처 GET(프로필 상태/폼 조회
        // 등)에는 Origin을 **안 보내는데**(실측 패킷 `프로필 생성하기 전`), 우리가 GET에도 Origin을
        // 붙이면 네이버 봇탐지(UMON)가 비정상으로 보고 403 UMON_*_BANNED(가짜 밴)를 준다 — 계정은
        // 멀쩡한데(lhs**** 브라우저 200) 우리만 막혔다(실측 2026-07-01).
        if send_origin {
            headers.insert(ORIGIN, HeaderValue::from_static(STOCK_ORIGIN));
        }
        headers.insert(REFERER, header_value(referer, "referer")?);
        headers.insert(USER_AGENT, header_value(&self.user_agent, "user-agent")?);
        headers.insert(
            COOKIE,
            header_value(&self.cookie_header_for(host), "cookie")?,
        );
        headers.insert(
            ACCEPT_LANGUAGE,
            HeaderValue::from_static("ko-KR,ko;q=0.9,en-US;q=0.8,en;q=0.7,ja;q=0.6"),
        );
        headers.insert("sec-fetch-site", HeaderValue::from_static(sec_fetch_site));
        headers.insert("sec-fetch-mode", HeaderValue::from_static("cors"));
        headers.insert("sec-fetch-dest", HeaderValue::from_static("empty"));
        // 브라우저는 모든 요청에 client-hints(sec-ch-ua*)를 보낸다. 우리가 빠뜨리면 봇으로 탐지돼
        // 403/500이 난다(실측: 우리 403 UMON_BANNED·프로필 500 ↔ 브라우저 200). UA 버전과 맞춰 붙인다.
        self.insert_client_hints(&mut headers);
        headers.insert("priority", HeaderValue::from_static("u=1, i"));
        Ok(headers)
    }

    /// 브라우저가 모든 요청에 붙이는 client-hints(sec-ch-ua 계열)를 넣는다. 값은 UA의 Chrome 메이저
    /// 버전과 일치시킨다(UA와 sec-ch-ua 버전 불일치도 봇 신호라 실제 UA에서 뽑는다).
    fn insert_client_hints(&self, headers: &mut HeaderMap) {
        if let Ok(value) = HeaderValue::from_str(&sec_ch_ua_from_user_agent(&self.user_agent)) {
            headers.insert("sec-ch-ua", value);
        }
        headers.insert("sec-ch-ua-mobile", HeaderValue::from_static("?0"));
        headers.insert(
            "sec-ch-ua-platform",
            HeaderValue::from_static("\"Windows\""),
        );
    }

    // 429(Too Many Requests)·5xx 같은 일시적 실패에 지수 백오프로 재시도하며 POST를 보낸다.
    // 다종목 연속 게시 때 글쓰기 form(txId)·add 엔드포인트가 간헐 429를 반환해, 무재시도로
    // 일부 종목만 실패하던 문제를 막는다. 성공 시 응답 본문(text)을 돌려준다.
    fn post_with_retry(
        &self,
        url: &str,
        headers: HeaderMap,
        json_body: Option<&Value>,
        label: &str,
    ) -> AutomationResult<String> {
        let mut attempt: u32 = 0;
        loop {
            attempt += 1;
            let mut builder = self.client.post(url).headers(headers.clone());
            if let Some(body) = json_body {
                builder = builder.json(body);
            }
            // 실제 HTTP 호출 1건의 소요시간을 잰다 — "게시 시작까지 N초"·"즉시 대기초과"의
            // 진짜 원인이 어느 단계인지 로그로 드러내기 위함(사수 지적).
            let started = Instant::now();
            let response = builder.send_traced(&self.client).map_err(|error| {
                tracing::warn!(label, attempt, error = %error, "패킷 전송 실패(전송 계층)");
                AutomationError::new(format!("{label} 패킷 전송 실패: {error}"))
            })?;
            let status = response.status();
            let elapsed_ms = started.elapsed().as_millis();
            if status.is_success() {
                // 성공도 네이버 원본 응답 body를 그대로 남긴다(사용자·사수 지시: 성공/실패 전부 원문).
                let text = response.text().map_err(|error| {
                    AutomationError::new(format!("{label} 응답 읽기 실패: {error}"))
                })?;
                tracing::info!(
                    label,
                    status = status.as_u16(),
                    elapsed_ms,
                    body = %log_snippet(&text),
                    "패킷 HTTP 응답 OK — 네이버 원문"
                );
                return Ok(text);
            }
            if is_retryable_status(status.as_u16()) && attempt < POST_RETRY_MAX_ATTEMPTS {
                let delay =
                    parse_retry_after(response.headers()).unwrap_or_else(|| backoff_delay(attempt));
                tracing::warn!(
                    label,
                    status = status.as_u16(),
                    attempt,
                    elapsed_ms,
                    retry_after_secs = delay.as_secs(),
                    "패킷 HTTP 일시 실패 — 재시도 예정"
                );
                std::thread::sleep(delay);
                continue;
            }
            // 원본 API 호출의 *실제 실패*(최종)를 상태 + 네이버 원본 body와 함께 남긴다(원문 그대로).
            let body = response.text().unwrap_or_default();
            tracing::warn!(
                label,
                status = status.as_u16(),
                attempt,
                elapsed_ms,
                body = %log_snippet(&body),
                "패킷 HTTP 최종 실패 — 네이버 원문"
            );
            return Err(AutomationError::new(format!(
                "{label} 패킷 HTTP 실패: HTTP status {status} for url ({url}) body={}",
                log_snippet(&body)
            )));
        }
    }

    // 반응 변경(PUT)처럼 기존 리소스를 갱신하는 JSON 요청을 보낸다. post_with_retry와 같은 429·5xx
    // 백오프 재시도 정책을 쓰되 메서드만 PUT이다(좋아요↔싫어요 전환용). 실제 API 호출 결과를
    // 로그로 남겨, 좋아요가 어느 계정에서 무슨 status로 처리/실패했는지 로그 파일에서 확인케 한다.
    fn put_with_retry(
        &self,
        url: &str,
        headers: HeaderMap,
        json_body: &Value,
        label: &str,
    ) -> AutomationResult<String> {
        let mut attempt: u32 = 0;
        loop {
            attempt += 1;
            let response = self
                .client
                .put(url)
                .headers(headers.clone())
                .json(json_body)
                .send_traced(&self.client)
                .map_err(|error| {
                    AutomationError::new(format!("{label} 패킷 전송 실패: {error}"))
                })?;
            let status = response.status();
            if status.is_success() {
                // 성공도 네이버 원본 body 그대로(좋아요/싫어요 전환 결과).
                let text = response.text().map_err(|error| {
                    AutomationError::new(format!("{label} 응답 읽기 실패: {error}"))
                })?;
                tracing::info!(
                    label,
                    status = status.as_u16(),
                    body = %log_snippet(&text),
                    "반응 API 응답 OK — 네이버 원문"
                );
                return Ok(text);
            }
            if is_retryable_status(status.as_u16()) && attempt < POST_RETRY_MAX_ATTEMPTS {
                let delay =
                    parse_retry_after(response.headers()).unwrap_or_else(|| backoff_delay(attempt));
                std::thread::sleep(delay);
                continue;
            }
            let body = response.text().unwrap_or_default();
            tracing::warn!(
                label,
                status = status.as_u16(),
                attempt,
                body = %log_snippet(&body),
                "반응 API HTTP 실패 — 네이버 원문"
            );
            return Err(AutomationError::new(format!(
                "{label} 패킷 HTTP 실패: HTTP status {status} for url ({url}) body={}",
                log_snippet(&body)
            )));
        }
    }

    // 전송 계층(연결/타임아웃) 실패를 짧은 백오프로 재시도하며 GET을 보낸다. IP 교체 직후 등
    // 일시적 망 끊김이 흐름의 첫 HTTP 호출(getProfile)을 그대로 터뜨려 게시 전체가 실패하던 것을
    // 막는다(#330 후속). 응답이 도착하면(HTTP 상태 무관) 그대로 돌려주고, 상태 단계 실패는
    // 호출부/response_text가 다룬다. 마지막까지 전송이 실패하면 describe_reqwest_error로 source
    // 체인(연결 거부/타임아웃/DNS)까지 드러낸 메시지를 만들어 원인 진단이 가능하게 한다.
    fn get_with_transport_retry(
        &self,
        url: &str,
        headers: HeaderMap,
        label: &str,
    ) -> AutomationResult<reqwest::blocking::Response> {
        retry_transient(
            TRANSPORT_RETRY_MAX_ATTEMPTS,
            || {
                self.client
                    .get(url)
                    .headers(headers.clone())
                    .send_traced(&self.client)
            },
            |error| is_retryable_transport_kind(crate::util::reqwest_kind(error)),
            |attempt| std::thread::sleep(transport_backoff_delay(attempt)),
        )
        .map_err(|error| {
            AutomationError::new(format!(
                "{label} 패킷 전송 실패: {}",
                crate::util::describe_reqwest_error(&error)
            ))
        })
    }
}

/// 게시(패킷) HTTP 와이어 트레이스 on/off. **기본 ON** — CDP 로그인 트레이스(`PSTMACRO_CDP_TRACE`)와
/// 같은 규약이다. 빌드만 하면 네이버로 나가는 모든 요청(메서드·URL·헤더·바디)과 응답(상태·헤더)이
/// 원문 그대로 로그에 남는다. 응답 바디는 각 호출부가 이미 `body={...}`로 남기므로(요청 원문 + 응답
/// 헤더 + 기존 응답 바디 = 100% raw), 여기선 중복해서 읽지 않는다. 끄려면 환경변수
/// `PSTMACRO_PACKET_TRACE=0`(또는 `false`/`off`).
pub(crate) fn packet_trace_enabled() -> bool {
    match std::env::var("PSTMACRO_PACKET_TRACE") {
        Ok(v) => {
            let v = v.trim();
            !(v == "0" || v.eq_ignore_ascii_case("false") || v.eq_ignore_ascii_case("off"))
        }
        // 미설정 = 기본 ON. 빌드만 하면 원문 트레이스가 나온다(CDP 트레이스와 동일 규약).
        Err(_) => true,
    }
}

/// `prefix` 바로 뒤부터 `terminator`(또는 문자열 끝) 전까지를 `•` 하나로 치환한다. `prefix` 앞 글자가
/// 영숫자면(다른 키의 꼬리) 건너뛴다 — `pw=` 가 `xpw=` 안에서 오검출되는 것을 막는다.
fn redact_pattern(input: &str, prefix: &str, terminator: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut rest = input;
    while let Some(pos) = rest.find(prefix) {
        let before = &rest[..pos];
        let after = &rest[pos + prefix.len()..];
        let boundary_ok = before
            .chars()
            .last()
            .map_or(true, |c| !c.is_ascii_alphanumeric());
        out.push_str(before);
        out.push_str(prefix);
        if boundary_ok {
            let end = after.find(terminator).unwrap_or(after.len());
            out.push('•');
            rest = &after[end..];
        } else {
            rest = after;
        }
    }
    out.push_str(rest);
    out
}

/// 로그에 남기기 전, 로그인류 자격 비밀값만 `•` 로 가린다 — `eccpw`/`password`/`pw` 필드의 값만.
/// form/query(`key=value`)·json(`"key":"value"`) 두 형태를 다룬다. **쿠키(Cookie/Set-Cookie)는
/// 가리지 않는다**(호출부가 쿠키 헤더엔 이 함수를 적용하지 않음) — 사용자가 raw 를 원하고 쿠키로
/// 디버그하기 때문. 게시 경로엔 비밀번호가 없어 실제론 거의 발동하지 않는 방어용이다.
fn redact_credentials(input: &str) -> String {
    const KEYS: [&str; 3] = ["eccpw", "password", "pw"];
    let mut out = input.to_owned();
    for key in KEYS {
        out = redact_pattern(&out, &format!("\"{key}\":\""), "\"");
        out = redact_pattern(&out, &format!("{key}="), "&");
    }
    out
}

/// 나가는 요청 하나를 원문 그대로 `target: "packet"`(CDP 트레이스의 `"cdp"` 와 짝) 에 남긴다.
/// **주의(유출 위험)**: `Cookie` 를 포함한 모든 헤더와 바디를 원문 그대로 남긴다(사용자가 raw 를
/// 원하고 쿠키로 디버그) — 로그 파일을 공유하면 세션 쿠키가 노출된다. 로그인류 비밀값만 가린다.
pub(crate) fn log_packet_request(req: &reqwest::blocking::Request) {
    let mut lines = format!("→ {} {}", req.method(), req.url());
    for (name, value) in req.headers() {
        let raw = String::from_utf8_lossy(value.as_bytes());
        // 쿠키는 원문 유지. 그 외 헤더값만 자격 비밀값 마스킹.
        let shown = if name == COOKIE || name == SET_COOKIE {
            raw.into_owned()
        } else {
            redact_credentials(&raw)
        };
        lines.push_str(&format!("\n{name}: {shown}"));
    }
    if let Some(bytes) = req.body().and_then(reqwest::blocking::Body::as_bytes) {
        let body = String::from_utf8_lossy(bytes);
        lines.push_str(&format!("\n\n{}", redact_credentials(&body)));
    }
    tracing::info!(target: "packet", "{lines}");
}

/// 응답 라인 + 헤더 전체를 원문 그대로 `target: "packet"` 에 남긴다(바디는 호출부가 이미 남김).
pub(crate) fn log_packet_response(resp: &reqwest::blocking::Response) {
    let mut lines = format!("← {}", resp.status());
    for (name, value) in resp.headers() {
        lines.push_str(&format!(
            "\n{name}: {}",
            String::from_utf8_lossy(value.as_bytes())
        ));
    }
    tracing::info!(target: "packet", "{lines}");
}

/// `.send()` 를 대신하는 트레이스 전송. 요청 원문(트레이스 ON일 때)을 남기고 `client` 로 실행한 뒤
/// 응답 라인·헤더를 남긴다. 실행 클라이언트를 인자로 받아, `self.client`(리다이렉트 추종)와 npay
/// 가입용 `redirect::none` 클라이언트가 각자 자기 정책으로 실행되게 한다(둘을 섞으면 가입 홉 추종이
/// 깨진다). 응답 바디는 소비하지 않는다 — 호출부가 그대로 읽어 `body={...}` 로 남긴다.
pub(crate) trait TracedSend {
    fn send_traced(self, client: &Client) -> reqwest::Result<reqwest::blocking::Response>;
}

impl TracedSend for reqwest::blocking::RequestBuilder {
    fn send_traced(self, client: &Client) -> reqwest::Result<reqwest::blocking::Response> {
        let req = self.build()?;
        if packet_trace_enabled() {
            log_packet_request(&req);
        }
        let resp = client.execute(req)?;
        if packet_trace_enabled() {
            log_packet_response(&resp);
        }
        Ok(resp)
    }
}

// 429·5xx 처럼 재시도해 볼 만한(일시적) 상태코드인지 판별하는 함수입니다.
fn is_retryable_status(status: u16) -> bool {
    status == 429 || (500..=599).contains(&status)
}

// 전송 오류 분류 라벨(util::reqwest_kind)이 일시적 재시도 대상인지 판별한다. 연결 실패·타임아웃·
// 요청/전송 오류는 망이 잠깐 끊긴 경우가 많아 재시도하지만, 응답이 도착한 뒤의 디코드/본문/
// 리다이렉트 오류는 다시 보내도 같은 결과라 재시도하지 않는다.
fn is_retryable_transport_kind(kind: &str) -> bool {
    matches!(kind, "연결 실패" | "타임아웃" | "요청 오류" | "전송 오류")
}

// op를 최대 max_attempts번 시도하되, retryable이 true인 일시적 실패에만 재시도한다(재시도 직전
// on_retry(attempt) 호출 — 대기/로그를 호출부가 주입). 성공하면 즉시 그 값을, 재시도 불가
// 실패거나 시도를 소진하면 마지막 에러를 돌려준다. reqwest::Error는 공개 생성자가 없어 직접
// 만들 수 없으므로, 루프 로직을 send와 분리해 둬 단위 테스트가 가능하게 한다.
fn retry_transient<T, E>(
    max_attempts: u32,
    mut op: impl FnMut() -> Result<T, E>,
    retryable: impl Fn(&E) -> bool,
    mut on_retry: impl FnMut(u32),
) -> Result<T, E> {
    let mut attempt: u32 = 0;
    loop {
        attempt += 1;
        match op() {
            Ok(value) => return Ok(value),
            Err(error) if retryable(&error) && attempt < max_attempts => on_retry(attempt),
            Err(error) => return Err(error),
        }
    }
}

// 전송 계층 재시도 대기시간(0.5초에서 시작해 2배씩, 상한 3초). POST 429용 backoff_delay와 분리해
// 짧게 유지한다.
fn transport_backoff_delay(attempt: u32) -> Duration {
    let shift = attempt.saturating_sub(1).min(5);
    let scaled = TRANSPORT_RETRY_BASE.saturating_mul(1u32 << shift);
    scaled.min(TRANSPORT_RETRY_MAX_DELAY)
}

// 재시도 대기시간을 계산하는 함수입니다. POST_RETRY_BASE=POST_RETRY_MAX_DELAY=70초이므로
// 모든 시도에서 70초로 고정된다(사수 요청). 상수를 다시 벌리면 지수 백오프로 동작.
fn backoff_delay(attempt: u32) -> Duration {
    let shift = attempt.saturating_sub(1).min(5);
    let scaled = POST_RETRY_BASE.saturating_mul(1u32 << shift);
    scaled.min(POST_RETRY_MAX_DELAY)
}

// 응답의 Retry-After 헤더(초 단위 정수)를 대기시간으로 해석하는 함수입니다.
// HTTP-date 형식은 다루지 않고, 상한 POST_RETRY_MAX_DELAY로 캡한다.
fn parse_retry_after(headers: &HeaderMap) -> Option<Duration> {
    let secs: u64 = headers
        .get(RETRY_AFTER)?
        .to_str()
        .ok()?
        .trim()
        .parse()
        .ok()?;
    Some(Duration::from_secs(secs).min(POST_RETRY_MAX_DELAY))
}

// 현재 토론방 URL에서 네이버 discussionType과 itemCode를 계산하는 함수입니다.
fn discussion_target_from_url(page_url: &str) -> AutomationResult<DiscussionTarget> {
    let path = url::Url::parse(page_url)
        .map_err(|error| AutomationError::new(format!("현재 URL 해석 실패: {error}")))?
        .path()
        .to_owned();
    let item_code = path
        .split("/stock/")
        .nth(1)
        .or_else(|| path.split("/index/").nth(1))
        .and_then(|value| value.split('/').next())
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .ok_or_else(|| {
            AutomationError::new(format!(
                "현재 URL에서 종목 코드를 찾지 못했습니다: {page_url}"
            ))
        })?;
    let discussion_type = if path.contains("/domestic/index/") {
        "domesticIndex"
    } else if path.contains("/domestic/stock/") {
        "domesticStock"
    } else if path.contains("/worldstock/index/") {
        "foreignIndex"
    } else if path.contains("/worldstock/stock/") {
        "foreignStock"
    } else {
        "domesticStock"
    };

    Ok(DiscussionTarget {
        discussion_type: discussion_type.to_owned(),
        item_code,
    })
}

// 현재 토론글 URL에서 댓글 API에 필요한 objectId를 추출하는 함수입니다.
fn object_id_from_url(page_url: &str) -> AutomationResult<String> {
    page_url
        .split("/discussion/")
        .nth(1)
        .map(|value| {
            value
                .chars()
                .take_while(|character| character.is_ascii_digit())
                .collect::<String>()
        })
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            AutomationError::new(format!(
                "현재 URL에서 댓글 objectId를 찾지 못했습니다: {page_url}"
            ))
        })
}

// 랭킹/시세 API 응답 전체에서 종목 후보를 모으는 함수입니다.
fn collect_stock_candidates(value: &Value) -> Vec<StockCandidate> {
    let mut candidates = Vec::new();
    collect_stock_candidates_from_value(value, &mut candidates);
    dedupe_stock_candidates(candidates)
}

// 중첩 JSON을 재귀적으로 순회하면서 종목 코드와 종목명을 찾는 함수입니다.
fn collect_stock_candidates_from_value(value: &Value, candidates: &mut Vec<StockCandidate>) {
    match value {
        Value::Array(items) => {
            for item in items {
                collect_stock_candidates_from_value(item, candidates);
            }
        }
        Value::Object(object) => {
            if let Some(item_code) = direct_string(
                object,
                &[
                    "itemCode",
                    "stockCode",
                    "code",
                    "symbolCode",
                    "reutersCode",
                    "localCode",
                ],
            ) {
                if looks_like_stock_code(&item_code) {
                    let item_name = direct_string(
                        object,
                        &[
                            "itemName",
                            "stockName",
                            "name",
                            "korName",
                            "stockNameKr",
                            "displayName",
                        ],
                    )
                    .unwrap_or_else(|| item_code.clone());
                    let rank = direct_number(object, &["rank", "ranking", "rankNo", "no"])
                        .map(|rank| rank.to_string())
                        .unwrap_or_else(|| (candidates.len() + 1).to_string());

                    candidates.push(StockCandidate {
                        item_code,
                        item_name,
                        rank,
                    });
                }
            }

            for child in object.values() {
                collect_stock_candidates_from_value(child, candidates);
            }
        }
        _ => {}
    }
}

// 토론글 목록 API 응답 전체에서 게시글 후보를 모으는 함수입니다.
fn collect_post_candidates(value: &Value) -> Vec<PostCandidate> {
    let mut candidates = Vec::new();
    collect_post_candidates_from_value(value, &mut candidates);
    dedupe_post_candidates(candidates)
}

// 중첩 JSON을 재귀적으로 순회하면서 게시글 ID 후보를 찾는 함수입니다.
fn collect_post_candidates_from_value(value: &Value, candidates: &mut Vec<PostCandidate>) {
    match value {
        Value::Array(items) => {
            for item in items {
                collect_post_candidates_from_value(item, candidates);
            }
        }
        Value::Object(object) => {
            if let Some(post_id) = direct_string(
                object,
                &[
                    "postId",
                    "discussionPostId",
                    "discussionId",
                    "id",
                    "articleId",
                ],
            ) {
                if looks_like_post_id(&post_id) {
                    candidates.push(PostCandidate { post_id });
                }
            }

            for child in object.values() {
                collect_post_candidates_from_value(child, candidates);
            }
        }
        _ => {}
    }
}

// JSON 문자열 또는 숫자를 후보 추출용 문자열로 바꾸는 함수입니다.
fn value_to_string(value: &Value) -> Option<String> {
    match value {
        Value::String(value) if !value.trim().is_empty() => Some(value.trim().to_owned()),
        Value::Number(value) => Some(value.to_string()),
        _ => None,
    }
}

// JSON 숫자 또는 숫자 문자열을 u64로 바꾸는 함수입니다.
fn value_to_u64(value: &Value) -> Option<u64> {
    match value {
        Value::Number(value) => value.as_u64(),
        Value::String(value) => value.parse().ok(),
        _ => None,
    }
}

// 현재 JSON 객체의 직접 필드에서만 문자열 후보를 찾는 함수입니다.
fn direct_string(object: &serde_json::Map<String, Value>, keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|key| object.get(*key).and_then(value_to_string))
}

// 현재 JSON 객체의 직접 필드에서만 숫자 후보를 찾는 함수입니다.
fn direct_number(object: &serde_json::Map<String, Value>, keys: &[&str]) -> Option<u64> {
    keys.iter()
        .find_map(|key| object.get(*key).and_then(value_to_u64))
}

// 같은 종목 코드가 여러 번 발견됐을 때 첫 후보만 남기는 함수입니다.
fn dedupe_stock_candidates(candidates: Vec<StockCandidate>) -> Vec<StockCandidate> {
    let mut seen = BTreeMap::new();
    let mut unique = Vec::new();

    for candidate in candidates {
        if seen.insert(candidate.item_code.clone(), ()).is_none() {
            unique.push(candidate);
        }
    }

    unique
}

// 같은 게시글 ID가 여러 번 발견됐을 때 첫 후보만 남기는 함수입니다.
fn dedupe_post_candidates(candidates: Vec<PostCandidate>) -> Vec<PostCandidate> {
    let mut seen = BTreeMap::new();
    let mut unique = Vec::new();

    for candidate in candidates {
        if seen.insert(candidate.post_id.clone(), ()).is_none() {
            unique.push(candidate);
        }
    }

    unique
}

// 문자열이 네이버 종목 코드 형태인지 대략적으로 판단하는 함수입니다.
fn looks_like_stock_code(value: &str) -> bool {
    let len = value.chars().count();
    (5..=8).contains(&len)
        && value
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || character == '.')
        && value.chars().any(|character| character.is_ascii_digit())
}

// 문자열이 네이버 토론글 ID 형태인지 대략적으로 판단하는 함수입니다.
fn looks_like_post_id(value: &str) -> bool {
    let len = value.chars().count();
    (6..=12).contains(&len) && value.chars().all(|character| character.is_ascii_digit())
}

// discussionType, itemCode, postId를 화면 이동용 네이버 토론 URL로 바꾸는 함수입니다.
fn discussion_url_for(discussion_type: &str, item_code: &str, post_id: Option<&str>) -> String {
    let base_path = match discussion_type {
        "domesticIndex" => format!("{STOCK_ORIGIN}/domestic/index/{item_code}/discussion"),
        "foreignStock" => format!("{STOCK_ORIGIN}/worldstock/stock/{item_code}/discussion"),
        "foreignIndex" => format!("{STOCK_ORIGIN}/worldstock/index/{item_code}/discussion"),
        _ => format!("{STOCK_ORIGIN}/domestic/stock/{item_code}/discussion"),
    };

    match post_id {
        Some(post_id) => format!("{base_path}/{post_id}?chip=all"),
        None => format!("{base_path}?chip=all"),
    }
}

// Wireshark에서 확인한 글쓰기 add 요청의 JSON 본문을 만드는 함수입니다.
fn build_post_payload(title: &str, body: &str, target: &DiscussionTarget, tx_id: &str) -> Value {
    let document_id = packet_id("DOC");
    let component_id = packet_id("TEXT");
    let paragraph_id = packet_id("PARAGRAPH");
    let node_id = packet_id("NODE");
    let body_length = body.chars().count();

    json!({
        "title": title,
        "contentJson": {
            "document": {
                "version": "2.9.0",
                "theme": "default",
                "language": "ko-KR",
                "id": document_id,
                "components": [{
                    "id": component_id,
                    "layout": "default",
                    "value": [{
                        "id": paragraph_id,
                        "nodes": [{
                            "id": node_id,
                            "value": body,
                            "@ctype": "textNode"
                        }],
                        "@ctype": "paragraph"
                    }],
                    "@ctype": "text"
                }],
                "di": {
                    "dif": false,
                    "dio": [{
                        "dis": "N",
                        "dia": {
                            "t": 0,
                            "p": 0,
                            "st": body_length,
                            "sk": 0
                        }
                    }]
                }
            },
            "documentId": ""
        },
        "isCleanbotDisabled": false,
        "danglingImages": [],
        "discussionType": target.discussion_type,
        "itemCode": target.item_code,
        "txId": tx_id,
        "inflow": "NFS-P-P"
    })
}

// 추천 닉네임 후보 목록에서 used에 없는 첫 닉네임을 고르고, 다 겹치면 마지막 후보를 돌려주는
// 순수 함수입니다(네트워크 없이 중복 회피 판정만 테스트할 수 있게 분리).
fn choose_unused_nickname(
    candidates: &[String],
    used: &std::collections::HashSet<String>,
) -> Option<String> {
    candidates
        .iter()
        .find(|candidate| !used.contains(*candidate))
        .cloned()
        .or_else(|| candidates.last().cloned())
}

// Wireshark에서 확인한 댓글 create 요청의 form-urlencoded 본문을 만드는 함수입니다.
fn build_comment_form(object_id: &str, object_url: &str, body: &str, cbox_token: &str) -> String {
    Serializer::new(String::new())
        .append_pair("lang", "ko")
        .append_pair("pageType", "more")
        .append_pair("country", "")
        .append_pair("objectId", object_id)
        .append_pair("categoryId", "")
        .append_pair("pageSize", "10")
        .append_pair("indexSize", "10")
        .append_pair("groupId", "")
        .append_pair("listType", "OBJECT")
        .append_pair("clientType", "web-pc")
        .append_pair("objectUrl", object_url)
        .append_pair("contents", body)
        .append_pair("userType", "")
        .append_pair("pick", "false")
        .append_pair("manager", "false")
        .append_pair("score", "0")
        .append_pair("likeItId", "")
        .append_pair("secret", "false")
        .append_pair("refresh", "true")
        .append_pair("imageCount", "0")
        .append_pair("commentType", "txt")
        .append_pair("validateBanWords", "true")
        .append_pair("invalidateCleanbotAlert", "false")
        .append_pair("cbox_token", cbox_token)
        .finish()
}

/// 실제 API 응답 본문을 로그에 남길 때 너무 길지 않게 자른다(원본 성공/실패 응답 기록용).
/// 쿠키/비밀번호 같은 민감값은 응답 본문에 없으므로 그대로 남겨도 안전하다.
fn log_snippet(body: &str) -> String {
    const MAX: usize = 600;
    let trimmed = body.trim();
    if trimmed.chars().count() <= MAX {
        return trimmed.to_owned();
    }
    let head: String = trimmed.chars().take(MAX).collect();
    format!("{head}…(생략)")
}

// API 응답 문자열을 JSON으로 파싱하고 오류 메시지에 패킷 이름을 붙이는 함수입니다.
fn parse_json(response_text: &str, label: &str) -> AutomationResult<Value> {
    serde_json::from_str(response_text).map_err(|error| {
        AutomationError::new(format!(
            "{label} 패킷 응답 JSON 해석 실패: {error}; body={}",
            response_text.chars().take(200).collect::<String>()
        ))
    })
}

// getProfile JSONP 응답에서 callback wrapper를 제거하는 함수입니다.
fn strip_jsonp(response_text: &str) -> AutomationResult<&str> {
    let start = response_text
        .find('(')
        .ok_or_else(|| AutomationError::new("JSONP 응답에서 여는 괄호를 찾지 못했습니다."))?;
    let end = response_text
        .rfind(')')
        .ok_or_else(|| AutomationError::new("JSONP 응답에서 닫는 괄호를 찾지 못했습니다."))?;

    if end <= start {
        return Err(AutomationError::new(
            "JSONP 응답 괄호 위치가 올바르지 않습니다.",
        ));
    }

    Ok(&response_text[start + 1..end])
}

// HTTP 응답 상태를 확인하고 본문 문자열을 읽는 함수입니다.
fn response_text(response: reqwest::blocking::Response, label: &str) -> AutomationResult<String> {
    let status = response.status();
    let text = response.text().map_err(|error| {
        AutomationError::new(format!("{label} 패킷 응답 본문 읽기 실패: {error}"))
    })?;

    if status.is_success() {
        // 성공도 네이버 **원본 응답 body를 그대로** 남긴다(사용자·사수 지시 2026-07-01: 성공/실패
        // 가리지 말고 전부 네이버 원문). 프로필 상태·닉네임·방/글 선택·조회 등 모든 GET 계열이 여기 지난다.
        tracing::info!(
            label,
            status = status.as_u16(),
            body = %log_snippet(&text),
            "실제 API 응답 OK — 네이버 원문"
        );
        return Ok(text);
    }

    // 실제 API의 *원본 실패 응답*(상태코드+본문)을 그대로 로그 파일에 남긴다. 예) 프로필 상태
    // status=500, body={"message":"Failed to fetch profile user status"}. 호출부로 올라가며
    // AutomationError의 백트레이스로도 이어진다(자세히 보기/[POST] 실패 줄의 trace).
    tracing::warn!(
        label,
        status = status.as_u16(),
        body = %log_snippet(&text),
        "실제 API HTTP 실패"
    );
    Err(AutomationError::new(format!(
        "{label} 패킷 HTTP 실패: status={}, body={}",
        status.as_u16(),
        text.chars().take(300).collect::<String>()
    )))
}

// 네이버 API 실패 응답에서 사람이 읽을 오류 메시지를 뽑는 함수입니다.
fn packet_error_message(value: &Value, fallback: &str) -> String {
    value
        .get("message")
        .and_then(Value::as_str)
        .or_else(|| value.get("detailCode").and_then(Value::as_str))
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| fallback.chars().take(200).collect())
}

// 글쓰기 contentJson에 넣을 임시 문서 ID를 만드는 함수입니다.
fn packet_id(prefix: &str) -> String {
    format!("SE-{prefix}-{}", timestamp_nanos())
}

// 패킷 callback과 임시 ID에 사용할 현재 시간 값을 나노초 단위로 구하는 함수입니다.
fn timestamp_nanos() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default()
}

// 실행 시점, 프로세스 ID, salt를 섞어 후보 목록에서 하나를 고르는 함수입니다.
fn pseudo_index_with_seed(len: usize, seed: u128, salt: u128) -> usize {
    if len == 0 {
        return 0;
    }

    (mix_seed(seed, salt) as usize) % len
}

// 프로세스마다 다른 랜덤 선택 기준 seed를 만드는 함수입니다.
fn selection_seed() -> u128 {
    timestamp_nanos() ^ ((process::id() as u128) << 64)
}

// seed와 salt를 섞어 낮은 자리수 편향을 줄이는 함수입니다.
fn mix_seed(seed: u128, salt: u128) -> u128 {
    let mut value = seed ^ salt.wrapping_mul(0x9E37_79B9_7F4A_7C15_6A09_E667_F3BC_C909_u128);
    value ^= value >> 64;
    value = value.wrapping_mul(0xBF58_476D_1CE4_E5B9_94D0_49BB_1331_11EB_u128);
    value ^ (value >> 61)
}

// 문자열을 reqwest HeaderValue로 변환하고 오류 메시지에 헤더 이름을 붙이는 함수입니다.
fn header_value(value: &str, label: &str) -> AutomationResult<HeaderValue> {
    HeaderValue::from_str(value)
        .map_err(|error| AutomationError::new(format!("{label} 헤더 값 생성 실패: {error}")))
}

// 대상 호스트에 적용되는 쿠키만 골라 "name=value; ..." Cookie 헤더를 만드는 함수입니다.
// (domain, name)으로 구분하고, 같은 이름이 겹치면 host-only 쿠키가 도메인 쿠키를 이깁니다.
// 동의하기(가입) GET 리다이렉트 추종 후 최종 URL이 가입 완료 상태인지 판정한다. 아래 어느
// 페이지에 머물러 있으면 **미완료**로 본다(가입 안 됨):
//  - 약관 페이지(member.pay.naver.com/.../agreement)
//  - 가입 입력 페이지(financial-service/join)
//  - 로그인 페이지(nid.naver.com/nidlogin.login) — 미가입 계정은 필수 약관 동의가 없어 여기로
//    튕긴다. 예전엔 이 URL의 /agreement·/join 부분이 이중 URL인코딩(%252F…)이라 걸러지지 않아
//    "완료 ✅"로 오판했다(실측 로그 2026-07-01). login 페이지·약관동의 페이지를 명시로 잡는다.
//  - 공통 약관동의 페이지(commonTermAgree) — 필수 약관 동의를 요구하는 중간 페이지.
// 그 밖(가입 성공 콜백·토론 페이지로 빠짐)이면 완료로 본다.
/// 네이버 인증 쿠키(NID_AUT/NID_SES/BUC)를 **원문 그대로**(값 자르지 않음) 한 줄로 만든다. 좋아요
/// 경로 진단용(사용자 요청 2026-07-03): 발급된 쿠키 문자열과 재로그인 후 새 문자열을 원문 대조.
/// ⚠️ 살아있는 세션 토큰이라 이 로그가 찍힌 파일은 민감정보다.
fn auth_cookies_raw_line(cookies: &[NaverCookie]) -> String {
    let pick = |name: &str| -> &str {
        cookies
            .iter()
            .find(|cookie| cookie.name == name)
            .map(|cookie| cookie.value.as_str())
            .unwrap_or("(없음)")
    };
    format!(
        "NID_AUT={} | NID_SES={} | BUC={}",
        pick("NID_AUT"),
        pick("NID_SES"),
        pick("BUC")
    )
}

/// 프로필 form 응답 **원문(status+body)** 만으로 제재/만료/정상을 가른다(순수 함수, 테스트 가능).
/// 차단 신호(네이버 원문): UMON 밴/`403F0x`/`penaltyDays`, 또는 페이지 본문 `아이디 잠금`/`잠금조치`/
/// `보호조치`/`이용제한`/`이용이 제한`/`글쓰기 금지`/`비정상적인 활동`. 로그인 안 됨 신호: 로그인
/// 페이지(`nidlogin.login`/`frmNIDLogin`) 리다이렉트·비로그인 응답(401/`"rtn_cd":"1"`).
fn classify_restriction_body(status: u16, body: &str) -> RestrictionVerdict {
    let has = |needle: &str| body.contains(needle);
    let blocked = has("UMON_TEMP_BANNED")
        || has("UMON_PERMANENT_BANNED")
        || has("403F01")
        || has("403F02")
        || has("penaltyDays")
        || has("아이디 잠금")
        || has("잠금조치")
        || has("보호조치")
        || has("이용제한")
        || has("이용이 제한")
        || has("글쓰기 금지")
        || has("비정상적인 활동");
    if blocked {
        return RestrictionVerdict::Blocked(log_snippet(body));
    }
    let logged_out = status == 401
        || has("nidlogin.login")
        || has("frmNIDLogin")
        || has("\"rtn_cd\":\"1\"")
        || has("네이버에 로그인");
    if logged_out {
        return RestrictionVerdict::Expired(log_snippet(body));
    }
    if status == 200 {
        RestrictionVerdict::Healthy
    } else {
        // 판정 못 한 비200 — 원문은 이미 로그에 남겼으니 보수적으로 만료(재로그인) 처리.
        RestrictionVerdict::Expired(log_snippet(body))
    }
}

fn financial_join_completed(final_url: &str) -> bool {
    !final_url.contains("/agreement")
        && !final_url.contains("/financial-service/join")
        && !final_url.contains("nidlogin.login")
        && !final_url.contains("commonTermAgree")
}

/// 필수약관 페이지(commonTermAgree)면 그 `rurl`(=약관동의 콜백 URL)을 꺼낸다(순수 함수).
///
/// commonTermAgree는 HTTP 3xx가 아니라 200 HTML을 주고 JS `location.href = Base64.decode(<콜백>)`로
/// 콜백에 이동한다 — 리다이렉트만 따라가는 GET-follow는 여기서 멈춘다. 다행히 그 콜백 URL은
/// commonTermAgree의 `rurl` 쿼리에 그대로 들어있어(브라우저 JS가 가던 그 주소), 이 값을 이어서 GET
/// 하면 약관 콜백이 가입을 완료시킨다. commonTermAgree가 아니거나 rurl이 없으면 `None`.
fn term_agree_callback_url(current_url: &str) -> Option<String> {
    let parsed = url::Url::parse(current_url).ok()?;
    if !parsed.path().contains("commonTermAgree") {
        return None;
    }
    parsed
        .query_pairs()
        .find(|(key, _)| key == "rurl")
        .map(|(_, value)| value.into_owned())
        .filter(|value| !value.trim().is_empty())
}

/// user-agent 문자열의 Chrome 메이저 버전으로 `sec-ch-ua` 헤더 값을 만든다(순수 함수). 브라우저는 모든
/// 요청에 client-hints를 보내는데 우리가 안 보내면 네이버 봇탐지(UMON)가 막는다(실측: 우리 403
/// UMON_BANNED·프로필 500 ↔ 브라우저 200). UA의 버전과 sec-ch-ua 버전이 다른 것도 봇 신호라 실제
/// UA(`Chrome/149...`)에서 버전을 뽑아 맞춘다. 버전을 못 찾으면 최신 안정 버전을 기본값으로 쓴다.
fn chrome_major_from_user_agent(user_agent: &str) -> &str {
    user_agent
        .split("Chrome/")
        .nth(1)
        .and_then(|rest| rest.split('.').next())
        .filter(|v| !v.is_empty() && v.bytes().all(|b| b.is_ascii_digit()))
        .unwrap_or("149")
}

fn sec_ch_ua_from_user_agent(user_agent: &str) -> String {
    let major = chrome_major_from_user_agent(user_agent);
    format!("\"Google Chrome\";v=\"{major}\", \"Chromium\";v=\"{major}\", \"Not)A;Brand\";v=\"24\"")
}

/// 단일 Set-Cookie 헤더 한 줄을 (도메인·이름·값)으로 파싱한다(순수 함수). `name=value; domain=.naver.com;
/// path=/; ...` 형태에서 이름/값과 domain 속성만 취한다. domain 속성이 없으면 응답 호스트(host-only)로
/// 스코프한다(브라우저 규칙). 이름이 비면 `None`. 만료/삭제 속성은 다루지 않는다 — 가입 체인(수초)에선
/// 서버가 재발급하는 쿠키를 이어받는 것만 중요하고, 만료 마커 쿠키는 서버가 무시한다.
fn parse_set_cookie(line: &str, response_host: &str) -> Option<NaverCookie> {
    let mut parts = line.split(';');
    let (name, value) = parts.next()?.trim().split_once('=')?;
    let name = name.trim();
    if name.is_empty() {
        return None;
    }
    let domain = parts
        .filter_map(|attr| attr.trim().split_once('='))
        .find(|(key, _)| key.trim().eq_ignore_ascii_case("domain"))
        .map(|(_, v)| v.trim().to_owned())
        .filter(|d| !d.is_empty())
        .unwrap_or_else(|| response_host.to_owned());
    Some(NaverCookie {
        domain,
        name: name.to_owned(),
        value: value.trim().to_owned(),
    })
}

/// 응답의 Set-Cookie들을 jar에 병합한다(같은 `(이름, 도메인)`은 값 갱신, 없으면 추가) — 브라우저 쿠키
/// 자와 동일하게, 가입 리다이렉트 체인이 회전시킨 쿠키를 다음 홉이 이어 쓰게 한다. 같은 이름이 한
/// 응답에서 여러 번(마지막이 최종값) 오면 순서대로 적용해 마지막 값이 남는다(브라우저 last-wins).
fn merge_set_cookies<'a>(
    jar: &mut Vec<NaverCookie>,
    headers: impl Iterator<Item = &'a HeaderValue>,
    response_host: &str,
) {
    for header in headers {
        let Ok(line) = header.to_str() else { continue };
        let Some(parsed) = parse_set_cookie(line, response_host) else {
            continue;
        };
        match jar
            .iter_mut()
            .find(|c| c.name == parsed.name && c.domain == parsed.domain)
        {
            Some(existing) => existing.value = parsed.value,
            None => jar.push(parsed),
        }
    }
}

fn build_cookie_header(cookies: &[NaverCookie], host: &str) -> String {
    let mut applicable: Vec<&NaverCookie> = cookies
        .iter()
        .filter(|cookie| cookie_applies_to_host(&cookie.domain, host))
        .collect();
    // 도메인 쿠키(앞에 '.')를 먼저, host-only 쿠키를 나중에 둬 last-wins로 host-only가 이기게 한다.
    applicable.sort_by_key(|cookie| u8::from(!cookie.domain.starts_with('.')));

    let mut by_name: BTreeMap<&str, &str> = BTreeMap::new();
    for cookie in applicable {
        by_name.insert(cookie.name.as_str(), cookie.value.as_str());
    }
    by_name
        .into_iter()
        .map(|(name, value)| format!("{name}={value}"))
        .collect::<Vec<_>>()
        .join("; ")
}

// 쿠키 도메인이 대상 호스트에 적용되는지 판단하는 함수입니다. 앞에 '.'가 있으면 도메인
// 쿠키(서브도메인 포함), 없으면 host-only 쿠키(정확히 그 호스트만)입니다.
fn cookie_applies_to_host(domain: &str, host: &str) -> bool {
    match domain.strip_prefix('.') {
        Some(base) => host == base || host.ends_with(&format!(".{base}")),
        None => host == domain,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use serde_json::json;

    use super::*;

    fn cookie(domain: &str, name: &str, value: &str) -> NaverCookie {
        NaverCookie {
            domain: domain.to_owned(),
            name: name.to_owned(),
            value: value.to_owned(),
        }
    }

    #[test]
    fn from_storage_state_builds_with_naver_session_cookies() {
        // NID_AUT/NID_SES가 있으면 파일 쿠키만으로 클라이언트가 만들어진다(Chrome 없이 좋아요).
        let storage = json!({
            "cookies": [
                { "domain": ".naver.com", "name": "NID_AUT", "value": "aut-token" },
                { "domain": ".naver.com", "name": "NID_SES", "value": "ses-token" },
                { "domain": "example.com", "name": "OTHER", "value": "ignored" },
            ]
        });
        let client =
            NaverPacketClient::from_storage_state(&storage).expect("세션 쿠키 있으면 성공");
        // naver 도메인 쿠키만 수집되고, 무관 도메인(example.com)은 제외된다.
        let header = client.cookie_header_for(STOCK_HOST);
        assert!(header.contains("NID_AUT=aut-token"));
        assert!(header.contains("NID_SES=ses-token"));
        assert!(!header.contains("OTHER"));
    }

    #[test]
    fn from_storage_state_errors_without_session_cookies() {
        // 세션 쿠키(NID_AUT/NID_SES)가 없으면 로그인 만료로 보고 실패한다.
        let storage = json!({
            "cookies": [
                { "domain": ".naver.com", "name": "NNB", "value": "x" },
            ]
        });
        assert!(NaverPacketClient::from_storage_state(&storage).is_err());
    }

    #[test]
    fn is_retryable_status_covers_429_and_5xx_only() {
        assert!(is_retryable_status(429));
        assert!(is_retryable_status(500));
        assert!(is_retryable_status(503));
        // 404/401/403 같은 클라이언트 오류와 2xx는 재시도하지 않는다.
        assert!(!is_retryable_status(200));
        assert!(!is_retryable_status(404));
        assert!(!is_retryable_status(401));
    }

    #[test]
    fn is_retryable_transport_kind_covers_transient_send_failures() {
        // 연결/타임아웃/요청/전송 오류는 일시적 망 끊김(IP 교체 직후 등)일 때가 많아 재시도.
        assert!(is_retryable_transport_kind("연결 실패"));
        assert!(is_retryable_transport_kind("타임아웃"));
        assert!(is_retryable_transport_kind("요청 오류"));
        assert!(is_retryable_transport_kind("전송 오류"));
        // 응답이 도착한 뒤의 디코드/본문/리다이렉트 오류는 재시도해도 의미 없다.
        assert!(!is_retryable_transport_kind("디코드 오류"));
        assert!(!is_retryable_transport_kind("본문 오류"));
        assert!(!is_retryable_transport_kind("리다이렉트 오류"));
    }

    #[test]
    fn retry_transient_returns_first_success_without_retry() {
        let mut calls = 0;
        let retries = std::cell::Cell::new(0);
        let result: Result<&str, i32> = retry_transient(
            4,
            || {
                calls += 1;
                Ok("ok")
            },
            |_| true,
            |_| retries.set(retries.get() + 1),
        );
        assert_eq!(result, Ok("ok"));
        assert_eq!(calls, 1, "성공이면 한 번만 호출");
        assert_eq!(retries.get(), 0, "성공이면 재시도 대기 없음");
    }

    #[test]
    fn retry_transient_retries_transient_errors_then_succeeds() {
        let mut calls = 0;
        let retries = std::cell::Cell::new(0);
        let result: Result<&str, i32> = retry_transient(
            4,
            || {
                calls += 1;
                if calls < 3 {
                    Err(503)
                } else {
                    Ok("ok")
                }
            },
            |_| true, // 모두 일시적
            |_| retries.set(retries.get() + 1),
        );
        assert_eq!(result, Ok("ok"));
        assert_eq!(calls, 3, "2번 실패 후 3번째 성공");
        assert_eq!(retries.get(), 2, "성공 전 2번 재시도 대기");
    }

    #[test]
    fn retry_transient_stops_immediately_on_non_retryable_error() {
        let mut calls = 0;
        let retries = std::cell::Cell::new(0);
        let result: Result<&str, i32> = retry_transient(
            4,
            || {
                calls += 1;
                Err(404)
            },
            |&e| e >= 500, // 4xx는 재시도 안 함
            |_| retries.set(retries.get() + 1),
        );
        assert_eq!(result, Err(404));
        assert_eq!(calls, 1, "재시도 불가 에러는 한 번에 중단");
        assert_eq!(retries.get(), 0);
    }

    #[test]
    fn retry_transient_exhausts_attempts_and_returns_last_error() {
        let mut calls = 0;
        let retries = std::cell::Cell::new(0);
        let result: Result<&str, i32> = retry_transient(
            4,
            || {
                calls += 1;
                Err(500 + calls) // 매번 다른 일시적 에러
            },
            |_| true,
            |_| retries.set(retries.get() + 1),
        );
        assert_eq!(result, Err(504), "마지막(4번째) 시도의 에러를 돌려준다");
        assert_eq!(calls, 4, "max_attempts번까지 시도");
        assert_eq!(retries.get(), 3, "마지막 시도 빼고 3번 재시도 대기");
    }

    #[test]
    fn transport_backoff_delay_is_short_and_capped() {
        // POST 429용 70초 백오프와 달리, 전송 계층 재시도는 짧게(잠깐 끊긴 망이 곧 복구).
        assert_eq!(transport_backoff_delay(1), Duration::from_millis(500));
        assert_eq!(transport_backoff_delay(2), Duration::from_millis(1000));
        assert_eq!(transport_backoff_delay(3), Duration::from_millis(2000));
        // 상한 3초로 캡.
        assert_eq!(transport_backoff_delay(4), TRANSPORT_RETRY_MAX_DELAY);
        assert_eq!(transport_backoff_delay(99), TRANSPORT_RETRY_MAX_DELAY);
        assert_eq!(TRANSPORT_RETRY_MAX_DELAY, Duration::from_secs(3));
    }

    #[test]
    fn backoff_delay_is_fixed_seventy_seconds() {
        // 사수 요청: 429 백오프를 70초로 고정.
        assert_eq!(POST_RETRY_MAX_DELAY, Duration::from_secs(70));
        assert_eq!(backoff_delay(1), Duration::from_secs(70));
        assert_eq!(backoff_delay(2), Duration::from_secs(70));
        assert_eq!(backoff_delay(99), Duration::from_secs(70));
    }

    #[test]
    fn parse_retry_after_reads_seconds_caps_and_ignores_non_integer() {
        let mut headers = HeaderMap::new();
        headers.insert(RETRY_AFTER, HeaderValue::from_static("3"));
        assert_eq!(parse_retry_after(&headers), Some(Duration::from_secs(3)));

        // 상한 초과는 캡된다.
        let mut big = HeaderMap::new();
        big.insert(RETRY_AFTER, HeaderValue::from_static("999"));
        assert_eq!(parse_retry_after(&big), Some(POST_RETRY_MAX_DELAY));

        // 헤더가 없거나 HTTP-date 형식이면 None → 지수 백오프로 폴백.
        assert_eq!(parse_retry_after(&HeaderMap::new()), None);
        let mut date = HeaderMap::new();
        date.insert(
            RETRY_AFTER,
            HeaderValue::from_static("Wed, 21 Oct 2026 07:28:00 GMT"),
        );
        assert_eq!(parse_retry_after(&date), None);
    }

    #[test]
    fn cookie_applies_to_host_respects_domain_vs_host_scope() {
        // 도메인 쿠키(앞에 '.')는 서브도메인까지 적용된다.
        assert!(cookie_applies_to_host(".naver.com", "stock.naver.com"));
        assert!(cookie_applies_to_host(".naver.com", "apis.naver.com"));
        // host-only 쿠키는 정확히 그 호스트만.
        assert!(cookie_applies_to_host("stock.naver.com", "stock.naver.com"));
        assert!(!cookie_applies_to_host(
            "stock.naver.com",
            "m.stock.naver.com"
        ));
        assert!(!cookie_applies_to_host("stock.naver.com", "apis.naver.com"));
    }

    #[test]
    fn build_cookie_header_scopes_host_only_cookies_per_host() {
        // 같은 이름 NNB가 도메인 전역(.naver.com)과 host-only(stock.naver.com) 둘 다 존재.
        let cookies = vec![
            cookie(".naver.com", "NID_AUT", "aut"),
            cookie(".naver.com", "NNB", "global"),
            cookie("stock.naver.com", "NNB", "stockonly"),
        ];

        let stock = build_cookie_header(&cookies, "stock.naver.com");
        // stock.naver.com에는 host-only 값이 우선 적용된다.
        assert!(stock.contains("NNB=stockonly"));
        assert!(stock.contains("NID_AUT=aut"));

        let apis = build_cookie_header(&cookies, "apis.naver.com");
        // apis.naver.com에는 stock host-only 쿠키가 새지 않고 전역 값만 적용된다.
        assert!(apis.contains("NNB=global"));
        assert!(!apis.contains("stockonly"));
    }

    #[test]
    fn login_cookies_apply_to_npay_join_host() {
        // 동의하기(가입) GET은 member-web.pay.naver.com 으로 가는데, .naver.com 도메인 로그인
        // 쿠키가 거기에도 붙어야 한다(안 붙으면 비로그인으로 처리돼 가입이 안 됨).
        assert!(cookie_applies_to_host(
            ".naver.com",
            "member-web.pay.naver.com"
        ));
        let cookies = vec![
            cookie(".naver.com", "NID_AUT", "aut"),
            cookie(".naver.com", "NID_SES", "ses"),
            cookie("stock.naver.com", "NNB", "stockonly"),
        ];
        let pay = build_cookie_header(&cookies, "member-web.pay.naver.com");
        assert!(
            pay.contains("NID_AUT=aut"),
            "pay 호스트에 로그인 쿠키가 붙어야 한다"
        );
        assert!(pay.contains("NID_SES=ses"));
        // stock host-only 쿠키는 pay 호스트로 새지 않는다.
        assert!(!pay.contains("stockonly"));
    }

    #[test]
    fn financial_join_completed_reads_final_url() {
        // 성공: join_success_url(=stock.naver.com/discussion)로 빠지면 완료. 쿼리가 붙어도 동일.
        assert!(financial_join_completed(
            "https://stock.naver.com/discussion"
        ));
        assert!(financial_join_completed(
            "https://stock.naver.com/discussion?from=pay"
        ));
        // 실패: join_fail_url(약관 페이지)에 머물면 미완료.
        assert!(!financial_join_completed(
            "https://member.pay.naver.com/financial-member/agreement"
        ));
        // 미완료: 가입 입력 페이지(financial-service/join)에서 더 못 빠져나갔으면 미완료.
        assert!(!financial_join_completed(
            "https://member-web.pay.naver.com/financial-service/join?from_pc=Y"
        ));
        // 미완료(회귀, 2026-07-01): 미가입 계정은 로그인 페이지로 튕긴다. 예전엔 URL 속 /agreement·
        // /join 이 이중 URL인코딩(%252F…)이라 안 걸려 "완료"로 오판했다 — 이제 nidlogin.login·
        // commonTermAgree 를 명시로 잡아 미완료로 본다(실측 로그의 final_url 그대로 검증).
        assert!(!financial_join_completed(
            "https://nid.naver.com/nidlogin.login?mode=form&url=https%3A%2F%2Fnid.naver.com%2Fuser2%2Fhelp%2FcommonTermAgree%3Ftermcd%3D40%26cpcd%3D123%26rurl%3Dhttps%253A%252F%252Fmember-web.pay.naver.com%252Ffinancial-service%252Fjoin%252Fnaver-term-consent%252Fcallback%26surl%3Dhttps%253A%252F%252Fmember.pay.naver.com%252Ffinancial-member%252Fagreement"
        ));
    }

    #[test]
    fn term_agree_callback_url_extracts_rurl() {
        // commonTermAgree(200 HTML·JS 콜백 이동)에 갇혔을 때, rurl에서 약관동의 콜백 URL을 꺼낸다.
        // 실측 패킷(npay 약관동의)의 commonTermAgree URL 그대로 — rurl 안의 session_id는 %3D 인코딩.
        let common_term = "https://nid.naver.com/user2/help/commonTermAgree?termcd=40&cpcd=123&rurl=https://member-web.pay.naver.com/financial-service/join/naver-term-consent/callback?session_id%3D1938f359-6ac3-460a-98af-4c4dc742df20&surl=https://member.pay.naver.com/financial-member/agreement";
        let callback = term_agree_callback_url(common_term).expect("rurl 콜백을 꺼내야 한다");
        assert!(
            callback.starts_with(
                "https://member-web.pay.naver.com/financial-service/join/naver-term-consent/callback"
            ),
            "콜백 URL이어야 한다: {callback}"
        );
        assert!(
            callback.contains("session_id=1938f359-6ac3-460a-98af-4c4dc742df20"),
            "session_id가 디코드돼 실려야 한다: {callback}"
        );
        // commonTermAgree가 아니면 None(성공 콜백·토론 페이지 등에선 이어가지 않는다).
        let not_term = term_agree_callback_url("https://stock.naver.com/discussion");
        assert!(not_term.is_none());
        // rurl이 없으면 None.
        let no_rurl =
            term_agree_callback_url("https://nid.naver.com/user2/help/commonTermAgree?termcd=40");
        assert!(no_rurl.is_none());
    }

    #[test]
    fn sec_ch_ua_uses_chrome_major_from_user_agent() {
        // 실제 UA의 Chrome 버전을 sec-ch-ua에 맞춘다(UA와 불일치도 봇 신호).
        let ua = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/151.0.0.0 Safari/537.36";
        let hint = sec_ch_ua_from_user_agent(ua);
        assert!(hint.contains("\"Google Chrome\";v=\"151\""), "{hint}");
        assert!(hint.contains("\"Chromium\";v=\"151\""), "{hint}");
        // Chrome 버전을 못 찾으면 기본값(빈 값·봇 탐지 유발 방지).
        let fallback = sec_ch_ua_from_user_agent("curl/8.0");
        assert!(fallback.contains("v=\"149\""), "{fallback}");
    }

    #[test]
    fn parse_set_cookie_reads_name_value_and_domain() {
        // 실측 패킷(`동의+프로필까지`)의 Set-Cookie 그대로: domain 속성이 있으면 그 도메인으로 스코프.
        let c = parse_set_cookie(
            "BUC=C1K-rII1UeFTKv4C-A8Z2R02ppXk5gDPnL3idqKQJ2k=; expires=Sat, 01 Jan 2050 09:00:00 GMT; path=/; domain=.naver.com; SameSite=None; Secure; HttpOnly",
            "member-web.pay.naver.com",
        )
        .expect("BUC를 파싱해야 한다");
        assert_eq!(c.name, "BUC");
        assert_eq!(c.value, "C1K-rII1UeFTKv4C-A8Z2R02ppXk5gDPnL3idqKQJ2k=");
        assert_eq!(c.domain, ".naver.com");
    }

    #[test]
    fn parse_set_cookie_without_domain_scopes_to_response_host() {
        // domain 속성이 없으면 host-only — 응답 호스트로 스코프(브라우저 규칙).
        let c = parse_set_cookie("JSESSIONID=6C20E5E5; Path=/; HttpOnly", "finance.naver.com")
            .expect("host-only 쿠키를 파싱해야 한다");
        assert_eq!(c.name, "JSESSIONID");
        assert_eq!(c.domain, "finance.naver.com");
        // 이름이 비면 None.
        assert!(parse_set_cookie("=orphan; path=/", "nid.naver.com").is_none());
    }

    #[test]
    fn merge_set_cookies_rotates_existing_and_adds_new() {
        // 로그인 jar에서 출발 — 가입 체인이 회전시키는 쿠키를 이어받는 게 핵심(실측: BUC 회전,
        // commonTermAgree가 NID_AUT/NID_SES 재발급). 같은 (이름,도메인)은 값 갱신, 새 이름은 추가.
        let mut jar = vec![
            NaverCookie {
                domain: ".naver.com".to_owned(),
                name: "NID_AUT".to_owned(),
                value: "OLD_AUT".to_owned(),
            },
            NaverCookie {
                domain: ".naver.com".to_owned(),
                name: "BUC".to_owned(),
                value: "OLD_BUC".to_owned(),
            },
        ];
        let headers = [
            HeaderValue::from_static("BUC=NEW_BUC; path=/; domain=.naver.com; Secure"),
            HeaderValue::from_static(
                "NID_AUT=NEW_AUT; path=/; domain=.naver.com; Secure; HttpOnly",
            ),
            HeaderValue::from_static("NID_SES=NEW_SES; path=/; domain=.naver.com; Secure"),
        ];
        merge_set_cookies(&mut jar, headers.iter(), "nid.naver.com");

        let get = |name: &str| {
            jar.iter()
                .find(|c| c.name == name)
                .map(|c| c.value.as_str())
        };
        assert_eq!(get("BUC"), Some("NEW_BUC"), "회전된 BUC로 갱신돼야 한다");
        assert_eq!(
            get("NID_AUT"),
            Some("NEW_AUT"),
            "재발급 NID_AUT로 갱신돼야 한다"
        );
        assert_eq!(
            get("NID_SES"),
            Some("NEW_SES"),
            "새 NID_SES가 추가돼야 한다"
        );
        // 갱신은 새 항목을 만들지 않는다(중복 방지) — BUC/NID_AUT는 각각 하나만.
        assert_eq!(jar.iter().filter(|c| c.name == "BUC").count(), 1);
        assert_eq!(jar.iter().filter(|c| c.name == "NID_AUT").count(), 1);
    }

    #[test]
    fn merge_set_cookies_last_value_wins_for_repeated_name() {
        // 한 응답에서 같은 이름이 여러 번(실측 commonTermAgree 응답은 NID_SES를 두 번 준다) → 마지막이 최종.
        let mut jar: Vec<NaverCookie> = Vec::new();
        let headers = [
            HeaderValue::from_static("NID_SES=FIRST; domain=.naver.com; path=/"),
            HeaderValue::from_static("NID_SES=SECOND; domain=.naver.com; path=/"),
        ];
        merge_set_cookies(&mut jar, headers.iter(), "nid.naver.com");
        assert_eq!(jar.iter().filter(|c| c.name == "NID_SES").count(), 1);
        assert_eq!(jar[0].value, "SECOND", "마지막 Set-Cookie 값이 남아야 한다");
    }

    #[test]
    fn strip_jsonp_extracts_get_profile_payload() {
        let payload = r#"jsonp_123({"rtn_cd":"0","rtn_msg":"Success","nick_name":"테스트","image_url":"u"});"#;

        let json_text = strip_jsonp(payload).expect("JSONP wrapper should be removed");
        let value = parse_json(json_text, "getProfile").expect("payload should be valid JSON");

        assert_eq!(value["rtn_cd"], "0");
        assert_eq!(value["nick_name"], "테스트");
    }

    #[test]
    fn discussion_target_from_url_detects_stock_and_index_types() {
        let stock = discussion_target_from_url(
            "https://stock.naver.com/domestic/stock/005930/discussion?chip=all",
        )
        .expect("domestic stock URL should parse");
        let index = discussion_target_from_url(
            "https://stock.naver.com/domestic/index/KOSPI/discussion?chip=all",
        )
        .expect("domestic index URL should parse");

        assert_eq!(stock.discussion_type, "domesticStock");
        assert_eq!(stock.item_code, "005930");
        assert_eq!(index.discussion_type, "domesticIndex");
        assert_eq!(index.item_code, "KOSPI");
    }

    #[test]
    fn object_id_from_url_extracts_discussion_post_id() {
        let object_id = object_id_from_url(
            "https://stock.naver.com/domestic/stock/005930/discussion/421063210?chip=all",
        )
        .expect("discussion post URL should contain object id");

        assert_eq!(object_id, "421063210");
    }

    #[test]
    fn collect_stock_candidates_finds_nested_candidates_and_dedupes() {
        let value = json!({
            "result": {
                "stocks": [
                    {
                        "rank": 1,
                        "itemCode": "005930",
                        "itemName": "삼성전자"
                    },
                    {
                        "rank": 2,
                        "stockCode": "000660",
                        "stockName": "SK하이닉스"
                    },
                    {
                        "rank": 3,
                        "itemCode": "005930",
                        "itemName": "삼성전자 중복"
                    },
                    {
                        "rank": 4,
                        "itemCode": "NO_CODE",
                        "itemName": "무효"
                    }
                ]
            }
        });

        let candidates = collect_stock_candidates(&value);

        assert_eq!(candidates.len(), 2);
        assert_eq!(candidates[0].item_code, "005930");
        assert_eq!(candidates[0].item_name, "삼성전자");
        assert_eq!(candidates[0].rank, "1");
        assert_eq!(candidates[1].item_code, "000660");
        assert_eq!(candidates[1].item_name, "SK하이닉스");
    }

    #[test]
    fn collect_stock_candidates_uses_direct_row_fields_for_rank() {
        let value = json!({
            "result": {
                "rank": 99,
                "stocks": [
                    {
                        "rank": 7,
                        "itemCode": "297570",
                        "itemName": "알로이스"
                    }
                ]
            }
        });

        let candidates = collect_stock_candidates(&value);

        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].item_code, "297570");
        assert_eq!(candidates[0].item_name, "알로이스");
        assert_eq!(candidates[0].rank, "7");
    }

    #[test]
    fn collect_post_candidates_finds_nested_ids_and_dedupes() {
        let value = json!({
            "result": {
                "posts": [
                    { "postId": "421063210", "title": "첫 글" },
                    { "id": 421029979, "title": "둘째 글" },
                    { "discussionPostId": "421063210", "title": "중복 글" },
                    { "id": "abc", "title": "무효 글" }
                ]
            }
        });

        let candidates = collect_post_candidates(&value);

        assert_eq!(candidates.len(), 2);
        assert_eq!(candidates[0].post_id, "421063210");
        assert_eq!(candidates[1].post_id, "421029979");
    }

    #[test]
    fn discussion_url_for_builds_room_and_post_urls() {
        let room_url = discussion_url_for("domesticStock", "005930", None);
        let post_url = discussion_url_for("domesticStock", "005930", Some("421063210"));

        assert_eq!(
            room_url,
            "https://stock.naver.com/domestic/stock/005930/discussion?chip=all"
        );
        assert_eq!(
            post_url,
            "https://stock.naver.com/domestic/stock/005930/discussion/421063210?chip=all"
        );
    }

    #[test]
    fn build_post_payload_matches_captured_add_packet_shape() {
        let target = DiscussionTarget {
            discussion_type: "domesticStock".to_owned(),
            item_code: "005930".to_owned(),
        };

        let payload = build_post_payload("테스트 제목", "본문 내용", &target, "tx-123");

        assert_eq!(payload["title"], "테스트 제목");
        assert_eq!(payload["discussionType"], "domesticStock");
        assert_eq!(payload["itemCode"], "005930");
        assert_eq!(payload["txId"], "tx-123");
        assert_eq!(payload["inflow"], "NFS-P-P");
        assert_eq!(payload["contentJson"]["document"]["version"], "2.9.0");
        assert_eq!(
            payload["contentJson"]["document"]["components"][0]["value"][0]["nodes"][0]["value"],
            "본문 내용"
        );
    }

    #[test]
    fn edit_body_reuses_add_document_builder_with_new_title_and_text() {
        // edit_post의 body 구성과 동일하게: add 빌더로 document를 만들고 edit 형태로 감싼다.
        let add_payload = build_post_payload(
            "수정 제목",
            "수정 본문",
            &DiscussionTarget {
                discussion_type: String::new(),
                item_code: String::new(),
            },
            "",
        );
        let document = add_payload
            .pointer("/contentJson/document")
            .cloned()
            .unwrap();
        let edit_body = json!({
            "title": "수정 제목",
            "contentJson": { "document": document },
            "documentId": "",
            "isCleanbotDisabled": false,
            "danglingImages": [],
        });

        // 제목이 새 값으로 들어가고
        assert_eq!(edit_body["title"], "수정 제목");
        // 본문 텍스트가 add 빌더가 만든 SE 문서 구조 그대로 새 값으로 들어간다.
        assert_eq!(
            edit_body["contentJson"]["document"]["components"][0]["value"][0]["nodes"][0]["value"],
            "수정 본문"
        );
        assert_eq!(edit_body["contentJson"]["document"]["version"], "2.9.0");
        // edit body 고정 필드(패킷 실측)
        assert_eq!(edit_body["documentId"], "");
        assert_eq!(edit_body["isCleanbotDisabled"], false);
        assert_eq!(edit_body["danglingImages"], json!([]));
        // add 전용 필드(txId·discussionType·itemCode)는 edit body에 없어야 한다.
        assert!(edit_body.get("txId").is_none());
        assert!(edit_body.get("discussionType").is_none());
    }

    #[test]
    fn choose_unused_nickname_picks_first_not_in_used() {
        let used: std::collections::HashSet<String> = ["곰돌이".to_owned(), "너구리".to_owned()]
            .into_iter()
            .collect();
        let candidates = vec![
            "곰돌이".to_owned(),
            "너구리".to_owned(),
            "다람쥐".to_owned(),
        ];

        assert_eq!(
            choose_unused_nickname(&candidates, &used).as_deref(),
            Some("다람쥐")
        );
    }

    #[test]
    fn choose_unused_nickname_falls_back_to_last_when_all_used() {
        let used: std::collections::HashSet<String> =
            ["가".to_owned(), "나".to_owned(), "다".to_owned()]
                .into_iter()
                .collect();
        let candidates = vec!["가".to_owned(), "나".to_owned(), "다".to_owned()];

        // 전부 겹치면 마지막 후보를 그대로 돌려준다.
        assert_eq!(
            choose_unused_nickname(&candidates, &used).as_deref(),
            Some("다")
        );
    }

    #[test]
    fn choose_unused_nickname_returns_none_for_empty_candidates() {
        let used: std::collections::HashSet<String> = std::collections::HashSet::new();
        assert_eq!(choose_unused_nickname(&[], &used), None);
    }

    #[test]
    fn build_comment_form_contains_captured_create_packet_fields() {
        let form = build_comment_form(
            "421063210",
            "https://stock.naver.com/domestic/stock/005930/discussion/421063210",
            "댓글 내용",
            "token-123",
        );
        let pairs = url::form_urlencoded::parse(form.as_bytes())
            .map(|(key, value)| (key.into_owned(), value.into_owned()))
            .collect::<BTreeMap<_, _>>();

        assert_eq!(pairs.get("objectId").map(String::as_str), Some("421063210"));
        assert_eq!(
            pairs.get("objectUrl").map(String::as_str),
            Some("https://stock.naver.com/domestic/stock/005930/discussion/421063210")
        );
        assert_eq!(pairs.get("contents").map(String::as_str), Some("댓글 내용"));
        assert_eq!(pairs.get("commentType").map(String::as_str), Some("txt"));
        assert_eq!(
            pairs.get("validateBanWords").map(String::as_str),
            Some("true")
        );
        assert_eq!(
            pairs.get("cbox_token").map(String::as_str),
            Some("token-123")
        );
    }
}
