-- Admin–하위 원격제어 서버 스키마 (PostgreSQL, 사수 확정 DB · PR #324). 설계 §7·§9.
-- 운영 시 적용: psql "$DATABASE_URL" -f server/migrations/0001_init.sql
-- (서버 기동 시에도 PostgresRepo::connect가 동일 스키마를 멱등 생성한다.)

CREATE TABLE IF NOT EXISTS operators (
  login_id             TEXT PRIMARY KEY,
  pw_hash              TEXT NOT NULL,                 -- argon2 해시(단방향, §5)
  role                 TEXT NOT NULL,                 -- 'super' | 'operator'
  approved             BOOLEAN NOT NULL DEFAULT FALSE,
  must_change_password BOOLEAN NOT NULL DEFAULT FALSE,
  token_version        BIGINT NOT NULL DEFAULT 1      -- 회수용(+1 시 옛 토큰 무효, §5)
);

CREATE TABLE IF NOT EXISTS devices (
  id        UUID PRIMARY KEY,
  name      TEXT NOT NULL,
  ip        TEXT,
  state     TEXT NOT NULL,                            -- online | rotating | reconnecting | offline (§4)
  last_seen TIMESTAMPTZ NOT NULL
);

CREATE TABLE IF NOT EXISTS device_codes (
  code       TEXT PRIMARY KEY,                        -- 1회용·10분(§6)
  created_at TIMESTAMPTZ NOT NULL,
  used       BOOLEAN NOT NULL DEFAULT FALSE
);

-- 계정 ID/PW 스테이징. pw_cipher는 AES-256-GCM 암호문만 저장(at-rest, §7).
CREATE TABLE IF NOT EXISTS staged_accounts (
  id        UUID PRIMARY KEY,
  login_id  TEXT NOT NULL,
  pw_cipher TEXT NOT NULL
);

-- 감사로그 = 통신로그 화면 출처(§10-5).
CREATE TABLE IF NOT EXISTS audit_log (
  id     UUID PRIMARY KEY,
  ts     TIMESTAMPTZ NOT NULL,
  tag    TEXT NOT NULL,                               -- [CMD] [RESULT] [HEARTBEAT] [SSE] [REGISTER] [STATE] [REJECT]
  dir    TEXT NOT NULL,
  device TEXT NOT NULL,
  msg    TEXT NOT NULL,
  level  TEXT NOT NULL                                -- cmd | ok | fail | info | warn
);
CREATE INDEX IF NOT EXISTS audit_log_ts_idx ON audit_log (ts);
