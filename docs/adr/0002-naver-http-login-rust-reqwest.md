# 2. Rust reqwest 기반 네이버 HTTP 로그인

Date: 2026-05-27

## Status

Accepted

## Context

네이버 자동 로그인 기능이 필요하다. 네이버 로그인은 단순 form POST가 아니라 RSA 공개키로
비밀번호를 암호화한 뒤 URL-safe Base64로 인코딩해 전송하며, 서버는 JSONP로 프로파일
정보를 반환한다. 기존에 Playwright(Node.js + Chrome) sidecar 방식을 검토했으나
배포 복잡도가 높았다.

## Decision

Playwright sidecar를 제거하고, Rust `reqwest`로 HTTP 요청을 직접 재현하는 방식을 채택한다.

로그인 흐름:

1. `pub_key_url` GET → HTML 응답에서 RSA 공개키 추출
2. `rsa` 크레이트로 비밀번호 RSA-PKCS1v15 암호화 → URL-safe Base64 인코딩
3. `data={encoded}` 형태로 `login_url` form POST (쿠키는 `reqwest::Client`가 자동 관리)
4. `profile_url` JSONP GET → 콜백 래퍼 제거 후 JSON 파싱으로 로그인 성공 여부 확인

추가 의존성 (`src-tauri/Cargo.toml`):

```toml
reqwest = { version = "0.12", default-features = false, features = ["rustls-tls", "cookies", "json"] }
rsa = { version = "0.9", features = ["pkcs1", "pkcs8"] }
rand = "0.8"
base64 = "0.22"
tokio = { version = "1", features = ["full"] }
```

쿠키 세션은 `reqwest::Client::builder().cookie_store(true)`로 자동 관리한다.

구현 모듈은 `src-tauri/src/auth.rs`에 위치하며, Tauri command로 래핑해 프론트엔드에 노출한다.

## Alternatives Considered

- **`openssl` crate**: 시스템 OpenSSL에 의존해 크로스 컴파일 복잡도가 높다. pure-Rust `rsa` 크레이트를 선택.
- **OAEP 패딩**: 암호화 출력 예시(`-`, `_` 포함 URL-safe Base64, `==` 패딩) 분석 결과 PKCS#1 v1.5로 추정. 서버 검증 후 변경 가능하도록 `encrypt` 함수를 분리해 설계.
- **동기 reqwest**: Tauri 2는 비동기 런타임 기반이므로 async 버전을 사용.

## Consequences

쉬워지는 것:

- 배포 시 Node.js 런타임·시스템 Chrome 설치 불필요 — 단일 바이너리로 배포 가능
- Rust 타입 시스템과 `?` 연산자로 에러 전파가 명시적

어려워지는 것:

- 네이버가 로그인 API 구조(공개키 추출 위치, 평문 형식, RSA 패딩)를 변경하면
  리버스 엔지니어링 후 코드를 수정해야 한다
- CAPTCHA 발생 시 현재 버전에서는 에러로 처리하므로, 자동 해결은 별도 작업이 필요하다

후속 작업:

- 실제 패킷 캡처로 공개키 추출 위치·평문 형식·RSA 패딩 방식 확정 후 `extract_public_key` 완성
