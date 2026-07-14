# pstmacro-server — Admin–하위 원격제어 중앙 서버

설계 `docs/설계.md` §9·§10 구현. Tauri 앱(`src-tauri`)과 **독립된 별도 Rust 크레이트**(워크스페이스로 묶지 않아 기존 빌드 무손상).

## 무엇인가

Admin 웹(`src/admin`, 브라우저)이 호출하는 HTTP API + SSE 허브. 운영자 인증, 기기(하위) 등록·모니터링, 계정 스테이징·분배(MOVE), 감사로그(통신로그)를 담당한다.

- **인증**: 운영자 로그인 = JWT(만료 8시간 + 토큰버전 회수), 기기 토큰 = JWT(만료 없음, 줄삭제 회수). 비번 해시 = argon2.
- **DB = PostgreSQL**(사수 확정). 계정 ID/PW는 AES-256-GCM으로 at-rest 암호화 저장(§7).
- **SuperAdmin 부트스트랩**: 최초 기동 시 `Superadmin`/`Superadmin` 시드 → 첫 로그인 시 비번 변경 강제.

## 실행

```bash
cd server
# (개발/오프라인) DATABASE_URL 없이 → in-memory 저장소. 데모·미리보기용.
cargo run
# (운영) PostgreSQL 사용:
DATABASE_URL=postgres://user:pass@localhost/pstmacro \
PSTMACRO_JWT_SECRET=... \
PSTMACRO_ENC_KEY=<32바이트> \
cargo run
```

기본 바인드 `0.0.0.0:8080`. Admin 웹은 `VITE_ADMIN_API`로 이 주소를 가리킨다(기본 `http://localhost:8080`).

### 빠른 DB 연결(로컬 PostgreSQL, 턴키)

`DATABASE_URL`이 설정되면 서버가 **연결 즉시 스키마(테이블 7개)를 자동 생성**하고 SuperAdmin을
시드한다. 로컬은 옆의 `docker-compose.yml`로 PG를 한 번에 띄운다:

```bash
cd server
cp .env.example .env                 # 값(시크릿) 채우기
docker compose up -d                 # PostgreSQL 기동(pstmacro/pstmacro/pstmacro)
# 운영 모드는 JWT/ENC 시크릿이 기본값이면 기동 거부 → 함께 주입:
export DATABASE_URL=postgres://pstmacro:pstmacro@localhost:5432/pstmacro
export PSTMACRO_JWT_SECRET=$(openssl rand -hex 32)
export PSTMACRO_ENC_KEY=$(openssl rand -hex 32)
cargo run
```

로그에 `PostgreSQL 연결됨`이 뜨면 연결 성공(미설정이면 `DATABASE_URL 미설정 → in-memory …`
경고와 함께 개발 폴백). 운영은 이 compose가 아니라 **실제 PG의 접속 문자열**을 `DATABASE_URL`로 준다.

## 환경변수

| 변수                    | 용도                                  | 비고                        |
| ----------------------- | ------------------------------------- | --------------------------- |
| `DATABASE_URL`          | PostgreSQL 접속(없으면 in-memory)     | 운영 필수                   |
| `PSTMACRO_JWT_SECRET`   | JWT 서명 비밀                         | 운영 필수(강한 값)          |
| `PSTMACRO_ENC_KEY`      | 계정 암호화 키(32바이트, AES-256-GCM) | 운영 필수, DB와 분리(§13)   |
| `PSTMACRO_BIND`         | 바인드 주소:포트                      | **배포 전 결정**            |
| `PSTMACRO_PUBLIC_URL`   | 하위에 보여줄 서버 공인 주소(§6-1)    | **배포 전 결정**            |
| `PSTMACRO_TLS_CERT/KEY` | TLS 인증서/키                         | **배포 전 결정**(앞단 권장) |

> ⚠️ 주소·TLS·포트는 사수 지시로 **배포 전 결정**(`src/config.rs`에 자리만, 값 비움).

## 스키마

`migrations/0001_init.sql`(PostgreSQL). 서버 기동 시 `PostgresRepo`가 동일 스키마를 멱등 생성하므로 자동 적용된다.

## 검증

```bash
cargo test        # crypto(AES·argon2)·JWT·분배 단위 테스트
cargo build       # 컴파일
```

## 아직 안 된 것(다음 단계)

- **하위 에이전트**(`src-tauri/src/agent/`, P5): SSE 구독·하트비트·재연결·결과 보고. 서버의 `/device/register`·`/agent/*` 엔드포인트는 준비됨 — 에이전트가 붙으면 실시간 동작.
- 결과 보고 화면의 **게시 결과**는 에이전트 보고 데이터라 현재 더미.
