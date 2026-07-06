//! 공유 상태 + 인증/감사 헬퍼.
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use axum::http::{header::AUTHORIZATION, HeaderMap};
use chrono::Utc;
use uuid::Uuid;

use crate::config::Config;
use crate::error::{AppError, AppResult};
use crate::hub::Hub;
use crate::model::{
    AuditDto, AuditEntry, DailyResultDto, Device, DeviceInventory, DeviceQueueState,
    DeviceStopReport, LoginBatchDto, Operator, Role, StopLineDto,
};
use crate::repo::Repository;
use crate::{jwt, model::DeviceState};

#[derive(Clone)]
pub struct AppState {
    pub repo: Arc<dyn Repository>,
    pub hub: Arc<Hub>,
    pub cfg: Arc<Config>,
    /// 존재하지 않는 운영자 로그인 시에도 argon2를 한 번 돌려 타이밍을 맞추기 위한 더미 해시(계정 열거 방지).
    pub dummy_pw_hash: Arc<String>,
    /// 하위 인벤토리(글목록·성공계정) — 하위가 주기적으로 보고하는 **실시간 상태**라 DB가 아니라
    /// 메모리에 최신 1건만 둔다(서버 재시작해도 하위가 곧 재보고). 07-게시명령 3단계.
    pub inventory: Arc<Mutex<HashMap<Uuid, DeviceInventory>>>,
    /// 하위 실행/대기 게시큐 스냅샷 최신 1건(설계서 08 §10-2). 인벤토리와 같이 메모리 보관 —
    /// Admin "중지 명령" 페이지가 폴링해 실시간으로 본다.
    pub queue_states: Arc<Mutex<HashMap<Uuid, DeviceQueueState>>>,
    /// 하위 중지(kill) 요약 누적(설계서 08 §10-3). 결과보고 "중지" 섹션이 렌더. 메모리 보관.
    pub stop_reports: Arc<Mutex<HashMap<Uuid, DeviceStopReport>>>,
    /// 하위별·날짜별 결과 집계(날짜 분류). 로그인 4분류 + 중지를 그 날(KST) 버킷에 합산해,
    /// 결과보고에서 하위별로 날짜를 골라 그 날 결과만 보게 한다(날짜 섞임 방지). 메모리·최근 90일.
    pub daily: Arc<Mutex<HashMap<Uuid, std::collections::BTreeMap<String, DailyResultDto>>>>,
    /// 예약 게시 목록 — 서버가 보관하고 스케줄러가 시각되면 발송한다(07-게시명령 4단계). 인벤토리와
    /// 같은 이유로 메모리 보관(개발 기본 in-memory 저장소와 일관).
    pub scheduled: Arc<Mutex<Vec<crate::scheduled::ScheduledPost>>>,
}

fn bearer(headers: &HeaderMap) -> AppResult<String> {
    let raw = headers
        .get(AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| AppError::Unauthorized("인증 토큰 없음".into()))?;
    raw.strip_prefix("Bearer ")
        .map(|s| s.to_string())
        .ok_or_else(|| AppError::Unauthorized("Bearer 토큰 형식 오류".into()))
}

impl AppState {
    /// 운영자 토큰 검증 단일 통로: JWT 서명·만료 + DB 토큰버전 일치 + 존재 + 승인(§5).
    /// 헤더 경로와 SSE 쿼리 경로가 모두 이 함수를 거쳐 게이트가 어긋나지 않게 한다.
    pub async fn auth_operator_token(&self, token: &str) -> AppResult<Operator> {
        let claims = jwt::verify_operator(&self.cfg.jwt_secret, token)
            .map_err(|_| AppError::Unauthorized("토큰 무효/만료 — 재로그인".into()))?;
        let op = self
            .repo
            .find_operator(&claims.sub)
            .await?
            .ok_or_else(|| AppError::Unauthorized("삭제된 운영자".into()))?;
        if op.token_version != claims.ver {
            return Err(AppError::Unauthorized("세션 만료(비번 변경/강제 로그아웃) — 재로그인".into()));
        }
        if !op.approved {
            return Err(AppError::Forbidden("승인 대기 중인 계정".into()));
        }
        Ok(op)
    }

    /// 운영자 인증(헤더 Bearer). 내부적으로 `auth_operator_token`을 거친다(§5).
    pub async fn auth_operator(&self, headers: &HeaderMap) -> AppResult<Operator> {
        let token = bearer(headers)?;
        self.auth_operator_token(&token).await
    }

    /// SuperAdmin 전용 동작 게이트(운영자 삭제·비번 재설정, §5).
    pub async fn auth_super(&self, headers: &HeaderMap) -> AppResult<Operator> {
        let op = self.auth_operator(headers).await?;
        if op.role != Role::Super {
            return Err(AppError::Forbidden("SuperAdmin 전용 동작".into()));
        }
        Ok(op)
    }

    /// 기기 인증: 기기 JWT 서명 + DB에 그 device 존재(삭제=회수, §6-4).
    pub async fn auth_device(&self, headers: &HeaderMap) -> AppResult<Device> {
        let token = bearer(headers)?;
        let claims = jwt::verify_device(&self.cfg.jwt_secret, &token)
            .map_err(|_| AppError::Unauthorized("기기 토큰 무효".into()))?;
        let id = Uuid::parse_str(&claims.sub)
            .map_err(|_| AppError::Unauthorized("기기 토큰 형식 오류".into()))?;
        self.repo
            .find_device(id)
            .await?
            .ok_or_else(|| AppError::Unauthorized("등록 해제된 기기 — 재등록 필요(§6-4)".into()))
    }

    /// 감사로그 1줄 기록 + Admin SSE로 push(§10-5). 통신 로그 화면이 이걸 본다.
    pub async fn audit(&self, tag: &str, dir: &str, device: &str, msg: &str, level: &str) {
        let entry = AuditEntry {
            id: Uuid::new_v4(),
            ts: Utc::now(),
            tag: tag.into(),
            dir: dir.into(),
            device: device.into(),
            msg: msg.into(),
            level: level.into(),
        };
        let dto = AuditDto {
            ts: entry.ts.to_rfc3339(),
            tag: entry.tag.clone(),
            dir: entry.dir.clone(),
            device: entry.device.clone(),
            msg: entry.msg.clone(),
            level: entry.level.clone(),
        };
        // SSE push(실패해도 무시) → 감사로그 저장.
        if let Ok(j) = serde_json::to_string(&dto) {
            self.hub.admin_push(j);
        }
        // ★ 통신로그 화면(SSE+저장)뿐 아니라 **서버 로그 파일에도 원문 그대로** 남긴다(사용자 지시:
        //   로그 파일에서도 통신로그 화면에서도 전부 보여지게). 자르지 않는다. 레벨만 색/등급에 맞춘다.
        match level {
            "fail" => tracing::error!("{tag} {dir} [{device}] {msg}"),
            "warn" => tracing::warn!("{tag} {dir} [{device}] {msg}"),
            _ => tracing::info!("{tag} {dir} [{device}] {msg}"),
        }
        let _ = self.repo.add_audit(entry).await;
    }

    /// 기기 상태가 명령 가능(online)인지(§4-2 거부 판정용).
    pub fn is_commandable(state: DeviceState) -> bool {
        matches!(state, DeviceState::Online)
    }


    /// 하위 인벤토리 최신 1건 저장. 반환값 = 직전 내용과 **달라졌는지**(글목록·성공계정 기준).
    /// 통신로그는 바뀌었을 때만 남겨(주기 보고 도배 방지) 실제 변화만 기록한다.
    pub fn set_inventory(&self, id: Uuid, inv: DeviceInventory) -> bool {
        let mut g = self.inventory.lock().unwrap();
        let changed = inventory_changed(g.get(&id), &inv);
        g.insert(id, inv);
        changed
    }

    /// 하위 인벤토리 최신 1건 조회(없으면 None).
    pub fn get_inventory(&self, id: Uuid) -> Option<DeviceInventory> {
        self.inventory.lock().unwrap().get(&id).cloned()
    }

    /// 하위 실행큐 스냅샷 최신 1건 저장(설계서 08 §10-2). 주기 보고라 통신로그엔 안 남긴다
    /// (kill 명령만 원문 로그). Admin이 폴링으로 최신본을 읽는다.
    pub fn set_queue_state(&self, id: Uuid, qs: DeviceQueueState) {
        self.queue_states.lock().unwrap().insert(id, qs);
    }

    /// 하위 실행큐 스냅샷 최신 1건 조회(없으면 None).
    pub fn get_queue_state(&self, id: Uuid) -> Option<DeviceQueueState> {
        self.queue_states.lock().unwrap().get(&id).cloned()
    }

    /// 중지 요약을 디바이스별로 **누적**(개별 kill이 덮어써 사라지지 않게). 상한 200(오래된 것부터).
    pub fn set_stop_report(&self, id: Uuid, mut rpt: DeviceStopReport) {
        let mut g = self.stop_reports.lock().unwrap();
        let entry = g.entry(id).or_default();
        entry.stopped.append(&mut rpt.stopped);
        let len = entry.stopped.len();
        if len > 200 {
            entry.stopped.drain(0..len - 200);
        }
        entry.received_at = rpt.received_at;
    }

    /// 모든 디바이스의 중지 요약 스냅샷((id, report) 목록).
    pub fn stop_reports_snapshot(&self) -> Vec<(Uuid, DeviceStopReport)> {
        self.stop_reports
            .lock()
            .unwrap()
            .iter()
            .map(|(k, v)| (*k, v.clone()))
            .collect()
    }

    /// 로그인 결과 배치를 그 날(KST) 버킷에 합산(날짜 분류). 등록·누적은 무관, 4분류만 누적한다.
    pub fn add_login_daily(&self, id: Uuid, date: &str, batch: &LoginBatchDto) {
        let mut g = self.daily.lock().unwrap();
        let device_days = g.entry(id).or_default();
        {
            let day = device_days.entry(date.to_string()).or_default();
            day.date = date.to_string();
            day.success += batch.success;
            day.onhold.extend(batch.onhold.iter().cloned());
            day.timedout.extend(batch.timedout.iter().cloned());
            day.failed.extend(batch.failed.iter().cloned());
        }
        cap_days(device_days);
    }

    /// 중지(kill) 요약을 그 날(KST) 버킷에 합산(날짜 분류).
    pub fn add_stop_daily(&self, id: Uuid, date: &str, lines: &[StopLineDto]) {
        let mut g = self.daily.lock().unwrap();
        let device_days = g.entry(id).or_default();
        {
            let day = device_days.entry(date.to_string()).or_default();
            day.date = date.to_string();
            day.stopped.extend(lines.iter().cloned());
        }
        cap_days(device_days);
    }

    /// 하위별 날짜별 결과 스냅샷((id, 최신날짜 우선 목록)).
    pub fn daily_snapshot(&self) -> Vec<(Uuid, Vec<DailyResultDto>)> {
        self.daily
            .lock()
            .unwrap()
            .iter()
            .map(|(id, days)| {
                let mut v: Vec<DailyResultDto> = days.values().cloned().collect();
                v.sort_by(|a, b| b.date.cmp(&a.date)); // 최신 날짜 우선
                (*id, v)
            })
            .collect()
    }
}

/// 날짜 버킷을 최근 90일로 제한(오래된 날짜부터 제거). BTreeMap은 날짜 오름차순이라 앞이 가장 오래된 것.
fn cap_days(days: &mut std::collections::BTreeMap<String, DailyResultDto>) {
    while days.len() > 90 {
        let Some(oldest) = days.keys().next().cloned() else {
            break;
        };
        days.remove(&oldest);
    }
}

/// 인벤토리가 직전 대비 바뀌었는지(글목록·성공계정 기준). `received_at`은 매번 바뀌므로 비교에서
/// 제외한다 — 시각만 달라진 재보고를 "변화"로 보면 통신로그가 도배된다. 순수함수(테스트 대상).
fn inventory_changed(prev: Option<&DeviceInventory>, next: &DeviceInventory) -> bool {
    match prev {
        Some(p) => p.posts != next.posts || p.accounts != next.accounts,
        None => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::InvPost;

    fn inv(posts: &[(&str, &str)], accounts: &[&str]) -> DeviceInventory {
        DeviceInventory {
            posts: posts
                .iter()
                .map(|(id, t)| InvPost {
                    id: (*id).into(),
                    title: (*t).into(),
                    kind: "post".into(),
                    excerpt: String::new(),
                })
                .collect(),
            accounts: accounts.iter().map(|s| (*s).to_string()).collect(),
            // received_at은 비교 제외 대상이므로 일부러 채워 넣어도 결과가 같아야 한다.
            received_at: Some("2026-07-03T00:00:00Z".into()),
        }
    }

    #[test]
    fn first_report_is_always_a_change() {
        assert!(inventory_changed(None, &inv(&[("p1", "글")], &["a"])));
    }

    #[test]
    fn same_content_is_not_a_change_even_if_received_at_differs() {
        let prev = inv(&[("p1", "글")], &["a"]);
        let mut next = inv(&[("p1", "글")], &["a"]);
        next.received_at = Some("2099-01-01T00:00:00Z".into()); // 시각만 다름
        assert!(!inventory_changed(Some(&prev), &next), "시각만 바뀐 재보고는 변화 아님");
    }

    #[test]
    fn changed_posts_or_accounts_is_a_change() {
        let prev = inv(&[("p1", "글")], &["a"]);
        assert!(inventory_changed(Some(&prev), &inv(&[("p1", "글2")], &["a"])), "제목 변경");
        assert!(inventory_changed(Some(&prev), &inv(&[("p1", "글"), ("p2", "새글")], &["a"])), "글 추가");
        assert!(inventory_changed(Some(&prev), &inv(&[("p1", "글")], &["a", "b"])), "성공계정 추가");
    }
}
