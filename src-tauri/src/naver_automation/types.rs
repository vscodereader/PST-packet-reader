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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
// 글쓰기 후 방금 작성한 글에 댓글까지 이어서 달 때 필요한 입력값을 담는 구조체입니다.
pub struct NaverPostWithCommentRequest {
    pub title: String,
    pub body: String,
    pub comment: String,
    pub host: String,
    pub port: u16,
    #[serde(default)]
    pub stock: Option<DiscussionStock>,
    // 로그인 자동화로 저장된 계정 ID. 지정되면 해당 계정의 쿠키를 Chrome에 주입합니다.
    #[serde(default)]
    pub account_id: Option<String>,
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
