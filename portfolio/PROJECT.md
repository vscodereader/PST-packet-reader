# PSTMACRO — 사용 기술과 실제 적용 위치

2026-09-10 저장소 및 전체 브랜치 조사 기준으로 작성함. 프로젝트 전체 기술과 본인 작성 PR은 구분하며, 상세 기여 근거는 PR별 기록과 커밋 이력에 연결함.

## 1. 프로젝트 개요와 구현 구조

여러 Windows PC에서 네이버·밴드 계정의 로그인, 게시, 댓글, 작업 예약을 실행하고 중앙 Admin에서 기기·계정·명령·결과를 관리하는 시스템을 개발함. 브라우저 정상 동작의 패킷을 관찰하여 HTTP 요청 계약을 파악하고, 브라우저 조작이 필요한 구간은 CDP로 처리함.

화면은 `src/`, Windows 실행 계층은 `src-tauri/`, 중앙 서버는 `server/`로 분리함. Admin은 `src/admin/`에서 별도 화면으로 구성함. 원본 기본 브랜치는 `master`이며 실험·미병합 브랜치의 내용을 기본 브랜치에 병합된 것으로 서술하지 않음.

## 2. 화면·데스크톱 기술

### React 19·TypeScript

- 적용 위치: `src/features/`, `src/admin/features/`, `src/admin/api.ts`.
- 계정 목록, 로그인 상태, 종목 선택, 게시 설정, 큐, 기기별 통신 로그와 결과 보고 화면에 사용함.
- 화면 입력을 타입으로 정의하고 API·IPC 응답을 화면 상태로 변환함. 자동 종목 선택과 수동 선택은 공통 게시 입력 계약을 유지하면서 입력을 만드는 단계에서 구분함.
- Admin의 종목 선택 모달은 데스크톱과 공통 화면을 재사용하고 데이터 공급 어댑터만 분리하여 같은 선택 규칙이 다르게 구현되지 않도록 함.

### Mantine·Vite

- Mantine은 공용 입력·모달·알림·레이아웃 구성에 사용함. Vite는 React 개발 서버와 배포 번들 생성에 사용함.
- 적용 근거: 루트 `package.json`, `vite.config.ts`, `src/admin/main.tsx`.
- 실제 채널 통신은 UI 컴포넌트에 넣지 않고 Rust 도메인 또는 서버 API로 위임함.

### Tauri 2·Rust·IPC

- 적용 위치: `src-tauri/src/lib.rs`, `src-tauri/src/ipc/`, `src-tauri/Cargo.toml`.
- React의 명령을 Rust 함수로 전달하여 계정 저장, 브라우저 시작, 게시 실행, Excel 처리와 작업 큐를 구동함.
- Tauri 플러그인은 파일 선택, 자동 시작, 단일 인스턴스, 시스템 알림과 트레이 상주를 담당함.
- `serde`·`serde_json`으로 명령 데이터를 직렬화하고 `ts-rs`와 `gen:bindings`로 Rust 타입을 TypeScript에 전달하는 구성을 둠.

## 3. 패킷 분석·HTTP·브라우저 제어

### Wireshark·TShark·TLS key log

- 사용자가 직접 Chrome 동작을 캡처하고 `D:\packet_copy`의 시나리오별 pcapng와 TLS key log로 HTTP/2 내용을 확인한 개발 방식임.
- Wireshark의 `http2` 필터로 시작하여 요청 HEADERS, 메서드, 도메인, 경로를 확인하고 같은 TCP 연결·HTTP/2 stream의 요청과 응답을 묶음.
- TShark는 GUI에서 찾은 요청의 프레임 번호·stream·헤더·DATA를 반복 추출하고 정상·실패 흐름을 대조하는 데 사용함.
- TLS 복호화 결과에서 JSON·form-urlencoded 필드와 리다이렉트를 확인함. TLS 전송 암호화와 서버의 계정 저장 암호화는 서로 다른 계층임.
- 캡처와 키 파일은 실행 코드 의존성이 아니며 Git 이력 복사만으로 별도 디스크의 캡처가 자동 포함되는 것은 아님.

### reqwest·rustls·응답 압축 처리

- 적용 위치: `naver_automation/packet_client.rs`, `naver_blog/`, `naver_cafe/`, `band_post/client.rs`.
- 관측한 API의 URL, 헤더, 쿠키, JSON·폼 body를 Rust HTTP 클라이언트로 구성함.
- Cargo에서 gzip·brotli·deflate·zstd·multipart 기능을 켜 압축 응답과 파일 업로드를 처리함. Accept-Encoding 문자열만 흉내 내지 않고 실제 해제 기능도 갖추도록 함.
- 상태 코드만으로 성공을 판정하지 않고 오류 body, 인증 상태, 리다이렉트와 최종 결과를 함께 확인하는 진단 흐름에 사용함.

### Chrome DevTools Protocol·WebSocket

- 적용 위치: `auth/chrome.rs`, `auth/login_flow.rs`, `naver_automation/devtools_connection.rs`, `browser_flow.rs`.
- 브라우저 시작, 페이지·DOM 상태 확인, 입력·클릭, 브라우저 쿠키와 네트워크 상태 확인에 사용함.
- 네이버 로그인 폼 변경에는 셀렉터와 준비 조건을 수정하고, 게시·신고에서 브라우저 제출이 필요한 경로는 해당 흐름으로 처리함.
- `tungstenite`는 CDP WebSocket 통신에 사용함. 원격 Admin 명령 전달에 사용하는 SSE와 역할이 다름.
- WASM Fetch 차단은 `feat/wasm-fetch-block-experiment`에 존재하는 실험으로 기록하며 일반적인 인증 해결책 또는 운영 반영 완료로 표현하지 않음.

### ADB

- 적용 위치: `src-tauri/src/auth/adb.rs`.
- Android 기기 연결과 네트워크 전환을 호출하는 CLI 연동에 사용함. 로그인 작업과 별도로 실행 가능한 기기 작업을 구성하고 명령 결과를 확인함.

### 밴드 HMAC 서명·네이버 에디터 문서 변환

- 밴드: `band_post/signature.rs`에서 HMAC-SHA256과 Base64를 사용하여 요청 서명 형식을 처리함. 로그인 쿠키 관리와 게시 요청 생성은 별도 모듈로 분리함.
- 블로그: `naver_blog/document_model.rs`, `editor_api.rs`, `write_client.rs`에서 에디터 문서 구조, 미디어와 게시 요청을 처리함.
- 카페: `naver_cafe/article_list/`, `cafe_ref/`, `comment/`에서 카페·게시판 식별과 글 목록·댓글 요청을 구분함. 최신 글을 카페 전체가 아닌 지정 게시판으로 좁히는 수정 이력이 있음.

## 4. 중앙 서버·데이터·인증

### Axum·Tokio·SSE

- 적용 위치: `server/src/main.rs`, `routes.rs`, `hub.rs`, `scheduled.rs`, `src-tauri/src/agent/net.rs`.
- Axum으로 운영자·기기 API를 제공하고 Tokio로 비동기 요청, 예약과 이벤트 전달을 처리함.
- `hub.rs`는 device_id별 SSE 송신 채널과 Admin 브로드캐스트를 유지함. 연결이 끊긴 기기의 명령은 제한된 대기 큐에 보관하고 재구독 시 전달함.
- 코드의 `MAX_PENDING`은 128이며, 재연결하지 않는 기기의 대기열이 계속 증가하는 것을 제한하는 기술 상한임.
- 단순 전송 성공과 실제 실행 완료를 구분하기 위해 하위의 결과 보고를 Admin 화면에 연결함.

### PostgreSQL·SQLx·repository trait

- 적용 위치: `server/src/repo/postgres.rs`, `repo/memory.rs`, `repo/mod.rs`.
- 운영자·기기·계정·작업과 결과를 PostgreSQL에 보관하고 SQLx로 접근함.
- 동일한 repository 계약의 메모리 구현을 테스트에 사용하여 실제 DB 없이 분배·조회 정책을 검증함.
- 기기 식별은 재설치마다 새 ID를 생성하는 대신 안정적인 machine 식별값으로 upsert하는 수정 이력이 있음.
- 삭제 기기의 과거 결과는 저장 이력을 지우는 방식이 아니라 현재 등록 기기를 기준으로 조회를 제한하는 방식으로 보완함.

### Argon2id·AES-256-GCM·JWT

- 적용 위치: `server/src/crypto.rs`, `jwt.rs`.
- 운영자 비밀번호는 Argon2id와 salt로 단방향 해시하여 로그인 검증에 사용함.
- 하위에 분배할 계정 ID/PW는 복원이 필요하므로 AES-256-GCM으로 저장 암호화함. 코드에서 32바이트 키와 매번 생성한 12바이트 nonce를 사용하고 결과를 Base64로 저장함.
- JWT는 인증된 요청의 자격 확인에 사용함. 비밀번호 해시·저장 암호화·TLS를 동일한 기능으로 설명하지 않음.
- 코드의 암호화 왕복·다른 키 거부·nonce 변경 테스트가 존재함. 이번 문서화 작업에서 운영 계정 로그인이나 원격 작업을 새로 실행한 것은 아님.

## 5. 데이터 입출력·품질·배포

- Excel: `calamine`으로 입력을 읽고 `rust_xlsxwriter`로 출력함. 적용 진입점은 `src-tauri/src/ipc/excel.rs`임.
- 로그: `tracing`, `tracing-subscriber`, `tracing-appender`와 `src-tauri/src/logging.rs`로 진단 정보를 기록함. 화면의 통신 로그 필터와 런타임 로그는 각각 사용 목적에 맞게 연결함.
- 테스트: Vitest·Testing Library는 화면과 TypeScript 로직, Cargo test는 Rust 도메인, wiremock은 외부 HTTP 계약 검증에 사용함.
- 정적 검사: TypeScript, ESLint, Prettier, Stylelint, rustfmt·Clippy를 사용하며 Husky·lint-staged·commitlint로 커밋 전 검사를 구성함.
- 배포: Docker·Cloud Run 관련 설정과 GitHub Actions가 존재함. Windows 크로스 빌드와 PDB 배포 설정은 릴리스에서도 오류 위치를 추적하기 위한 목적임.

## 6. 개인 기여와 검증 자료

본인 기여는 `vscodereader` 작성 PR 및 Git author 이력에서 확인함. 팀 전체 구조를 본인이 단독 작성한 것으로 치환하지 않음. 전체 PR 목록, 본인 PR 본문·변경 파일·리뷰·커밋, 원격·로컬 브랜치와 미커밋 스냅샷을 개인 저장소 문서에서 각각 조회하도록 구성함. 기존 Notion 패킷 분석 설명은 그대로 두고 본 기술별 적용 설명을 보완함.
