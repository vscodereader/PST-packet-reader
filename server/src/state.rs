//! 공유 상태 + 인증/감사 헬퍼.
use std::sync::Arc;

use axum::http::{header::AUTHORIZATION, HeaderMap};
use chrono::Utc;
use uuid::Uuid;

use crate::config::Config;
use crate::error::{AppError, AppResult};
use crate::hub::Hub;
use crate::model::{AuditDto, AuditEntry, Device, Operator, Role};
use crate::repo::Repository;
use crate::{jwt, model::DeviceState};

#[derive(Clone)]
pub struct AppState {
    pub repo: Arc<dyn Repository>,
    pub hub: Arc<Hub>,
    pub cfg: Arc<Config>,
    /// 존재하지 않는 운영자 로그인 시에도 argon2를 한 번 돌려 타이밍을 맞추기 위한 더미 해시(계정 열거 방지).
    pub dummy_pw_hash: Arc<String>,
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
}
