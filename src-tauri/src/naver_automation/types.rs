use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
// 자동화 실행에 필요한 입력값을 담는 구조체입니다.
pub struct NaverDiscussionRequest {
    pub title: String,
    pub body: String,
    pub host: String,
    pub port: u16,
    pub target: AutomationTarget,
    pub submit_after_fill: bool,
    #[serde(default)]
    pub stock: Option<DiscussionStock>,
    // 로그인 자동화로 저장된 계정 ID. 지정되면 해당 계정의 쿠키를 Chrome에 주입합니다.
    // None이면 기존처럼 Chrome에 이미 로그인된 세션의 쿠키를 사용합니다.
    #[serde(default)]
    pub account_id: Option<String>,
    // 댓글(Comment) 대상이 "특정 게시글"일 때, 그 글의 종목토론방 URL. 지정되면 댓글은
    // 랜덤 글을 고르지 않고 이 URL로 직접 이동해 그 글에 달린다. None/빈값이면 기존처럼
    // 선택 종목토론방의 랜덤 글에 댓글을 단다(하위호환).
    #[serde(default)]
    pub comment_url: Option<String>,
    // 닉네임 랜덤 댓글(설계서 §2): 댓글(Comment) 대상일 때 submit_comment 직전에 프로필 닉네임을
    // 랜덤으로 바꾼다(계정 내 중복 금지 — used 집합은 호출부가 계정 단위로 관리). 기본 false(무변경).
    #[serde(default)]
    pub comment_nickname_random: bool,
    // 글 내용 변경(설계서 §5): 채워지면 글 게시(submit_post) 후 delay_sec초 뒤 새 제목/본문으로
    // edit한다(글 대상 전용). None이면 변경하지 않는다(기본).
    #[serde(default)]
    pub content_change: Option<crate::ipc::queue::ContentChange>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
// 글쓰기 후 방금 작성한 글에 댓글까지 이어서 달 때 필요한 입력값을 담는 구조체입니다.
pub struct NaverPostWithCommentRequest {
    pub title: String,
    pub body: String,
    // 방금 만든 내 글에 달 댓글 풀(설계서 §2 확장). 풀 전체를 작성 순서대로 단다 — 닉네임 랜덤이
    // 켜져 있으면 댓글마다 닉네임을 회전한다. 단일 댓글 경로(CSV 배치 등)는 원소 1개짜리 풀을 넘긴다.
    pub comments: Vec<String>,
    pub host: String,
    pub port: u16,
    #[serde(default)]
    pub stock: Option<DiscussionStock>,
    // 로그인 자동화로 저장된 계정 ID. 지정되면 해당 계정의 쿠키를 Chrome에 주입합니다.
    #[serde(default)]
    pub account_id: Option<String>,
    // 닉네임 랜덤 댓글(설계서 §2): 댓글 직전에 프로필 닉네임을 계정 내 중복 없이 바꾼다. 기본 false.
    #[serde(default)]
    pub comment_nickname_random: bool,
    // 글 내용 변경(설계서 §5): 채워지면 글 게시 후 delay_sec초 뒤 edit한다(글→edit→댓글 순서). 기본 None.
    #[serde(default)]
    pub content_change: Option<crate::ipc::queue::ContentChange>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
// 자동화 대상이 글쓰기인지 댓글쓰기인지 구분하는 enum입니다.
pub enum AutomationTarget {
    Post,
    Comment,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
// 사용자가 UI에서 선택한 종목명, 종목코드, 링크를 담는 구조체입니다.
pub struct DiscussionStock {
    pub name: String,
    pub code: String,
    pub link: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
// 랜덤으로 선택된 토론방 정보를 담는 구조체입니다.
pub struct DiscussionSelection {
    pub category: String,
    pub rank: String,
    pub item_text: String,
    pub method: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
// 자동화 실행 결과를 CLI와 Tauri UI에 돌려주는 구조체입니다.
pub struct AutomationReport {
    pub current_url: String,
    /// 글쓰기 성공 시 작성된 글의 URL(add 응답 id 기반). 글이 아니면 None.
    #[serde(default)]
    pub post_url: Option<String>,
    pub login_profile: NaverLoginProfile,
    pub register_button_highlighted: bool,
    pub submitted: bool,
    pub selected: DiscussionSelection,
    pub target: AutomationTarget,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
// getProfile 패킷 기반 로그인 확인 결과를 담는 구조체입니다.
pub struct NaverLoginProfile {
    pub logged_in: bool,
    pub nickname: Option<String>,
    pub image_url: Option<String>,
    pub message: String,
}
