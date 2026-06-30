//! PostgreSQL 저장소 (사수 확정 DB, PR #324). sqlx 런타임 쿼리만 사용 → 빌드 시 DB 불필요.
//! 운영 경로. `DATABASE_URL`이 설정되면 main이 이 구현을 쓴다.
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::postgres::PgPoolOptions;
use sqlx::{PgPool, Row};
use uuid::Uuid;

use super::Repository;
use crate::error::{AppError, AppResult};
use crate::model::{AuditEntry, Device, DeviceCode, DeviceState, Operator, Role, StagedAccount};

/// 스키마(멱등). `server/migrations/0001_init.sql`과 동일 내용.
const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS operators (
  login_id TEXT PRIMARY KEY,
  pw_hash TEXT NOT NULL,
  role TEXT NOT NULL,
  approved BOOLEAN NOT NULL DEFAULT FALSE,
  must_change_password BOOLEAN NOT NULL DEFAULT FALSE,
  token_version BIGINT NOT NULL DEFAULT 1
);
CREATE TABLE IF NOT EXISTS devices (
  id UUID PRIMARY KEY,
  name TEXT NOT NULL,
  ip TEXT,
  state TEXT NOT NULL,
  last_seen TIMESTAMPTZ NOT NULL
);
CREATE TABLE IF NOT EXISTS device_codes (
  code TEXT PRIMARY KEY,
  created_at TIMESTAMPTZ NOT NULL,
  used BOOLEAN NOT NULL DEFAULT FALSE
);
CREATE TABLE IF NOT EXISTS staged_accounts (
  id UUID PRIMARY KEY,
  login_id TEXT NOT NULL,
  pw_cipher TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS audit_log (
  id UUID PRIMARY KEY,
  ts TIMESTAMPTZ NOT NULL,
  tag TEXT NOT NULL,
  dir TEXT NOT NULL,
  device TEXT NOT NULL,
  msg TEXT NOT NULL,
  level TEXT NOT NULL
);
"#;

pub struct PostgresRepo {
    pool: PgPool,
}

fn role_str(r: Role) -> &'static str {
    match r {
        Role::Super => "super",
        Role::Operator => "operator",
    }
}
fn role_from(s: &str) -> Role {
    match s {
        "super" => Role::Super,
        _ => Role::Operator,
    }
}
fn state_str(s: DeviceState) -> &'static str {
    match s {
        DeviceState::Online => "online",
        DeviceState::Rotating => "rotating",
        DeviceState::Reconnecting => "reconnecting",
        DeviceState::Offline => "offline",
    }
}
fn state_from(s: &str) -> DeviceState {
    match s {
        "online" => DeviceState::Online,
        "rotating" => DeviceState::Rotating,
        "reconnecting" => DeviceState::Reconnecting,
        _ => DeviceState::Offline,
    }
}

fn db_err(e: sqlx::Error) -> AppError {
    AppError::Internal(format!("DB 오류: {e}"))
}

impl PostgresRepo {
    pub async fn connect(url: &str) -> AppResult<Self> {
        let pool = PgPoolOptions::new()
            .max_connections(8)
            .connect(url)
            .await
            .map_err(db_err)?;
        sqlx::query(SCHEMA).execute(&pool).await.map_err(db_err)?;
        Ok(Self { pool })
    }
}

#[async_trait]
impl Repository for PostgresRepo {
    async fn create_operator(&self, op: Operator) -> AppResult<()> {
        sqlx::query(
            "INSERT INTO operators (login_id, pw_hash, role, approved, must_change_password, token_version)
             VALUES ($1,$2,$3,$4,$5,$6)
             ON CONFLICT (login_id) DO NOTHING",
        )
        .bind(&op.login_id)
        .bind(&op.pw_hash)
        .bind(role_str(op.role))
        .bind(op.approved)
        .bind(op.must_change_password)
        .bind(op.token_version)
        .execute(&self.pool)
        .await
        .map_err(db_err)?;
        Ok(())
    }
    async fn find_operator(&self, login_id: &str) -> AppResult<Option<Operator>> {
        let row = sqlx::query("SELECT * FROM operators WHERE login_id = $1")
            .bind(login_id)
            .fetch_optional(&self.pool)
            .await
            .map_err(db_err)?;
        Ok(row.map(|r| Operator {
            login_id: r.get("login_id"),
            pw_hash: r.get("pw_hash"),
            role: role_from(r.get::<String, _>("role").as_str()),
            approved: r.get("approved"),
            must_change_password: r.get("must_change_password"),
            token_version: r.get("token_version"),
        }))
    }
    async fn list_operators(&self) -> AppResult<Vec<Operator>> {
        let rows = sqlx::query("SELECT * FROM operators ORDER BY login_id")
            .fetch_all(&self.pool)
            .await
            .map_err(db_err)?;
        Ok(rows
            .into_iter()
            .map(|r| Operator {
                login_id: r.get("login_id"),
                pw_hash: r.get("pw_hash"),
                role: role_from(r.get::<String, _>("role").as_str()),
                approved: r.get("approved"),
                must_change_password: r.get("must_change_password"),
                token_version: r.get("token_version"),
            })
            .collect())
    }
    async fn set_operator_approved(&self, login_id: &str, approved: bool) -> AppResult<()> {
        sqlx::query("UPDATE operators SET approved=$1 WHERE login_id=$2")
            .bind(approved)
            .bind(login_id)
            .execute(&self.pool)
            .await
            .map_err(db_err)?;
        Ok(())
    }
    async fn delete_operator(&self, login_id: &str) -> AppResult<()> {
        sqlx::query("DELETE FROM operators WHERE login_id=$1")
            .bind(login_id)
            .execute(&self.pool)
            .await
            .map_err(db_err)?;
        Ok(())
    }
    async fn set_operator_password(
        &self,
        login_id: &str,
        new_hash: &str,
        must_change: bool,
    ) -> AppResult<()> {
        sqlx::query(
            "UPDATE operators SET pw_hash=$1, token_version=token_version+1, must_change_password=$2 WHERE login_id=$3",
        )
        .bind(new_hash)
        .bind(must_change)
        .bind(login_id)
        .execute(&self.pool)
        .await
        .map_err(db_err)?;
        Ok(())
    }
    async fn count_super_admins(&self) -> AppResult<usize> {
        let row = sqlx::query("SELECT COUNT(*) AS n FROM operators WHERE role='super'")
            .fetch_one(&self.pool)
            .await
            .map_err(db_err)?;
        Ok(row.get::<i64, _>("n") as usize)
    }

    async fn create_device(&self, d: Device) -> AppResult<()> {
        sqlx::query("INSERT INTO devices (id,name,ip,state,last_seen) VALUES ($1,$2,$3,$4,$5)")
            .bind(d.id)
            .bind(&d.name)
            .bind(&d.ip)
            .bind(state_str(d.state))
            .bind(d.last_seen)
            .execute(&self.pool)
            .await
            .map_err(db_err)?;
        Ok(())
    }
    async fn find_device(&self, id: Uuid) -> AppResult<Option<Device>> {
        let row = sqlx::query("SELECT * FROM devices WHERE id=$1")
            .bind(id)
            .fetch_optional(&self.pool)
            .await
            .map_err(db_err)?;
        Ok(row.map(|r| Device {
            id: r.get("id"),
            name: r.get("name"),
            ip: r.get("ip"),
            state: state_from(r.get::<String, _>("state").as_str()),
            last_seen: r.get("last_seen"),
        }))
    }
    async fn list_devices(&self) -> AppResult<Vec<Device>> {
        let rows = sqlx::query("SELECT * FROM devices ORDER BY name")
            .fetch_all(&self.pool)
            .await
            .map_err(db_err)?;
        Ok(rows
            .into_iter()
            .map(|r| Device {
                id: r.get("id"),
                name: r.get("name"),
                ip: r.get("ip"),
                state: state_from(r.get::<String, _>("state").as_str()),
                last_seen: r.get("last_seen"),
            })
            .collect())
    }
    async fn delete_device(&self, id: Uuid) -> AppResult<bool> {
        let r = sqlx::query("DELETE FROM devices WHERE id=$1")
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(db_err)?;
        Ok(r.rows_affected() > 0)
    }
    async fn touch_device(
        &self,
        id: Uuid,
        ip: Option<String>,
        state: DeviceState,
        last_seen: DateTime<Utc>,
    ) -> AppResult<()> {
        // ip가 None이면 기존 ip 유지(COALESCE).
        sqlx::query(
            "UPDATE devices SET ip=COALESCE($1, ip), state=$2, last_seen=$3 WHERE id=$4",
        )
        .bind(ip)
        .bind(state_str(state))
        .bind(last_seen)
        .bind(id)
        .execute(&self.pool)
        .await
        .map_err(db_err)?;
        Ok(())
    }
    async fn set_device_state(&self, id: Uuid, state: DeviceState) -> AppResult<()> {
        sqlx::query("UPDATE devices SET state=$1 WHERE id=$2")
            .bind(state_str(state))
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(db_err)?;
        Ok(())
    }

    async fn create_device_code(&self, c: DeviceCode) -> AppResult<()> {
        sqlx::query("INSERT INTO device_codes (code,created_at,used) VALUES ($1,$2,$3)")
            .bind(&c.code)
            .bind(c.created_at)
            .bind(c.used)
            .execute(&self.pool)
            .await
            .map_err(db_err)?;
        Ok(())
    }
    async fn consume_device_code(&self, code: &str, ttl_secs: i64) -> AppResult<bool> {
        // 미사용·미만료면 used=true로 1회 소비(원자적 UPDATE … RETURNING).
        let row = sqlx::query(
            "UPDATE device_codes SET used=TRUE
             WHERE code=$1 AND used=FALSE
               AND created_at > (now() - make_interval(secs => $2))
             RETURNING code",
        )
        .bind(code)
        .bind(ttl_secs as f64)
        .fetch_optional(&self.pool)
        .await
        .map_err(db_err)?;
        Ok(row.is_some())
    }

    async fn add_staged_accounts(
        &self,
        accounts: Vec<StagedAccount>,
    ) -> AppResult<(usize, usize)> {
        let (mut imported, mut skipped) = (0usize, 0usize);
        for a in accounts {
            // 같은 login_id 이미 있으면 건너뜀.
            let exists = sqlx::query("SELECT 1 FROM staged_accounts WHERE login_id=$1")
                .bind(&a.login_id)
                .fetch_optional(&self.pool)
                .await
                .map_err(db_err)?;
            if exists.is_some() {
                skipped += 1;
                continue;
            }
            sqlx::query("INSERT INTO staged_accounts (id,login_id,pw_cipher) VALUES ($1,$2,$3)")
                .bind(a.id)
                .bind(&a.login_id)
                .bind(&a.pw_cipher)
                .execute(&self.pool)
                .await
                .map_err(db_err)?;
            imported += 1;
        }
        Ok((imported, skipped))
    }
    async fn list_staged_accounts(&self) -> AppResult<Vec<StagedAccount>> {
        let rows = sqlx::query("SELECT * FROM staged_accounts ORDER BY login_id")
            .fetch_all(&self.pool)
            .await
            .map_err(db_err)?;
        Ok(rows
            .into_iter()
            .map(|r| StagedAccount {
                id: r.get("id"),
                login_id: r.get("login_id"),
                pw_cipher: r.get("pw_cipher"),
            })
            .collect())
    }
    async fn take_staged_accounts(&self, ids: &[Uuid]) -> AppResult<Vec<StagedAccount>> {
        if ids.is_empty() {
            return Ok(vec![]);
        }
        // MOVE: 삭제하면서 삭제된 행을 그대로 반환(§7).
        let rows = sqlx::query(
            "DELETE FROM staged_accounts WHERE id = ANY($1) RETURNING id, login_id, pw_cipher",
        )
        .bind(ids)
        .fetch_all(&self.pool)
        .await
        .map_err(db_err)?;
        Ok(rows
            .into_iter()
            .map(|r| StagedAccount {
                id: r.get("id"),
                login_id: r.get("login_id"),
                pw_cipher: r.get("pw_cipher"),
            })
            .collect())
    }

    async fn add_audit(&self, e: AuditEntry) -> AppResult<()> {
        sqlx::query("INSERT INTO audit_log (id,ts,tag,dir,device,msg,level) VALUES ($1,$2,$3,$4,$5,$6,$7)")
            .bind(e.id)
            .bind(e.ts)
            .bind(&e.tag)
            .bind(&e.dir)
            .bind(&e.device)
            .bind(&e.msg)
            .bind(&e.level)
            .execute(&self.pool)
            .await
            .map_err(db_err)?;
        Ok(())
    }
    async fn list_audit(&self) -> AppResult<Vec<AuditEntry>> {
        let rows = sqlx::query("SELECT * FROM audit_log ORDER BY ts")
            .fetch_all(&self.pool)
            .await
            .map_err(db_err)?;
        Ok(rows
            .into_iter()
            .map(|r| AuditEntry {
                id: r.get("id"),
                ts: r.get("ts"),
                tag: r.get("tag"),
                dir: r.get("dir"),
                device: r.get("device"),
                msg: r.get("msg"),
                level: r.get("level"),
            })
            .collect())
    }
}
