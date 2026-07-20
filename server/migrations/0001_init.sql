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
  id         UUID PRIMARY KEY,
  name       TEXT NOT NULL,
  ip         TEXT,
  state      TEXT NOT NULL,                           -- online | rotating | reconnecting | offline (§4)
  last_seen  TIMESTAMPTZ NOT NULL,
  machine_id TEXT                                     -- 기기 고유값(Windows MachineGuid 등, §E). 재설치·재등록에도 같은 PC=같은 기기
);
-- 기존 DB 업그레이드(멱등): 하위 기기 안정 식별(§E).
ALTER TABLE devices ADD COLUMN IF NOT EXISTS machine_id TEXT;

CREATE TABLE IF NOT EXISTS device_codes (
  code       TEXT PRIMARY KEY,                        -- 1회용·10분(§6)
  created_at TIMESTAMPTZ NOT NULL,
  used       BOOLEAN NOT NULL DEFAULT FALSE
);

-- 계정 ID/PW 스테이징. pw_cipher는 AES-256-GCM 암호문만 저장(at-rest, §7).
CREATE TABLE IF NOT EXISTS staged_accounts (
  id        UUID PRIMARY KEY,
  login_id  TEXT NOT NULL,
  pw_cipher TEXT NOT NULL,
  platform  TEXT NOT NULL DEFAULT 'forum'
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

-- 게시 결과 보고(§10-4-2). 하위가 올린 로컬 게시 완료 로그(LogBatch) 사본. items는
-- BatchItem 배열(platform/target/loginId/status/msg/trace?/posted?)을 JSONB로 보관.
-- (device_id, batch_id) PK로 재보고/재연결에도 중복 없이 멱등 UPSERT.
CREATE TABLE IF NOT EXISTS post_reports (
  device_id   UUID NOT NULL,
  batch_id    TEXT NOT NULL,
  device_name TEXT NOT NULL,
  title       TEXT NOT NULL,
  at          BIGINT NOT NULL,                          -- 게시 완료 epoch ms
  received_at TIMESTAMPTZ NOT NULL,
  items       JSONB NOT NULL,
  kind        TEXT NOT NULL DEFAULT '게시',             -- 결과 종류(15-기타명령 §6-3): 게시/좋아요/싫어요/조회수/IP
  PRIMARY KEY (device_id, batch_id)
);
ALTER TABLE post_reports ADD COLUMN IF NOT EXISTS kind TEXT NOT NULL DEFAULT '게시';
CREATE INDEX IF NOT EXISTS post_reports_received_idx ON post_reports (received_at);

-- 로그인 결과 보고(§10-4-1). 컴퓨터(device_id)당 최신 1건(누적이 합계를 담아 이력은 불필요).
-- batch=이번 분류(success 개수 + onhold/timedout/failed 줄), cumulative=그 하위 누적 합계.
CREATE TABLE IF NOT EXISTS login_reports (
  device_id          UUID PRIMARY KEY,
  device_name        TEXT NOT NULL,
  received_at        TIMESTAMPTZ NOT NULL,
  batch              JSONB NOT NULL,
  cumulative         JSONB NOT NULL,
  registered         INT NOT NULL DEFAULT 0,
  registered_visible INT NOT NULL DEFAULT 0
);
