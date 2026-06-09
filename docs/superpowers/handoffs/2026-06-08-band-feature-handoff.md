# 네이버 밴드(band.us) 기능 인수인계 (2026-06-08)

> **다른 환경/새 Claude 세션에서 이어서 작업하려면 이 문서를 읽히세요.**
> 핵심 요약 → 무엇이 됐고 / 안 됐고 / 다음 할 일 순서.

## 0. 한 줄 요약

네이버 밴드(band.us)에 **로그인(CDP) + 가입·글쓰기·댓글(순수 HTTP, md 서명 역공학) + 멀티 밴드 게시 UI**를 구현했다. 실기기 테스트에서 **로그인·가입·글·댓글 전부 성공(HTTP 200)** 확인. 사수 방침대로 **모든 밴드 통신 로직은 Rust(백엔드), 프론트는 트리거만**.

## 1. 작업 위치 / PR

- 코드(WSL): `/home/csw/projects/pstmacro` · 정식 브랜치 `master`
- 실행/테스트(Windows): `C:\Users\user\pstmacro2` 에서 `pnpm tauri dev`
- GitHub: `beyondsoft-kr/pstmacro` (gh 계정 `vscodereader`). 사수=**pallas-dev**, 동기=현준 최/CMU02.
- **열린 PR 2개 (둘 다 OPEN, 머지 안 함 — 사수 승인 대기):**
  - **PR #140** — `feat/131` — 밴드 **로그인**(CDP). base=master.
  - **PR #151** — `feat/150` — 밴드 **가입·글쓰기·댓글 + 멀티 게시**(순수 HTTP). base=**feat/131**(스택). #140 머지 후 base를 master로.
- **feat/150이 최신**(feat/131 + 최신 master 포함). 테스트는 `feat/150`으로.

## 2. 아키텍처 (사수 방침: 로그인=CDP, 게시=순수 HTTP)

```
밴드 로그인 (CDP, 보이는 Chrome)         밴드 게시 (순수 HTTP, reqwest)
  band_auth/  ← 네이버 auth/ 미러          band_post/  ← 네이버 naver_cafe 패턴 미러
  - 이메일→비번 2단계 CDP 타이핑           - md 서명, getKey, join/post/comment
  - 쿠키를 cookies-band/{loginId}.json     - 그 쿠키로 api-kr.band.us 직접 POST
```

- 프론트(`src/`)엔 밴드 프로토콜 로직 **0줄**. `ipc.band.{login,queueStatus,publish,resolveName}` 호출만.
- 백엔드 밴드 코드: `src-tauri/src/band_auth/`(로그인) + `src-tauri/src/band_post/`(게시).

## 3. 핵심 역공학 결과 (패킷 캡처로 검증)

캡처 원본: `/home/csw/packet_copy/` (`.pcapng` + `keylogfile.txt`, tshark로 TLS 복호화).

### (a) 로그인 성공 쿠키 = `band_session` (BUC 아님!)

- band 로그인 성공 시 `/email_login/password` 302 응답이 **`band_session`(domain `.band.us`)** + `secretKey`(Path=/s/login/getKey, HttpOnly) 등을 발급.
- ⚠️ `BUC`는 **네이버 쿠키**(domain `.naver.com`) — band은 발급 안 함. 초기 구현이 BUC로 성공 판정해 실패했던 버그 → `band_session`으로 정정(`d431037`).
- 성공 흐름: 비번 POST → 302 → `/b/validation_welcome`(정상 환영 인터스티셜) → 자동으로 `www.band.us` 도달.

### (b) `md` 서명 (api-kr.band.us 모든 요청 필수) — 완전 해독+검증

```
md = base64_standard( HMAC-SHA256( key = utf8(secretKey), msg = path ) )
  path = "/v2.1.0/join_band?ts=<ms>"   (호스트 제거, ts 쿼리 포함, 원문 그대로)
  secretKey = auth.band.us/s/login/getKey JSONP 응답값(세션 동적, 로테이션)
  akey = bbc59b0b5f7a1c6efe950f6236ccda35  (고정 헤더값, HMAC 키 아님)
```

- 캡처 실측 md 3개(join/post/comment) Rust에서 바이트 일치 검증(`signature.rs` 테스트).

### (c) getKey 2단계 + `secretKey` 쿠키

- getKey는 **`secretKey` 쿠키**가 있어야 진짜 키를 줌. 없으면 `bandWebAuthInfo='temp'` 부트스트랩만 → 서명 실패.
- 그 `secretKey` 쿠키는 Path=/s/login/getKey라 `Network.getCookies(urls)`로는 누락됨 → **`Network.getAllCookies`** 로 수집해야 저장됨(`6a8394d`). **이 수정 후엔 반드시 재로그인**해야 쿠키가 새로 저장됨.

### (d) 엔드포인트/바디 (캡처 실측)

- 가입: `POST /v2.1.0/join_band` · `join_type=band_no&join_value=<no>&profile_id=1`
- 글쓰기: `POST /v2.0.2/create_post` · `band_no=&content=&...&purpose=create` → `result_data.post.{post_no,web_url,band.name}`
- 댓글: `POST /v2.3.0/create_comment` · `band_no=&member_type=membership&body=&content_key={"content_type":"post","post_no":N}&...`
- 밴드명 조회: `GET /v2.2.0/get_band_information?ts=&band_no=` → `result_data.name`
- 공통헤더: `akey`, `md`, `content-type: x-www-form-urlencoded`, `device-time-zone-id: Asia/Seoul`, 쿠키

## 4. UI (게시 모달 밴드 섹션) — 현재 흐름

`src/features/posts/publish-modal.tsx` (DestinationPicker 밴드 섹션):

1. **가입할 밴드 링크** 입력 + **저장** → `ipc.band.resolveName`으로 실제 밴드명 조회, `resolvedBands[]`에 누적(band_no 중복 제거).
2. **사수의 드롭다운(`Select`)** — 누적된 실제 밴드명 목록. 선택 시 `selectedBands[]`(bandNo)에 추가.
3. **선택한 밴드 칩**(x로 제거).
4. **게시** → **선택한 각 밴드 계정 × 선택한 각 밴드** 마다 `ipc.band.publish`. 결과는 밴드명별 행.

- 게시 버튼은 밴드 1개 이상 선택해야 활성(예약은 밴드 제외 — 엔진 미연결).
- 계정관리 "선택 로그인": 플랫폼=밴드 계정은 `ipc.band.login`(band.us)로 분기(`48e8512`). 종토방/카페는 기존 네이버 로그인 그대로.

## 5. 검증 상태

- 테스트: Rust `band_post` 63 + `band_auth` 35 + 프론트 vitest 251 통과. clippy/tsc/eslint 클린.
- 실기기(Windows): 로그인 ✅ / getKey ✅ / join_band 200 ✅ / create_post 200 ✅ / create_comment 200 ✅.
- 로그인은 **랜선만 + ID/PW**로 됨(useAdb=false, ADB 불필요). 첫 콜드 스타트엔 **구글 reCAPTCHA(보안문자)** 가 뜰 수 있음(헤디드라 수동 1회 풀면 됨, wait_for_human=true).

## 6. 남은 일 / 알려진 한계 (다음 작업)

1. **밴드명 저장 시 번호로 폴백되는 케이스** — `get_band_information`이 이름을 안 줄 때 band_no로 폴백. `result_data.band.name` 중첩 폴백 + 진단 로그 추가함(`64f0403`). **윈도우에서 저장 후 `[BAND] get_band_information ...` 로그를 확보해 실제 응답 구조 확정 필요.** (하드코딩 아님 — 동적 조회. "데일밴드"는 테스트 목/주석에만 존재.)
2. **가입 불가/승인제 밴드 예외처리** — 현재: `join_band` best-effort(실패해도 게시 시도) → 비멤버면 `create_post` 실패 → 그 밴드 결과행만 빨강 실패(다른 밴드는 독립 진행). "승인 대기" 구분/안내는 없음. **그런 밴드로 캡처 떠서 join_band의 result_code 확인 후 분기 추가 권장.**
3. **카페 로그 누락** — `naver_cafe`는 tracing 로그를 안 씀(에러를 ErrorEnvelope로 프론트에만 반환). 일원화 완성하려면 `[CAFE]` 로그 추가.
4. **밴드 게시 로그가 HTTP status만** 찍음 — result_code도 넣어야 논리 실패(200+거절) 구분됨.
5. **초대 전용 단축링크(`band.us/n/...`)** 미지원 — band_no URL만(`/band/{숫자}`).

## 7. 로그 규칙 (pstmacro.log 일원화)

- 파일: `%APPDATA%\...\logs\pstmacro.log`(일자별), 콘솔도 동시. 시각 `YYYY-MM-DD HH:MM:SS`, 레벨 `PSTMACRO_LOG`(기본 info).
- 프리픽스: `[LOGIN]/[CHROME]/[ADB]`(로그인), `[POST]`(종토방), `[BAND]`(밴드 로그인+게시). **카페는 없음**(위 6-3).
- 예: `2026-06-08 16:23:04  INFO [BAND] POST /v2.0.2/create_post → status=200`

## 8. 재개 방법

```powershell
# Windows 테스트
cd C:\Users\user\pstmacro2
git fetch --all --prune
git checkout feat/150        # 또는 git checkout -f feat/150 (줄바꿈 충돌 시)
git pull origin feat/150
pnpm install
pnpm tauri dev
```

```bash
# WSL 코드 작업
cd /home/csw/projects/pstmacro
git checkout feat/150 && git pull
cd src-tauri && cargo test --lib band_   # 밴드 테스트
cd .. && pnpm exec vitest run            # 프론트 테스트
```

- 사수 승인 후 머지 순서: **#140(feat/131) → master**, 그 다음 **#151(feat/150)** base를 master로 바꿔 머지.
- TaskCreate 쓰지 말 것(훅이 GitHub 이슈 도배). 이슈는 `gh issue create`로 1개씩.

## 9. 관련 문서 / 메모리

- 설계: `docs/superpowers/specs/2026-06-08-band-multi-publish-design.md`
- 계획: `docs/superpowers/plans/2026-06-08-band-multi-publish.md`
- Claude 자동 메모리(WSL): `~/.claude/projects/-home-csw-projects-pstmacro/memory/` 의 `band-login-flow-spec`, `band-md-signature-decoded` 등.
- 이슈 #150(밴드 가입·글쓰기·댓글), #131(밴드 로그인).

## 10. 이 세션 커밋(feat/150, master 이후, 최신순)

```
64f0403 fix(band-post): 밴드명 조회 중첩 폴백 + 진단 로그
c0a3de9 feat(band-post): 멀티 밴드 게시 — 사수 드롭다운 복원 + 다중선택 칩
6a8394d fix(band-auth): 로그인 쿠키 수집 getAllCookies — secretKey 누락 해결
9e97c05 feat(band-post): 링크 저장 시 실제 밴드명 조회·표시
b58c164 feat(band-post): 게시 결과에 실제 밴드명 표시
d431037 fix(band-auth): 성공 판정 쿠키 BUC→band_session 정정
48e8512 feat(accounts): 밴드 플랫폼 계정 선택로그인은 band.us로
1583fe3 feat(band-post): md 서명(HMAC-SHA256) + 캡처 검증
cb2ae93 feat(band-auth): band.us CDP 이메일 로그인 백엔드 + IPC
(그 외 빌더/getKey/IPC/클라이언트/문서 커밋 — git log origin/master..feat/150)
```
