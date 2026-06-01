# 2. Rust reqwest 기반 네이버 HTTP 로그인

Date: 2026-05-27

## Status

Accepted (패킷 분석 결과 반영하여 업데이트 — 2026-05-27)

## Context

네이버 자동 로그인 기능이 필요하다. 초기 설계 시 RSA 암호화를 가정했으나,
실제 패킷 캡처 분석 결과 네이버는 **ECC (Elliptic Curve Cryptography)** 기반의
동적 공개키 암호화를 사용하고 있음이 확인됐다.
기존에 Playwright(Node.js + Chrome) sidecar 방식을 검토했으나 배포 복잡도가 높았다.

## Decision

Playwright sidecar를 제거하고, Rust `reqwest`로 HTTP 요청을 직접 재현하는 방식을 채택한다.

### 실제 로그인 흐름 (패킷 분석 기반, 5단계)

1. **로그인 폼 GET** — `GET https://nid.naver.com/nidlogin.login?mode=form&url=https://www.naver.com/`
   - 초기 쿠키 수집 (NAC, NNB, nid_buk 등)
   - 응답 HTML에서 CSRF 토큰 (`wtoken`) 파싱

2. **EC 공개키 GET** — `GET https://nid.naver.com/login/dynamicEcKey/{key_id}`
   - 세션마다 새로운 EC 공개키 발급 (재전송 공격 방지)
   - 응답: Base64 인코딩된 EC 공개키

3. **CAPTCHA 토큰 POST** — `POST https://ncpt.naver.com/v2/tokens?q={timestamp_ms}&tid={tid}`
   - 비정상 접근 감지용 토큰 발급

4. **자격증명 POST** — `POST https://nid.naver.com/nidlogin.login`
   - `Content-Type: application/x-www-form-urlencoded`
   - 아이디·비밀번호는 EC 공개키로 암호화 후 `eccpw` 필드에 전송
   - 주요 필드: `dynamicKey`, `eccpw`, `enctp=1`, `wtoken`, `svctype=1`,
     `template_type=V2_DESKTOP_DEFAULT`, `smart_LEVEL=1`, `locale=ko_KR`,
     `url=https://www.naver.com/`, `id=""`, `pw=""`

5. **세션 확정 GET** — `GET https://nid.naver.com/signin/v3/finalize?url=...&svctype=1`
   - 세션 최종 확정, `NID_AUT`·`NID_SES` 쿠키 발급

쿠키 세션은 `reqwest::Client::builder().cookie_store(true)`로 자동 관리한다.

구현 모듈은 `src-tauri/src/auth.rs`에 위치하며, Tauri command로 래핑해 프론트엔드에 노출한다.

### 의존성 (`src-tauri/Cargo.toml`)

```toml
reqwest = { version = "0.12", default-features = false, features = ["rustls-tls", "cookies", "json"] }
# ECC 크레이트는 곡선 종류 확정 후 결정 (p256 또는 k256 등 pure-Rust 후보)
rand = "0.8"
base64 = "0.22"
tokio = { version = "1", features = ["full"] }
```

## Alternatives Considered

- **`openssl` crate**: 시스템 OpenSSL에 의존해 크로스 컴파일 복잡도가 높다. pure-Rust ECC 크레이트를 선택.
- **RSA (초기 가정)**: 패킷 분석 전 PKCS#1 v1.5를 가정했으나, 실제 네이버 API는 ECC 기반임이 확인됨.
- **동기 reqwest**: Tauri 2는 비동기 런타임 기반이므로 async 버전을 사용.

## Consequences

쉬워지는 것:

- 배포 시 Node.js 런타임·시스템 Chrome 설치 불필요 — 단일 바이너리로 배포 가능
- Rust 타입 시스템과 `?` 연산자로 에러 전파가 명시적

어려워지는 것:

- 네이버가 로그인 API 구조(ECC 곡선 종류, 암호화 포맷, 필드 구성)를 변경하면
  리버스 엔지니어링 후 코드를 수정해야 한다
- CAPTCHA 발생 시 현재 버전에서는 에러로 처리하므로, 자동 해결은 별도 작업이 필요하다

## 미확정 사항 (구현 전 확인 필요)

- **ECC 곡선 종류**: P-256? secp256k1? — 네이버 로그인 JS 소스 또는 추가 패킷 분석으로 확정
- **`eccpw` 암호화 포맷**: ECIES? ECDH 후 AES? — JS 리버스 필요
- **`dynamicKey` (key_id) 취득 방법**: 로그인 폼 HTML 파싱? 별도 엔드포인트?
- **`bvsd` 필드**: 브라우저 지문 데이터, 빈 값으로 통과 가능한지 확인 필요
- **CAPTCHA `tid`**: 세션 식별자 생성 방법
