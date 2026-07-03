//! 예약 게시(07-게시명령 4단계). Admin이 만든 예약을 **서버가 보관**하고, 시각이 되면 **서버
//! 스케줄러**가 그 하위로 `publish_posts`를 발송한다(클라이언트 setTimeout 아님). 예약을 삭제하면
//! **무엇을(글·대상·계정×종목) 지웠는지 원문 전부**를 통신로그에 남긴다(사용자 지시).
//!
//! 저장은 인벤토리와 같은 이유로 메모리(최신 목록)에 둔다 — 개발 기본(in-memory) 저장소와 일관.
//! 게시 발송 경로(payload 조립 + 원문 audit)는 즉시 게시(`issue_publish`)와 **완전히 동일한
//! 함수**(`dispatch_publish`)를 재사용해, 예약이든 즉시든 하위가 받는 명령·로그가 같게 한다.

use std::time::Duration;

use chrono::Utc;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::model::Device;
use crate::state::AppState;

// ── 게시 명령 공유 타입(즉시 게시·예약 게시가 함께 쓴다) ──

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PublishStock {
    pub code: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PublishAssignment {
    pub login_id: String,
    pub stocks: Vec<PublishStock>,
}

/// 게시 1건의 확정 명세(글·대상·계정×종목). 즉시/예약 공통.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PublishSpec {
    pub post_id: String,
    #[serde(default)]
    pub post_title: String,
    #[serde(default)]
    pub target_label: String,
    #[serde(default)]
    pub split: bool,
    pub assignments: Vec<PublishAssignment>,
}

impl PublishSpec {
    /// 계정×종목을 사람이 읽는 원문 문자열로(로그·상세용). 자르지 않는다.
    pub fn detail(&self) -> String {
        self.assignments
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
            .join(" · ")
    }

    /// 대상 라벨(빈 값이면 종목토론방 기본).
    pub fn target_label_or_default(&self) -> String {
        if self.target_label.is_empty() {
            "종목토론방".to_string()
        } else {
            self.target_label.clone()
        }
    }
}

/// 게시 명령 발송(즉시·예약 공통 경로). 하위 SSE로 `publish_posts` payload를 내려보내고 **계정×종목·
/// payload 원문 전체**를 통신로그에 남긴다. `origin`은 발송 출처 설명(예 `operator=kim`,`예약 스케줄러`).
pub async fn dispatch_publish(
    st: &AppState,
    device: &Device,
    cid: &str,
    spec: &PublishSpec,
    origin: &str,
) {
    let target_label = spec.target_label_or_default();
    let assignments_json: Vec<serde_json::Value> = spec
        .assignments
        .iter()
        .map(|a| {
            serde_json::json!({
                "loginId": a.login_id,
                "stocks": a.stocks.iter()
                    .map(|s| serde_json::json!({ "code": s.code, "name": s.name }))
                    .collect::<Vec<_>>(),
            })
        })
        .collect();
    let payload = serde_json::json!({
        "type": "publish_posts",
        "commandId": cid,
        "publish": {
            "postId": spec.post_id,
            "postTitle": spec.post_title,
            "targetLabel": target_label,
            "split": spec.split,
            "assignments": assignments_json,
        }
    });
    st.hub.device_push(device.id, payload.to_string());

    st.audit(
        "[CMD]",
        &format!("{} → {}", origin_dir(origin), device.name),
        &device.id.to_string(),
        &format!(
            "publish_posts(게시 명령) commandId={cid} {origin} · 글=\"{}\"(postId={}) · 대상={target_label} · 방식={} · 계정×종목: {} · payload={payload}",
            spec.post_title,
            spec.post_id,
            if spec.split { "나눠서" } else { "전체" },
            spec.detail(),
        ),
        "cmd",
    )
    .await;
}

/// 통신로그 `dir` 필드 앞부분(발송 주체). operator=... 면 "Admin", 예약이면 "예약 스케줄러".
fn origin_dir(origin: &str) -> &str {
    if origin.contains("예약") {
        "예약 스케줄러"
    } else {
        "Admin"
    }
}

// ── 예약 저장 모델 ──

/// 서버가 보관하는 예약 1건 = 발송에 필요한 전체 명세 + 표시 필드 + 발송 시각.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScheduledPost {
    pub id: String,
    pub device_id: Uuid,
    pub device_name: String,
    #[serde(flatten)]
    pub spec: PublishSpec,
    /// 발송 시각(epoch ms). 이 시각 이하가 되면 스케줄러가 발송한다.
    pub at: i64,
    /// 예약된 글 화면 표시용 요약(계정/종목 분배 설명).
    #[serde(default)]
    pub detail: String,
    #[serde(default)]
    pub created_at: Option<String>,
}

/// Admin '예약된 글' 목록 응답(표시 전용 — 프론트 `ScheduledItem`과 동일 모양).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScheduledDto {
    pub id: String,
    pub device_name: String,
    pub post_title: String,
    pub target_label: String,
    pub detail: String,
    pub at: i64,
}

impl ScheduledPost {
    pub fn to_dto(&self) -> ScheduledDto {
        ScheduledDto {
            id: self.id.clone(),
            device_name: self.device_name.clone(),
            post_title: self.spec.post_title.clone(),
            target_label: self.spec.target_label_or_default(),
            detail: self.detail.clone(),
            at: self.at,
        }
    }
}

/// 현재 시각(epoch ms).
pub fn now_ms() -> i64 {
    Utc::now().timestamp_millis()
}

/// `items`에서 발송 시각이 지난(at <= now) 예약을 **꺼내** 반환하고, 목록엔 미도래만 남긴다.
/// 순수함수(테스트 대상) — 스케줄러가 매 틱 호출한다.
pub fn drain_due(items: &mut Vec<ScheduledPost>, now: i64) -> Vec<ScheduledPost> {
    let mut due = Vec::new();
    let mut remaining = Vec::new();
    for it in items.drain(..) {
        if it.at <= now {
            due.push(it);
        } else {
            remaining.push(it);
        }
    }
    *items = remaining;
    due
}

/// 서버 스케줄러 — 1초마다 도래한 예약을 발송한다(§07 4단계). 온라인이면 `dispatch_publish`로
/// 즉시 게시와 동일 경로 발송, 오프라인/삭제된 하위면 발송 못 함을 통신로그에 남긴다. 어느 쪽이든
/// 도래분은 목록에서 제거한다(과거 시각을 계속 붙들지 않음). main에서 spawn.
pub async fn scheduler_loop(st: AppState) {
    loop {
        tokio::time::sleep(Duration::from_secs(1)).await;
        let due = {
            let mut g = st.scheduled.lock().unwrap();
            drain_due(&mut g, now_ms())
        };
        for item in due {
            let cid = format!("c-{}", Uuid::new_v4());
            match st.repo.find_device(item.device_id).await {
                Ok(Some(device)) if AppState::is_commandable(device.state) => {
                    dispatch_publish(&st, &device, &cid, &item.spec, "예약 스케줄러").await;
                }
                Ok(Some(device)) => {
                    st.audit(
                        "[예약]",
                        &format!("예약 스케줄러 → {}", device.name),
                        &item.device_id.to_string(),
                        &format!(
                            "예약 발송 실패(대상 online 아님, {:?}): 글=\"{}\"(postId={}) · 대상={} · 계정×종목: {}",
                            device.state,
                            item.spec.post_title,
                            item.spec.post_id,
                            item.spec.target_label_or_default(),
                            item.spec.detail(),
                        ),
                        "fail",
                    )
                    .await;
                }
                _ => {
                    st.audit(
                        "[예약]",
                        "예약 스케줄러 → 서버",
                        &item.device_id.to_string(),
                        &format!(
                            "예약 발송 실패(대상 기기 없음/삭제됨): 글=\"{}\"(postId={}) · 계정×종목: {}",
                            item.spec.post_title,
                            item.spec.post_id,
                            item.spec.detail(),
                        ),
                        "fail",
                    )
                    .await;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sched(id: &str, at: i64) -> ScheduledPost {
        ScheduledPost {
            id: id.into(),
            device_id: Uuid::nil(),
            device_name: "하위-001".into(),
            spec: PublishSpec {
                post_id: "p1".into(),
                post_title: "글".into(),
                target_label: String::new(),
                split: false,
                assignments: vec![PublishAssignment {
                    login_id: "acc".into(),
                    stocks: vec![PublishStock { code: "005930".into(), name: "삼성전자".into() }],
                }],
            },
            at,
            detail: "종목 1 · 계정 1 전체".into(),
            created_at: None,
        }
    }

    #[test]
    fn drain_due_takes_only_past_and_keeps_future() {
        let mut items = vec![sched("a", 100), sched("b", 200), sched("c", 300)];
        let due = drain_due(&mut items, 200);
        let due_ids: Vec<&str> = due.iter().map(|d| d.id.as_str()).collect();
        assert_eq!(due_ids, vec!["a", "b"], "at<=now 만 발송 대상");
        let left: Vec<&str> = items.iter().map(|d| d.id.as_str()).collect();
        assert_eq!(left, vec!["c"], "미도래는 남는다");
    }

    #[test]
    fn drain_due_empty_when_none_due() {
        let mut items = vec![sched("a", 1000)];
        let due = drain_due(&mut items, 500);
        assert!(due.is_empty());
        assert_eq!(items.len(), 1);
    }

    #[test]
    fn spec_detail_and_target_default() {
        let s = sched("a", 1);
        assert_eq!(s.spec.detail(), "acc=[삼성전자(005930)]");
        assert_eq!(s.spec.target_label_or_default(), "종목토론방");
    }

    #[test]
    fn dto_maps_display_fields() {
        let dto = sched("a", 42).to_dto();
        assert_eq!(dto.id, "a");
        assert_eq!(dto.post_title, "글");
        assert_eq!(dto.target_label, "종목토론방");
        assert_eq!(dto.at, 42);
    }

    #[test]
    fn origin_dir_distinguishes_schedule() {
        assert_eq!(origin_dir("operator=kim"), "Admin");
        assert_eq!(origin_dir("예약 스케줄러"), "예약 스케줄러");
    }
}
