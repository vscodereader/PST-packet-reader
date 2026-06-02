//! 가입 카페 목록 모델.
//!
//! 응답 봉투는 `apis.naver.com/cafe-home-web/.../join-cafes/groups` 실측 기준이며,
//! 사용자에게 노출할 [`JoinedCafe`]만 ts-rs로 내보낸다. 민감 필드
//! (`memberKey`, `st` JWT)는 내부 구조체에서 아예 선언하지 않아 자동 배제된다.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::naver_cafe::error::{ErrorEnvelope, NaverCafeCommonErrorData};

/// 가입 카페 목록 조회 오류 타입 — 공통 오류 봉투 재사용.
pub type JoinedCafesError = ErrorEnvelope<NaverCafeCommonErrorData>;

/// UI로 반환하는 가입 카페 1건 (slim).
///
/// 원본 응답의 다수 필드 중 게시 대상 선택에 필요한 것만 추린다.
/// `cafe_url`은 전체 URL이 아니라 **슬러그**다(`cafe.naver.com/<cafe_url>`).
// 주의: 이 파일은 다른 바인딩 소스(src/ipc, src/naver_cafe)보다 한 단계 깊어
// (joined_cafes/) export_to 경로의 `../`가 하나 적다. [[project_ts_rs_export_path]]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, TS)]
#[ts(export, export_to = "../../src/shared/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct JoinedCafe {
    /// 숫자 카페 ID. (JS `number` — [`crate::ipc::cafes::Board`] 참고)
    #[ts(type = "number")]
    pub cafe_id: u64,
    /// 카페 표시 이름.
    pub cafe_name: String,
    /// 카페 슬러그 (`cafe.naver.com/<cafe_url>`).
    pub cafe_url: String,
    /// 이 계정의 카페 내 닉네임.
    pub member_nickname: String,
    /// 이 계정의 등급명 (예: "카페매니저", "성실회원") — 글쓰기 권한 신호.
    pub member_levelname: String,
    /// 내가 관리(매니저)하는 카페인지 여부.
    pub managing_cafe: bool,
    /// 휴면 카페 여부(글쓰기 제약 신호).
    pub dormant_cafe: bool,
}

// ---------------------------------------------------------------------------
// 내부 응답 봉투 (deserialize 전용) — message.result.groups[].cafes[]
// ---------------------------------------------------------------------------

/// 최상위 응답: `{ "message": { "result": { ... } } }`.
#[derive(Debug, Clone, Deserialize)]
pub(crate) struct JoinCafesEnvelope {
    pub message: JoinCafesMessage,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct JoinCafesMessage {
    pub result: JoinCafesResult,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct JoinCafesResult {
    #[serde(default)]
    pub groups: Vec<JoinCafeGroup>,
    pub page_info: PageInfo,
}

/// 그룹(사용자 폴더) 1개와 그 안의 카페들.
#[derive(Debug, Clone, Deserialize)]
pub(crate) struct JoinCafeGroup {
    #[serde(default)]
    pub cafes: Vec<JoinCafeItem>,
}

/// 페이지 정보 — `last_page`까지 page를 증가시키며 순회한다.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PageInfo {
    pub last_page: bool,
}

/// 응답의 카페 항목 — 필요한 키만 선언(민감 `memberKey`/`st`는 무시됨).
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct JoinCafeItem {
    pub cafe_id: u64,
    pub cafe_name: String,
    pub cafe_url: String,
    pub member_nickname: String,
    pub member_levelname: String,
    pub managing_cafe: bool,
    pub dormant_cafe: bool,
}

impl From<JoinCafeItem> for JoinedCafe {
    fn from(it: JoinCafeItem) -> Self {
        JoinedCafe {
            cafe_id: it.cafe_id,
            cafe_name: it.cafe_name,
            cafe_url: it.cafe_url,
            member_nickname: it.member_nickname,
            member_levelname: it.member_levelname,
            managing_cafe: it.managing_cafe,
            dormant_cafe: it.dormant_cafe,
        }
    }
}

impl JoinCafesEnvelope {
    /// 모든 그룹의 카페를 평탄화해 [`JoinedCafe`] 목록으로 변환한다.
    pub(crate) fn into_cafes(self) -> Vec<JoinedCafe> {
        self.message
            .result
            .groups
            .into_iter()
            .flat_map(|g| g.cafes)
            .map(JoinedCafe::from)
            .collect()
    }

    /// 마지막 페이지 여부.
    pub(crate) fn is_last_page(&self) -> bool {
        self.message.result.page_info.last_page
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const REAL_FIXTURE: &str = include_str!("fixtures/join_cafes_groups_success.json");

    #[test]
    fn joined_cafe_serializes_camel_case() {
        let cafe = JoinedCafe {
            cafe_id: 31732304,
            cafe_name: "test64979381".to_string(),
            cafe_url: "bluegrayoc3uc".to_string(),
            member_nickname: "Nokk1968".to_string(),
            member_levelname: "카페매니저".to_string(),
            managing_cafe: true,
            dormant_cafe: false,
        };
        let json = serde_json::to_value(&cafe).expect("직렬화 실패");
        assert!(json.get("cafeId").is_some(), "cafeId 키가 없음");
        assert!(json.get("memberLevelname").is_some(), "memberLevelname 키가 없음");
        assert!(json.get("managingCafe").is_some(), "managingCafe 키가 없음");
        assert!(json.get("cafe_id").is_none(), "snake_case 키가 있으면 안 됨");
    }

    #[test]
    fn joined_cafe_round_trips() {
        let original = JoinedCafe {
            cafe_id: 10000260,
            cafe_name: "자출사".to_string(),
            cafe_url: "bikecity".to_string(),
            member_nickname: "Nokk1968".to_string(),
            member_levelname: "성실회원".to_string(),
            managing_cafe: false,
            dormant_cafe: false,
        };
        let json = serde_json::to_string(&original).expect("직렬화 실패");
        let restored: JoinedCafe = serde_json::from_str(&json).expect("역직렬화 실패");
        assert_eq!(original, restored);
    }

    #[test]
    fn parses_real_fixture_into_flattened_cafes() {
        let envelope: JoinCafesEnvelope =
            serde_json::from_str(REAL_FIXTURE).expect("실측 fixture 역직렬화 실패");
        assert!(envelope.is_last_page(), "fixture는 lastPage=true여야 함");

        let cafes = envelope.into_cafes();
        assert_eq!(cafes.len(), 3, "그룹 안 카페 3건이 평탄화되어야 함");

        let first = &cafes[0];
        assert_eq!(first.cafe_id, 31732304);
        assert_eq!(first.cafe_name, "test64979381");
        assert_eq!(first.cafe_url, "bluegrayoc3uc");
        assert_eq!(first.member_levelname, "카페매니저");
        assert!(first.managing_cafe);
        assert!(!first.dormant_cafe);
    }

    #[test]
    fn ignores_sensitive_member_key_and_st_fields() {
        // 민감 필드가 응답에 있어도 모델엔 들어오지 않는다(역직렬화 성공 + 평탄화).
        let cafes = serde_json::from_str::<JoinCafesEnvelope>(REAL_FIXTURE)
            .expect("역직렬화 실패")
            .into_cafes();
        // 직렬화 결과 어디에도 더미 토큰이 없어야 한다.
        let serialized = serde_json::to_string(&cafes).expect("직렬화 실패");
        assert!(!serialized.contains("memberKey"), "memberKey가 노출됨");
        assert!(!serialized.contains("DUMMY_ST_TOKEN"), "st JWT가 노출됨");
    }
}
