//! 네이버 스마트에디터 `contentJson` 생성기.
//!
//! 평문 본문 텍스트를 스마트에디터 문서 구조로 변환하고,
//! 게시글 등록 요청 바디의 `article.contentJson`에 넣을 직렬화된 JSON 문자열을 생성한다.

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// ID 공급자 트레이트
// ---------------------------------------------------------------------------

/// 스마트에디터 문서에 사용될 ID 문자열을 순서대로 공급하는 트레이트.
///
/// 컴포넌트·단락·텍스트노드 등의 ID 생성을 외부에서 주입(injection)할 수 있도록
/// 트레이트로 추상화한다. 이를 통해 테스트에서는 고정된 ID를 사용해
/// 직렬화 결과를 바이트 수준까지 결정론적으로 검증할 수 있다.
pub trait IdProvider {
    /// 다음 ID 문자열을 반환한다.
    fn next_id(&mut self) -> String;
}

/// 테스트 및 결정론적 ID 생성을 위한 고정 시퀀스 ID 공급자.
///
/// 내부 카운터를 증가시키며 `SE-{:024}` 형식의 ID를 반환한다.
/// `document.id`(ULID 자리)도 동일한 카운터로 발급된다.
pub struct SequentialIdProvider {
    counter: u64,
}

impl SequentialIdProvider {
    /// 카운터를 0으로 초기화한 새 공급자를 생성한다.
    pub fn new() -> Self {
        Self { counter: 0 }
    }
}

impl Default for SequentialIdProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl IdProvider for SequentialIdProvider {
    fn next_id(&mut self) -> String {
        let id = self.counter;
        self.counter += 1;
        // 첫 번째 ID(document.id)는 ULID 형식 자리에 사용되므로
        // SE- 접두어 없이 숫자만으로 채운다.
        if id == 0 {
            format!("{:026}", id)
        } else {
            // 컴포넌트/단락/노드 ID는 SE-<uuid> 형식을 흉내낸다.
            format!("SE-{:08x}-{:04x}-{:04x}-{:04x}-{:012x}", id, id, id, id, id)
        }
    }
}

// ---------------------------------------------------------------------------
// `di` 메타데이터 모델
// ---------------------------------------------------------------------------

/// ⚠️ 미확인 의미: di(dif/dio) 필드의 정확한 산출 규칙은 캡처로 확인되지 않음. 캡처값 구조를 기본값으로 복제함.
///
/// 스마트에디터 문서 정보(Document Info) 내 개별 옵션 항목.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Dia {
    /// ⚠️ 미확인 의미.
    pub t: u32,
    /// ⚠️ 미확인 의미.
    pub p: u32,
    /// ⚠️ 미확인 의미.
    pub st: u32,
    /// ⚠️ 미확인 의미.
    pub sk: u32,
}

/// ⚠️ 미확인 의미: di(dif/dio) 필드의 정확한 산출 규칙은 캡처로 확인되지 않음. 캡처값 구조를 기본값으로 복제함.
///
/// 스마트에디터 문서 정보(Document Info) 내 각 옵션 항목.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DocumentInfoOption {
    /// ⚠️ 미확인 의미.
    pub dis: String,
    /// ⚠️ 미확인 의미.
    pub dia: Dia,
}

/// ⚠️ 미확인 의미: di(dif/dio) 필드의 정확한 산출 규칙은 캡처로 확인되지 않음. 캡처값 구조를 기본값으로 복제함.
///
/// 스마트에디터 문서 정보(Document Info).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DocumentInfo {
    /// ⚠️ 미확인 의미.
    pub dif: bool,
    /// ⚠️ 미확인 의미.
    pub dio: Vec<DocumentInfoOption>,
}

/// 패킷 캡처에서 확인된 `di` 필드 기본값을 반환한다.
///
/// ⚠️ 미확인 의미: 정확한 산출 규칙은 알 수 없으며, 캡처값을 그대로 복제한다.
fn default_document_info() -> DocumentInfo {
    DocumentInfo {
        dif: false,
        dio: vec![
            DocumentInfoOption {
                dis: "N".to_string(),
                dia: Dia { t: 0, p: 0, st: 1, sk: 0 },
            },
            DocumentInfoOption {
                dis: "N".to_string(),
                dia: Dia { t: 0, p: 0, st: 15, sk: 1 },
            },
        ],
    }
}

// ---------------------------------------------------------------------------
// 문서 구조 모델
// ---------------------------------------------------------------------------

/// 스마트에디터 텍스트노드 — 단락 내 실제 텍스트를 담는 최소 단위.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Node {
    /// 노드 고유 ID (예: `SE-<uuid>`).
    pub id: String,
    /// 텍스트 내용.
    pub value: String,
    /// 노드 유형. 항상 `"textNode"`.
    #[serde(rename = "@ctype")]
    pub ctype: String,
}

/// 스마트에디터 단락(paragraph) — 하나 이상의 노드를 포함한다.
///
/// 컴포넌트의 `value` 배열 원소로 사용된다.
/// 단일 줄 본문 → 단락 1개 + 텍스트노드 1개.
/// 다중 줄 본문(`\n` 구분) → 줄 수만큼의 단락.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Paragraph {
    /// 단락 고유 ID (예: `SE-<uuid>`).
    pub id: String,
    /// 단락에 포함된 노드 목록.
    pub nodes: Vec<Node>,
    /// 노드 유형. 항상 `"paragraph"`.
    #[serde(rename = "@ctype")]
    pub ctype: String,
}

/// 스마트에디터 컴포넌트 — 단락 목록을 포함하는 최상위 콘텐츠 단위.
///
/// 패킷 캡처에서는 단일 `text` 컴포넌트가 확인되었다.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Component {
    /// 컴포넌트 고유 ID (예: `SE-<uuid>`).
    pub id: String,
    /// 레이아웃 유형. 항상 `"default"`.
    pub layout: String,
    /// 컴포넌트에 포함된 단락 목록.
    pub value: Vec<Paragraph>,
    /// 컴포넌트 유형. 항상 `"text"`.
    #[serde(rename = "@ctype")]
    pub ctype: String,
}

/// 스마트에디터 문서 본문.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SmartEditorDocument {
    /// 에디터 버전. 패킷 캡처에서 확인된 값: `"2.9.0"`.
    pub version: String,
    /// 테마. 패킷 캡처에서 확인된 값: `"default"`.
    pub theme: String,
    /// 언어. 패킷 캡처에서 확인된 값: `"ko-KR"`.
    pub language: String,
    /// 문서 고유 ID (ULID 형식 자리).
    pub id: String,
    /// 컴포넌트 목록.
    pub components: Vec<Component>,
    /// ⚠️ 미확인 의미: di 필드. 캡처값 구조를 기본값으로 사용.
    pub di: DocumentInfo,
}

/// `article.contentJson`에 직렬화되는 최상위 래퍼.
///
/// `document` 객체와 빈 문자열 `documentId`를 담는다.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ContentJsonRoot {
    /// 스마트에디터 문서 본문.
    pub document: SmartEditorDocument,
    /// 패킷 캡처에서 확인된 값: 빈 문자열(`""`).
    pub document_id: String,
}

// ---------------------------------------------------------------------------
// 문서 빌드 함수
// ---------------------------------------------------------------------------

/// 본문 텍스트와 ID 공급자를 받아 스마트에디터 문서 구조를 생성한다.
///
/// # 다중 줄 처리
/// `body`에 `\n`이 포함된 경우 각 줄을 별도의 단락으로 분리한다.
/// 단, 패킷 캡처에서 확인된 것은 단일 줄(단락 1개) 사례뿐이다.
/// 빈 입력(`""`)은 빈 텍스트노드를 가진 단락 1개를 생성한다.
///
/// # ID 생성 순서
/// 1. `document.id` (ULID 자리)
/// 2. 컴포넌트 ID
/// 3. 단락 ID (줄 순서대로)
/// 4. 노드 ID (단락 내 순서대로)
pub fn build_content_document(body: &str, ids: &mut impl IdProvider) -> ContentJsonRoot {
    let doc_id = ids.next_id();
    let component_id = ids.next_id();

    // 빈 본문이면 빈 문자열 단락 1개, 아니면 \n 으로 분리한 줄 수만큼 단락 생성.
    let lines: Vec<&str> = if body.is_empty() {
        vec![""]
    } else {
        body.split('\n').collect()
    };

    let paragraphs: Vec<Paragraph> = lines
        .into_iter()
        .map(|line| {
            let para_id = ids.next_id();
            let node_id = ids.next_id();
            Paragraph {
                id: para_id,
                nodes: vec![Node {
                    id: node_id,
                    value: line.to_string(),
                    ctype: "textNode".to_string(),
                }],
                ctype: "paragraph".to_string(),
            }
        })
        .collect();

    let component = Component {
        id: component_id,
        layout: "default".to_string(),
        value: paragraphs,
        ctype: "text".to_string(),
    };

    let document = SmartEditorDocument {
        version: "2.9.0".to_string(),
        theme: "default".to_string(),
        language: "ko-KR".to_string(),
        id: doc_id,
        components: vec![component],
        di: default_document_info(),
    };

    ContentJsonRoot {
        document,
        document_id: String::new(),
    }
}

/// 본문 텍스트를 받아 `article.contentJson`에 삽입할 직렬화된 JSON 문자열을 반환한다.
///
/// 내부적으로 [`build_content_document`]를 호출하고 [`serde_json::to_string`]으로 직렬화한다.
pub fn build_content_json_string(
    body: &str,
    ids: &mut impl IdProvider,
) -> Result<String, serde_json::Error> {
    let root = build_content_document(body, ids);
    serde_json::to_string(&root)
}

// ---------------------------------------------------------------------------
// 테스트
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn fixed_ids() -> SequentialIdProvider {
        SequentialIdProvider::new()
    }

    // ------------------------------------------------------------------
    // 구조 검증 — 단일 줄 본문 "74423"
    // ------------------------------------------------------------------

    #[test]
    fn single_line_body_component_ctype_is_text() {
        let root = build_content_document("74423", &mut fixed_ids());
        let comp = root.document.components.first().expect("컴포넌트가 없음");
        assert_eq!(comp.ctype, "text");
    }

    #[test]
    fn single_line_body_paragraph_ctype_is_paragraph() {
        let root = build_content_document("74423", &mut fixed_ids());
        let comp = root.document.components.first().expect("컴포넌트가 없음");
        let para = comp.value.first().expect("단락이 없음");
        assert_eq!(para.ctype, "paragraph");
    }

    #[test]
    fn single_line_body_node_ctype_is_text_node() {
        let root = build_content_document("74423", &mut fixed_ids());
        let comp = root.document.components.first().expect("컴포넌트가 없음");
        let para = comp.value.first().expect("단락이 없음");
        let node = para.nodes.first().expect("노드가 없음");
        assert_eq!(node.ctype, "textNode");
    }

    #[test]
    fn single_line_body_node_value_matches() {
        let root = build_content_document("74423", &mut fixed_ids());
        let comp = root.document.components.first().expect("컴포넌트가 없음");
        let para = comp.value.first().expect("단락이 없음");
        let node = para.nodes.first().expect("노드가 없음");
        assert_eq!(node.value, "74423");
    }

    // ------------------------------------------------------------------
    // 상수 검증 — version / theme / language / documentId
    // ------------------------------------------------------------------

    #[test]
    fn document_version_matches_captured_constant() {
        let root = build_content_document("74423", &mut fixed_ids());
        assert_eq!(root.document.version, "2.9.0");
    }

    #[test]
    fn document_theme_matches_captured_constant() {
        let root = build_content_document("74423", &mut fixed_ids());
        assert_eq!(root.document.theme, "default");
    }

    #[test]
    fn document_language_matches_captured_constant() {
        let root = build_content_document("74423", &mut fixed_ids());
        assert_eq!(root.document.language, "ko-KR");
    }

    #[test]
    fn document_id_is_empty_string() {
        let root = build_content_document("74423", &mut fixed_ids());
        assert_eq!(root.document_id, "");
    }

    // ------------------------------------------------------------------
    // 직렬화 — @ctype 키 및 라운드트립
    // ------------------------------------------------------------------

    #[test]
    fn serialized_string_contains_at_ctype_key() {
        let json = build_content_json_string("74423", &mut fixed_ids())
            .expect("직렬화 실패");
        assert!(
            json.contains("\"@ctype\""),
            "직렬화 결과에 @ctype 키가 없음: {}",
            json
        );
    }

    #[test]
    fn serialized_string_contains_body_text() {
        let json = build_content_json_string("74423", &mut fixed_ids())
            .expect("직렬화 실패");
        assert!(
            json.contains("74423"),
            "직렬화 결과에 본문 텍스트가 없음: {}",
            json
        );
    }

    #[test]
    fn serialized_string_round_trips_via_serde_json() {
        let mut ids = fixed_ids();
        let original = build_content_document("74423", &mut ids);

        let mut ids2 = fixed_ids();
        let json = build_content_json_string("74423", &mut ids2)
            .expect("직렬화 실패");

        let restored: ContentJsonRoot = serde_json::from_str(&json)
            .expect("역직렬화 실패");

        assert_eq!(original, restored, "@ctype rename이 역직렬화에서 동작해야 함");
    }

    #[test]
    fn serialized_at_ctype_value_is_preserved_after_round_trip() {
        let json = build_content_json_string("74423", &mut fixed_ids())
            .expect("직렬화 실패");
        let restored: ContentJsonRoot = serde_json::from_str(&json)
            .expect("역직렬화 실패");
        let comp = restored.document.components.first().expect("컴포넌트가 없음");
        assert_eq!(comp.ctype, "text");
        let para = comp.value.first().expect("단락이 없음");
        assert_eq!(para.ctype, "paragraph");
        let node = para.nodes.first().expect("노드가 없음");
        assert_eq!(node.ctype, "textNode");
    }

    // ------------------------------------------------------------------
    // 다중 줄 본문
    // ------------------------------------------------------------------

    #[test]
    fn multiline_body_produces_two_paragraphs() {
        let root = build_content_document("a\nb", &mut fixed_ids());
        let comp = root.document.components.first().expect("컴포넌트가 없음");
        assert_eq!(comp.value.len(), 2, "두 줄 입력은 단락 2개를 생성해야 함");
    }

    #[test]
    fn multiline_body_first_paragraph_node_value() {
        let root = build_content_document("a\nb", &mut fixed_ids());
        let comp = root.document.components.first().expect("컴포넌트가 없음");
        let first_node = comp.value[0].nodes.first().expect("노드가 없음");
        assert_eq!(first_node.value, "a");
    }

    #[test]
    fn multiline_body_second_paragraph_node_value() {
        let root = build_content_document("a\nb", &mut fixed_ids());
        let comp = root.document.components.first().expect("컴포넌트가 없음");
        let second_node = comp.value[1].nodes.first().expect("노드가 없음");
        assert_eq!(second_node.value, "b");
    }

    // ------------------------------------------------------------------
    // 빈 본문
    // ------------------------------------------------------------------

    #[test]
    fn empty_body_produces_one_paragraph() {
        let root = build_content_document("", &mut fixed_ids());
        let comp = root.document.components.first().expect("컴포넌트가 없음");
        assert_eq!(comp.value.len(), 1, "빈 입력은 단락 1개를 생성해야 함");
    }

    #[test]
    fn empty_body_node_value_is_empty_string() {
        let root = build_content_document("", &mut fixed_ids());
        let comp = root.document.components.first().expect("컴포넌트가 없음");
        let node = comp.value[0].nodes.first().expect("노드가 없음");
        assert_eq!(node.value, "", "빈 입력의 노드 value는 빈 문자열이어야 함");
    }

    // ------------------------------------------------------------------
    // di 메타데이터 기본값 검증
    // ------------------------------------------------------------------

    #[test]
    fn di_dif_is_false() {
        let root = build_content_document("74423", &mut fixed_ids());
        assert!(!root.document.di.dif);
    }

    #[test]
    fn di_dio_has_two_entries() {
        let root = build_content_document("74423", &mut fixed_ids());
        assert_eq!(root.document.di.dio.len(), 2);
    }

    #[test]
    fn di_first_option_matches_captured_value() {
        let root = build_content_document("74423", &mut fixed_ids());
        let first = &root.document.di.dio[0];
        assert_eq!(first.dis, "N");
        assert_eq!(first.dia.t, 0);
        assert_eq!(first.dia.p, 0);
        assert_eq!(first.dia.st, 1);
        assert_eq!(first.dia.sk, 0);
    }

    #[test]
    fn di_second_option_matches_captured_value() {
        let root = build_content_document("74423", &mut fixed_ids());
        let second = &root.document.di.dio[1];
        assert_eq!(second.dis, "N");
        assert_eq!(second.dia.t, 0);
        assert_eq!(second.dia.p, 0);
        assert_eq!(second.dia.st, 15);
        assert_eq!(second.dia.sk, 1);
    }
}
