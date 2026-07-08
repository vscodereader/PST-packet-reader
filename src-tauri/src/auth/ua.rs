//! 로그인 브라우저의 User-Agent 를 판마다 최신 실존 크롬 버전 중 하나로 로테이션한다.
//!
//! 배경(2026-07-08 실측·WASM 분석): 봇탐지(wtm/ncaptcha) WASM 엔진이 fpHash(기기지문 해시)를
//! 만들 때 UA 도 입력으로 들어간다. 같은 PC 로 로그인하면 매판 UA·지문이 동일해 서버가 "한
//! 기기"로 묶어 로그인 시도를 누적(≈19판 통과→20판째 캡차)하는 것으로 관측됐다. UA 를 판마다
//! 다르게 흘려 그 묶음이 흩어지는지(=캡차 벽이 뒤로 밀리는지) 실험한다.
//!
//! ⚠️ UA 문자열만 바꾸면 sec-ch-ua(Client Hints)·서비스워커/iframe UA 와 어긋나
//! `_setHasLiedBrowser`/`NCAPTCHA_UA_DETECTION_RESULT`(WASM 교차검증)에 걸려 오히려 봇점수가
//! 오른다. 그래서 **일관되게** 바꾼다:
//!   (1) `--user-agent` 실행 플래그 → 브라우저 전역 UA 문자열 통일(페이지·iframe·서비스워커·헤더),
//!   (2) `Emulation.setUserAgentOverride` + `userAgentMetadata` → 페이지 Client Hints 를 같은
//!       버전으로 맞춤(이 오버라이드가 UA·Client Hints 를 둘 다 지배 → 내부 불일치 없음).
//!
//! 값의 규칙(사수 지시 "최신 UA만" + "설치버전 이하로만"):
//!   - 후보 버전은 **실존값만** 쓴다(지어낸 빌드 금지 — 없는 크롬으로 잡힘). 최신 실존 목록은
//!     구글 공식 **버전 히스토리 API**(무인증)에서 프로세스당 1회 받아 캐시하고, 네트워크 실패 시
//!     하드코딩 실측 폴백을 쓴다.
//!   - 실제 설치된 크롬 **메이저 이하**만 후보로 쓴다(상위 버전 주장은 기능탐지로 들통). 설치된
//!     실제 버전 자체는 항상 안전한 후보로 포함한다.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;
use std::time::Duration;

use serde_json::{json, Value};

/// 네트워크 실패 시 폴백 = **실측 실존** 크롬 풀버전(2026-07-07/08 패킷 캡처에서 확인). 오름차순.
const FALLBACK_VERSIONS: &[&str] = &["149.0.7827.201", "150.0.7871.47"];

/// 구글 공식 버전 히스토리 API(공개·무인증). win64 스테이블 크롬 전 버전을 **최신순**으로 준다.
const VERSION_HISTORY_URL: &str =
    "https://versionhistory.googleapis.com/v1/chrome/platforms/win64/channels/stable/versions";

/// 로테이션 후보로 쓸 **최근 메이저 버전 개수**. 각 메이저의 최신 실존 빌드 1개씩 뽑는다
/// (예: 150·149·148·147·146·145 각각의 최신 빌드). "최신 24 빌드"로 뽑으면 죄다 150.x/149.x
/// 라 메이저(=UA 문자열)가 안 갈리므로, 메이저 단위로 골고루 되게 한다.
const MAX_MAJORS: usize = 6;

/// 선택된 UA 한 벌. `user_agent`(문자열)와 Client Hints 구성에 필요한 버전 정보를 담는다.
#[derive(Debug, Clone)]
pub(crate) struct UaProfile {
    pub(crate) user_agent: String,
    pub(crate) major: String,
    pub(crate) full_version: String,
}

/// 축소(reduced) UA 문자열. 실제 크롬처럼 마이너/빌드는 `.0.0.0` 으로 고정하고 메이저만 넣는다
/// (상세 버전은 Client Hints 로 간다). Windows x64 데스크톱 고정.
fn ua_string(major: &str) -> String {
    format!(
        "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 \
         (KHTML, like Gecko) Chrome/{major}.0.0.0 Safari/537.36"
    )
}

fn major_of(full_version: &str) -> &str {
    full_version.split('.').next().unwrap_or(full_version)
}

fn major_num(full_version: &str) -> Option<u32> {
    major_of(full_version).parse().ok()
}

fn build_profile(full_version: &str) -> UaProfile {
    let major = major_of(full_version);
    UaProfile {
        user_agent: ua_string(major),
        major: major.to_owned(),
        full_version: full_version.to_owned(),
    }
}

fn fallback_pool() -> Vec<String> {
    FALLBACK_VERSIONS.iter().map(|v| (*v).to_owned()).collect()
}

/// 프로세스당 1회만 버전 풀을 확정한다(OnceLock). 성공=API 최신 실존목록, 실패=폴백 실측값.
/// 로그인 경로(launch_for_login)는 항상 blocking 스레드에서 도므로 여기서 reqwest::blocking
/// 호출은 안전하다(게시/밴드 경로는 UA 로테이션을 안 써 이 함수를 타지 않는다).
fn version_pool() -> &'static Vec<String> {
    static POOL: OnceLock<Vec<String>> = OnceLock::new();
    POOL.get_or_init(|| match fetch_version_history() {
        Some(v) if !v.is_empty() => {
            tracing::info!(count = v.len(), latest = %v[0], "[UA] 버전 히스토리 API 로드 성공");
            v
        }
        _ => {
            tracing::warn!("[UA] 버전 히스토리 API 실패/빈응답 — 실측 폴백 버전 사용");
            fallback_pool()
        }
    })
}

/// 버전 히스토리 API 에서 최신 실존 스테이블 크롬(win64) 풀버전 상위 N개를 가져온다(4초 타임아웃).
/// 어떤 실패(네트워크/파싱/빈값)든 None → 호출부가 폴백을 쓴다.
fn fetch_version_history() -> Option<Vec<String>> {
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(4))
        .build()
        .ok()?;
    let text = client.get(VERSION_HISTORY_URL).send().ok()?.text().ok()?;
    let json: Value = serde_json::from_str(&text).ok()?;
    let all: Vec<String> = json
        .get("versions")?
        .as_array()?
        .iter()
        .filter_map(|v| v.get("version").and_then(Value::as_str).map(ToOwned::to_owned))
        .filter(|v| major_num(v).is_some() && v.split('.').count() >= 3)
        .collect();
    let picked = dedup_latest_per_major(&all, MAX_MAJORS);
    (!picked.is_empty()).then_some(picked)
}

/// (순수) 최신순 버전 목록에서 **각 메이저의 최신(첫 등장) 빌드 1개씩**을, 최근 `max_majors`
/// 메이저까지 뽑는다. "최신 24 빌드"로 뽑으면 죄다 같은 메이저(150.x)라 UA 문자열이 안 갈리므로,
/// 메이저 단위로 골고루 되게 한다. 예: [150.b2,150.b1,149.b3,148.b2] → [150.b2,149.b3,148.b2].
fn dedup_latest_per_major(versions_newest_first: &[String], max_majors: usize) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for v in versions_newest_first {
        let Some(m) = major_num(v) else { continue };
        if seen.insert(m) {
            out.push(v.clone());
            if out.len() >= max_majors {
                break;
            }
        }
    }
    out
}

/// (순수) 후보 `pool` 에서 설치버전 이하만 골라 엔트로피로 하나 선택한다.
///
/// - `installed_full = None`(설치 버전 못 읽음): 상위 버전 주장 위험을 피해 **풀 최저값**(가장 낮은
///   실존 버전) 하나로 고정.
/// - 설치된 실제 버전 자체는 항상 안전한 후보(자기 버전 주장은 절대 안 어긋남).
pub(crate) fn pick_capped(pool: &[String], installed_full: Option<&str>, entropy: u64) -> UaProfile {
    let lowest = |p: &[String]| -> String {
        p.iter()
            .filter_map(|v| major_num(v).map(|m| (m, v.clone())))
            .min_by_key(|(m, _)| *m)
            .map(|(_, v)| v)
            .unwrap_or_else(|| FALLBACK_VERSIONS[0].to_owned())
    };

    let Some(cap_major) = installed_full.and_then(major_num) else {
        return build_profile(&lowest(pool));
    };

    let mut candidates: Vec<String> = pool
        .iter()
        .filter(|v| major_num(v).map(|m| m <= cap_major).unwrap_or(false))
        .cloned()
        .collect();

    if let Some(f) = installed_full {
        if !f.is_empty() && !candidates.iter().any(|c| c == f) {
            candidates.push(f.to_owned());
        }
    }
    if candidates.is_empty() {
        candidates.push(lowest(pool));
    }

    let idx = (entropy % candidates.len() as u64) as usize;
    build_profile(&candidates[idx])
}

/// 매 로그인 호출 — 캐시된 풀에서, 설치 버전 상한 안에서 **라운드로빈**으로 하나 고른다.
///
/// ⚠️ 엔트로피로 시각 나노초를 쓰면 Windows 에서 깨진다: Windows 시스템 시계는 100ns 단위라
/// `as_nanos()` 가 항상 100 의 배수가 되고, `% 5`(또는 100 의 약수) 가 **항상 0** → 매번 첫
/// 후보만 뽑혔다(2026-07-08 실측: chosen 20판 전부 149 고정). 프로세스 원자 카운터로 돌려
/// 후보를 확실히 골고루 순환시킨다(실험엔 균등 분포가 오히려 낫다).
pub(crate) fn pick_for_installed(installed_full: Option<&str>) -> UaProfile {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    pick_capped(version_pool(), installed_full, n)
}

/// `Emulation.setUserAgentOverride` 파라미터. UA 문자열 + acceptLanguage + userAgentMetadata
/// (Client Hints)를 **서로 일관되게** 만들어 sec-ch-ua/userAgentData 가 UA 버전과 어긋나지 않게
/// 한다. 이 오버라이드가 페이지의 UA·Client Hints 를 둘 다 지배하므로 내부 불일치가 없다.
pub(crate) fn set_user_agent_override_params(profile: &UaProfile) -> Value {
    json!({
        "userAgent": profile.user_agent,
        "acceptLanguage": "ko-KR,ko,en-US,en",
        "platform": "Windows",
        "userAgentMetadata": {
            "brands": [
                {"brand": "Not)A;Brand", "version": "99"},
                {"brand": "Google Chrome", "version": profile.major},
                {"brand": "Chromium", "version": profile.major}
            ],
            "fullVersionList": [
                {"brand": "Not)A;Brand", "version": "99.0.0.0"},
                {"brand": "Google Chrome", "version": profile.full_version},
                {"brand": "Chromium", "version": profile.full_version}
            ],
            "fullVersion": profile.full_version,
            "platform": "Windows",
            "platformVersion": "15.0.0",
            "architecture": "x86",
            "model": "",
            "mobile": false,
            "bitness": "64",
            "wow64": false
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pool() -> Vec<String> {
        // 최신순 API 응답을 흉내(내림차순): 150 두 빌드, 149, 148.
        vec![
            "150.0.7871.47".to_owned(),
            "150.0.7871.32".to_owned(),
            "149.0.7827.201".to_owned(),
            "148.0.7710.99".to_owned(),
        ]
    }

    #[test]
    fn reduced_ua_is_windows_and_major_only() {
        let p = build_profile("150.0.7871.47");
        assert!(p.user_agent.contains("Windows NT 10.0; Win64; x64"));
        assert!(p.user_agent.contains("Chrome/150.0.0.0"));
        assert!(!p.user_agent.contains("150.0.7871.47")); // 빌드번호는 UA 문자열엔 없다.
        assert!(!p.user_agent.contains("Headless"));
    }

    #[test]
    fn never_claims_higher_than_installed() {
        // 설치 149 → 어떤 엔트로피로도 150 을 주장하지 않는다.
        for e in 0..30u64 {
            let p = pick_capped(&pool(), Some("149.0.7827.201"), e);
            assert!(major_num(&p.full_version).unwrap() <= 149, "149 인데 {}", p.full_version);
        }
    }

    #[test]
    fn installed_149_rotates_among_le_149() {
        // 설치 149 → 149·148 은 나오고 150 은 절대 안 나온다.
        let seen: std::collections::HashSet<String> = (0..40u64)
            .map(|e| pick_capped(&pool(), Some("149.0.7827.201"), e).major)
            .collect();
        assert!(seen.contains("149"));
        assert!(seen.contains("148"));
        assert!(!seen.contains("150"));
    }

    #[test]
    fn installed_150_can_use_150_and_lower() {
        let seen: std::collections::HashSet<String> = (0..40u64)
            .map(|e| pick_capped(&pool(), Some("150.0.7871.47"), e).major)
            .collect();
        assert!(seen.contains("150"));
        assert!(seen.contains("149") || seen.contains("148"));
    }

    #[test]
    fn unknown_installed_falls_back_to_lowest() {
        // 설치 버전 못 읽음 → 풀 최저(148)로 고정(상향 주장 회피).
        let p = pick_capped(&pool(), None, 12345);
        assert_eq!(p.major, "148");
    }

    #[test]
    fn installed_own_version_always_candidate_even_if_not_in_pool() {
        // 설치 147(풀에 없음) → 풀엔 ≤147 후보가 없지만 설치 버전 자체로 선택된다.
        let p = pick_capped(&pool(), Some("147.0.7600.10"), 0);
        assert_eq!(p.full_version, "147.0.7600.10");
    }

    #[test]
    fn dedup_takes_latest_build_per_major() {
        // 최신순 입력 → 각 메이저의 첫(=최신) 빌드만, 메이저 골고루.
        let input: Vec<String> = [
            "150.0.7871.47",
            "150.0.7871.32",
            "149.0.7827.201",
            "149.0.7800.1",
            "148.0.7710.99",
        ]
        .iter()
        .map(|s| (*s).to_owned())
        .collect();
        let out = dedup_latest_per_major(&input, 6);
        assert_eq!(out, vec!["150.0.7871.47", "149.0.7827.201", "148.0.7710.99"]);
    }

    #[test]
    fn dedup_caps_major_count() {
        let input: Vec<String> = ["150.0.1.1", "149.0.1.1", "148.0.1.1", "147.0.1.1"]
            .iter()
            .map(|s| (*s).to_owned())
            .collect();
        assert_eq!(dedup_latest_per_major(&input, 2), vec!["150.0.1.1", "149.0.1.1"]);
    }

    #[test]
    fn override_params_keep_ua_and_hints_same_version() {
        let p = build_profile("150.0.7871.47");
        let v = set_user_agent_override_params(&p);
        assert_eq!(v["userAgent"], Value::String(p.user_agent.clone()));
        assert_eq!(v["userAgentMetadata"]["fullVersion"], "150.0.7871.47");
        assert_eq!(v["userAgentMetadata"]["brands"][1]["version"], "150");
        assert_eq!(v["acceptLanguage"], "ko-KR,ko,en-US,en");
    }
}
