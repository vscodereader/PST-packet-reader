//! 툴바 블록(프론트) → SmartEditor documentModel `components[]` 변환(순수 함수).
//!
//! 네이버 블로그 편집기에서 버튼으로 만든 글과 **동일한** documentModel을 만들기 위해, 프론트의
//! 블록 배열(텍스트/소스코드/일정/사진/파일/링크/스티커/장소)을 각 `@ctype` 컴포넌트로 옮긴다.
//! 스펙(패킷 실측, `blog-toolbar-spec.md`) JSON을 그대로 재현하며, 컴포넌트/노드 id는
//! [`write_client`](super::write_client)의 `se_id`(SE-uuid) 생성기를 재사용해 문서 내 고유성을
//! 맞춘다. 사진/파일/링크/스티커/장소 블록은 **이미 보조 API로 해석된 데이터**(src·fileId·
//! oglinkSign 등)를 프론트가 채워 넘기므로 여기서는 네트워크 없이 순수하게 JSON만 조립한다.

use serde::Deserialize;
use serde_json::{json, Value};

use super::write_client::se_id;

/// 문단/컴포넌트 정렬. documentModel의 `align` 문자열("left"/"center"/"right"/"justify").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Align {
    /// 왼쪽 정렬(기본).
    #[default]
    Left,
    /// 가운데 정렬.
    Center,
    /// 오른쪽 정렬.
    Right,
    /// 양쪽 정렬.
    Justify,
}

impl Align {
    /// documentModel `align`에 들어가는 문자열.
    pub fn as_str(self) -> &'static str {
        match self {
            Align::Left => "left",
            Align::Center => "center",
            Align::Right => "right",
            Align::Justify => "justify",
        }
    }
}

/// 텍스트 블록. 블록 전체에 서식(굵기/기울기/밑줄/취소선)과 정렬을 적용한다(편집기 하단 툴바 미러).
/// `text`의 줄바꿈(`\n`)마다 한 문단(paragraph)이 된다.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TextBlock {
    /// 본문 텍스트(줄바꿈 = 문단 구분).
    pub text: String,
    /// 정렬(기본 left).
    #[serde(default)]
    pub align: Align,
    /// 굵기.
    #[serde(default)]
    pub bold: bool,
    /// 기울기.
    #[serde(default)]
    pub italic: bool,
    /// 밑줄.
    #[serde(default)]
    pub underline: bool,
    /// 취소선.
    #[serde(default)]
    pub strike_through: bool,
}

/// 소스코드 블록(순수 클라이언트).
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodeBlock {
    /// 코드 본문.
    pub code: String,
    /// 정렬(기본 justify — 편집기 기본).
    #[serde(default = "align_justify")]
    pub align: Align,
}

fn align_justify() -> Align {
    Align::Justify
}

/// 일정 블록(순수 클라이언트).
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScheduleBlock {
    /// 일정 제목.
    pub title: String,
    /// 시작 시각(ISO8601, 예: `2026-07-14T10:40:09+09:00`).
    pub start_at: String,
    /// 날짜만 여부(시각 없음).
    #[serde(default)]
    pub date_only: bool,
    /// 정렬(기본 left).
    #[serde(default)]
    pub align: Align,
}

/// 파일 블록. 업로드 응답의 `fileId`/`fileName`/`fileSize`를 프론트가 미리 채운다.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileBlock {
    /// 업로드 응답 fileId.
    pub file_id: String,
    /// 파일명.
    pub file_name: String,
    /// 파일 크기(bytes).
    pub file_size: u64,
}

/// 사진 블록. 업로드로 얻은 src/path/크기 등을 프론트가 미리 채운다(blogfiles.pstatic.net).
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageBlock {
    /// 이미지 URL(blogfiles.pstatic.net/.../x.png?type=w1).
    pub src: String,
    /// 이미지 경로(`/.../x.png`).
    pub path: String,
    /// 도메인(`https://blogfiles.pstatic.net`).
    pub domain: String,
    /// 파일 크기(bytes).
    pub file_size: u64,
    /// 표시 너비.
    pub width: u32,
    /// 표시 높이.
    pub height: u32,
    /// 원본 너비.
    pub original_width: u32,
    /// 원본 높이.
    pub original_height: u32,
    /// 파일명.
    pub file_name: String,
}

/// 링크(oglink) 블록. oglink API 응답(title/domain/description/thumbnail/oglinkSign)을 프론트가 채운다.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OglinkBlock {
    /// 링크 제목.
    pub title: String,
    /// 도메인.
    pub domain: String,
    /// 원본 URL.
    pub link: String,
    /// 썸네일 이미지 URL.
    pub thumbnail_src: String,
    /// 썸네일 너비.
    pub thumbnail_width: u32,
    /// 썸네일 높이.
    pub thumbnail_height: u32,
    /// 설명.
    pub description: String,
    /// oglink 서명(응답 oglinkSign).
    pub oglink_sign: String,
}

/// 스티커 블록. packCode/seq만 있으면 썸네일 src는 규칙으로 만든다.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StickerBlock {
    /// 스티커 팩 코드(예: motion2d_01).
    pub pack_code: String,
    /// 팩 내 스티커 seq.
    pub seq: u32,
    /// 정렬(기본 left).
    #[serde(default)]
    pub align: Align,
}

/// 장소 검색 결과 1건(placesMap의 원소).
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Place {
    /// 장소 id(placeId).
    pub place_id: String,
    /// 장소명.
    pub name: String,
    /// 주소(도로명 우선).
    pub address: String,
    /// 위도(y).
    pub latitude: String,
    /// 경도(x).
    pub longitude: String,
    /// 검색 유형(place.type, 예: "s").
    pub search_type: String,
    /// 전화번호.
    #[serde(default)]
    pub tel: String,
}

/// 장소(지도) 블록. staticmap 썸네일 + 장소 목록을 프론트가 채운다.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlacesMapBlock {
    /// staticmap 이미지 URL(썸네일 src).
    pub thumbnail_src: String,
    /// 장소 목록.
    pub places: Vec<Place>,
    /// 정렬(기본 left).
    #[serde(default)]
    pub align: Align,
}

/// 툴바로 삽입/편집한 본문 블록. 프론트가 보내는 `type` 판별자로 갈린다.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Block {
    /// 텍스트(서식·정렬).
    Text(TextBlock),
    /// 소스코드.
    Code(CodeBlock),
    /// 일정.
    Schedule(ScheduleBlock),
    /// 파일.
    File(FileBlock),
    /// 사진.
    Image(ImageBlock),
    /// 링크.
    Oglink(OglinkBlock),
    /// 스티커.
    Sticker(StickerBlock),
    /// 장소(지도).
    PlacesMap(PlacesMapBlock),
}

/// 스티커 이미지 URL 규칙(storep-phinf.pstatic.net/{packCode}/original_{seq}.png).
fn sticker_src(pack_code: &str, seq: u32) -> String {
    format!("https://storep-phinf.pstatic.net/{pack_code}/original_{seq}.png")
}

/// 블록 배열 → documentModel `components[]`(순수 함수). 각 블록을 스펙의 `@ctype` 컴포넌트로 옮긴다.
pub fn blocks_to_components(blocks: &[Block]) -> Vec<Value> {
    blocks.iter().map(block_to_component).collect()
}

/// 블록 1개 → 컴포넌트 JSON(순수 함수).
pub fn block_to_component(block: &Block) -> Value {
    match block {
        Block::Text(b) => text_component(b),
        Block::Code(b) => code_component(b),
        Block::Schedule(b) => schedule_component(b),
        Block::File(b) => file_component(b),
        Block::Image(b) => image_component(b),
        Block::Oglink(b) => oglink_component(b),
        Block::Sticker(b) => sticker_component(b),
        Block::PlacesMap(b) => places_map_component(b),
    }
}

/// text 컴포넌트. 블록 서식을 모든 노드에 적용하고 줄바꿈마다 문단을 만든다.
///
/// 실측(cap10 RabbitWrite): text 컴포넌트에는 top-level `align`이 **없고**, 정렬은 각 문단의
/// `style:{align,@ctype:paragraphStyle}` 로 들어간다. 기본(left)일 때는 문단에 `style` 키 자체가
/// 없다(에디터가 생략). 예전 코드는 text에 top-level `align`을, 문단에 직접 `align` 문자열을 넣어
/// SmartEditor 스키마와 어긋났고, 이 때문에 사진/링크/스티커/파일 등 툴바 블록이 섞인 글이
/// RabbitWrite에서 `not acceptable`로 거부됐다(순수 텍스트 경로 `build_document_model`은 이
/// 필드가 없어 정상 발행됨).
fn text_component(b: &TextBlock) -> Value {
    let node_style = json!({
        "bold": b.bold,
        "italic": b.italic,
        "underline": b.underline,
        "strikeThrough": b.strike_through,
        "@ctype": "nodeStyle"
    });
    let paragraphs: Vec<Value> = b
        .text
        .split('\n')
        .map(|line| {
            let mut paragraph = json!({
                "id": se_id(),
                "nodes": [ {
                    "id": se_id(),
                    "value": line,
                    "style": node_style,
                    "@ctype": "textNode"
                } ],
                "@ctype": "paragraph"
            });
            // left(기본)이 아닐 때만 문단 정렬 style을 붙인다(실측: left 문단엔 style 키 없음).
            if b.align != Align::Left {
                paragraph["style"] = json!({
                    "align": b.align.as_str(),
                    "@ctype": "paragraphStyle"
                });
            }
            paragraph
        })
        .collect();
    json!({
        "id": se_id(),
        "layout": "default",
        "value": paragraphs,
        "@ctype": "text"
    })
}

/// code 컴포넌트.
fn code_component(b: &CodeBlock) -> Value {
    json!({
        "id": se_id(),
        "layout": "default",
        "fontSizeCode": "fs13",
        "codeContents": b.code,
        "align": b.align.as_str(),
        "@ctype": "code"
    })
}

/// schedule 컴포넌트.
fn schedule_component(b: &ScheduleBlock) -> Value {
    json!({
        "id": se_id(),
        "layout": "default",
        "align": b.align.as_str(),
        "title": b.title,
        "startAt": b.start_at,
        "dateOnly": b.date_only,
        "@ctype": "schedule"
    })
}

/// file 컴포넌트.
fn file_component(b: &FileBlock) -> Value {
    json!({
        "id": se_id(),
        "layout": "default",
        "fileId": b.file_id,
        "fileName": b.file_name,
        "fileSize": b.file_size,
        "@ctype": "file"
    })
}

/// image 컴포넌트.
fn image_component(b: &ImageBlock) -> Value {
    json!({
        "id": se_id(),
        "layout": "default",
        "src": b.src,
        "internalResource": true,
        "represent": true,
        "path": b.path,
        "domain": b.domain,
        "fileSize": b.file_size,
        "width": b.width,
        "widthPercentage": 0,
        "height": b.height,
        "originalWidth": b.original_width,
        "originalHeight": b.original_height,
        "fileName": b.file_name,
        "format": "normal",
        "displayFormat": "normal",
        "imageLoaded": true,
        "contentMode": "normal",
        "origin": { "srcFrom": "local", "@ctype": "imageOrigin" },
        "ai": false,
        "@ctype": "image"
    })
}

/// oglink 컴포넌트.
fn oglink_component(b: &OglinkBlock) -> Value {
    json!({
        "id": se_id(),
        "layout": "image",
        "title": b.title,
        "domain": b.domain,
        "link": b.link,
        "thumbnail": {
            "src": b.thumbnail_src,
            "width": b.thumbnail_width,
            "height": b.thumbnail_height,
            "@ctype": "thumbnail"
        },
        "description": b.description,
        "video": false,
        "oglinkSign": b.oglink_sign,
        "@ctype": "oglink"
    })
}

/// sticker 컴포넌트.
fn sticker_component(b: &StickerBlock) -> Value {
    json!({
        "id": se_id(),
        "layout": "default",
        "align": b.align.as_str(),
        "packCode": b.pack_code,
        "seq": b.seq,
        "thumbnail": {
            "src": sticker_src(&b.pack_code, b.seq),
            "width": 185,
            "height": 160,
            "@ctype": "thumbnail"
        },
        "format": "normal",
        "@ctype": "sticker"
    })
}

/// placesMap 컴포넌트.
fn places_map_component(b: &PlacesMapBlock) -> Value {
    let places: Vec<Value> = b
        .places
        .iter()
        .map(|p| {
            json!({
                "placeId": p.place_id,
                "name": p.name,
                "address": p.address,
                "latlng": {
                    "latitude": p.latitude,
                    "longitude": p.longitude,
                    "@ctype": "position"
                },
                "searchType": p.search_type,
                "tel": p.tel,
                "@ctype": "place"
            })
        })
        .collect();
    json!({
        "id": se_id(),
        "layout": "default",
        "align": b.align.as_str(),
        "searchEngine": "naver",
        "thumbnail": {
            "src": b.thumbnail_src,
            "@ctype": "thumbnail"
        },
        "places": places,
        "@ctype": "placesMap"
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(json_str: &str) -> Block {
        serde_json::from_str(json_str).unwrap()
    }

    #[test]
    fn text_block_applies_style_and_align_and_splits_paragraphs() {
        let b = parse(
            r#"{"type":"text","text":"첫줄\n둘째줄","align":"center","bold":true,"underline":true}"#,
        );
        let c = block_to_component(&b);
        assert_eq!(c["@ctype"], "text");
        assert_eq!(c["layout"], "default");
        // 실측(cap10): text 컴포넌트에 top-level align 키가 없다.
        assert!(c.get("align").is_none(), "text 컴포넌트에 top-level align이 있으면 안 됨");
        let paras = c["value"].as_array().unwrap();
        assert_eq!(paras.len(), 2);
        assert_eq!(paras[0]["@ctype"], "paragraph");
        // 정렬은 문단의 style:{align,@ctype:paragraphStyle}(직접 align 필드 아님).
        assert!(paras[0].get("align").is_none(), "문단에 직접 align 필드가 있으면 안 됨");
        assert_eq!(paras[0]["style"]["align"], "center");
        assert_eq!(paras[0]["style"]["@ctype"], "paragraphStyle");
        assert_eq!(paras[1]["style"]["align"], "center");
        assert_eq!(paras[0]["nodes"][0]["value"], "첫줄");
        assert_eq!(paras[1]["nodes"][0]["value"], "둘째줄");
        let style = &paras[0]["nodes"][0]["style"];
        assert_eq!(style["bold"], true);
        assert_eq!(style["italic"], false);
        assert_eq!(style["underline"], true);
        assert_eq!(style["strikeThrough"], false);
        assert_eq!(style["@ctype"], "nodeStyle");
        assert_eq!(paras[0]["nodes"][0]["@ctype"], "textNode");
    }

    #[test]
    fn text_block_defaults_left_and_no_paragraph_style() {
        let b = parse(r#"{"type":"text","text":"평문"}"#);
        let c = block_to_component(&b);
        // 실측(cap10): text에 top-level align 없음, left 문단엔 paragraphStyle 없음.
        assert!(c.get("align").is_none());
        assert!(
            c["value"][0].get("style").is_none(),
            "left(기본) 문단엔 paragraphStyle이 없어야 함"
        );
        let style = &c["value"][0]["nodes"][0]["style"];
        assert_eq!(style["bold"], false);
        assert_eq!(style["strikeThrough"], false);
        assert_eq!(style["@ctype"], "nodeStyle");
    }

    #[test]
    fn code_block_matches_spec() {
        let b = parse(r#"{"type":"code","code":"let x = 1;"}"#);
        let c = block_to_component(&b);
        assert_eq!(c["@ctype"], "code");
        assert_eq!(c["layout"], "default");
        assert_eq!(c["fontSizeCode"], "fs13");
        assert_eq!(c["codeContents"], "let x = 1;");
        assert_eq!(c["align"], "justify");
    }

    #[test]
    fn schedule_block_matches_spec() {
        let b = parse(
            r#"{"type":"schedule","title":"안녕","startAt":"2026-07-14T10:40:09+09:00","dateOnly":false}"#,
        );
        let c = block_to_component(&b);
        assert_eq!(c["@ctype"], "schedule");
        assert_eq!(c["layout"], "default");
        assert_eq!(c["align"], "left");
        assert_eq!(c["title"], "안녕");
        assert_eq!(c["startAt"], "2026-07-14T10:40:09+09:00");
        assert_eq!(c["dateOnly"], false);
    }

    #[test]
    fn file_block_matches_spec() {
        let b = parse(
            r#"{"type":"file","fileId":"F123","fileName":"a.pdf","fileSize":2048}"#,
        );
        let c = block_to_component(&b);
        assert_eq!(c["@ctype"], "file");
        assert_eq!(c["fileId"], "F123");
        assert_eq!(c["fileName"], "a.pdf");
        assert_eq!(c["fileSize"], 2048);
    }

    #[test]
    fn image_block_matches_spec_fixed_fields() {
        let b = parse(
            r#"{"type":"image","src":"https://blogfiles.pstatic.net/x/y.png?type=w1","path":"/x/y.png","domain":"https://blogfiles.pstatic.net","fileSize":1000,"width":600,"height":400,"originalWidth":1200,"originalHeight":800,"fileName":"y.png"}"#,
        );
        let c = block_to_component(&b);
        assert_eq!(c["@ctype"], "image");
        assert_eq!(c["src"], "https://blogfiles.pstatic.net/x/y.png?type=w1");
        assert_eq!(c["internalResource"], true);
        assert_eq!(c["represent"], true);
        assert_eq!(c["path"], "/x/y.png");
        assert_eq!(c["domain"], "https://blogfiles.pstatic.net");
        assert_eq!(c["fileSize"], 1000);
        assert_eq!(c["width"], 600);
        assert_eq!(c["widthPercentage"], 0);
        assert_eq!(c["height"], 400);
        assert_eq!(c["originalWidth"], 1200);
        assert_eq!(c["originalHeight"], 800);
        assert_eq!(c["fileName"], "y.png");
        assert_eq!(c["format"], "normal");
        assert_eq!(c["displayFormat"], "normal");
        assert_eq!(c["imageLoaded"], true);
        assert_eq!(c["contentMode"], "normal");
        assert_eq!(c["origin"]["srcFrom"], "local");
        assert_eq!(c["origin"]["@ctype"], "imageOrigin");
        assert_eq!(c["ai"], false);
    }

    #[test]
    fn oglink_block_matches_spec() {
        let b = parse(
            r#"{"type":"oglink","title":"제목","domain":"naver.com","link":"https://naver.com","thumbnailSrc":"https://img/x.png","thumbnailWidth":300,"thumbnailHeight":200,"description":"설명","oglinkSign":"SIGN123"}"#,
        );
        let c = block_to_component(&b);
        assert_eq!(c["@ctype"], "oglink");
        assert_eq!(c["layout"], "image");
        assert_eq!(c["title"], "제목");
        assert_eq!(c["domain"], "naver.com");
        assert_eq!(c["link"], "https://naver.com");
        assert_eq!(c["thumbnail"]["src"], "https://img/x.png");
        assert_eq!(c["thumbnail"]["width"], 300);
        assert_eq!(c["thumbnail"]["height"], 200);
        assert_eq!(c["thumbnail"]["@ctype"], "thumbnail");
        assert_eq!(c["description"], "설명");
        assert_eq!(c["video"], false);
        assert_eq!(c["oglinkSign"], "SIGN123");
    }

    #[test]
    fn sticker_block_matches_spec_and_src_rule() {
        let b = parse(r#"{"type":"sticker","packCode":"motion2d_01","seq":10}"#);
        let c = block_to_component(&b);
        assert_eq!(c["@ctype"], "sticker");
        assert_eq!(c["layout"], "default");
        assert_eq!(c["align"], "left");
        assert_eq!(c["packCode"], "motion2d_01");
        assert_eq!(c["seq"], 10);
        assert_eq!(
            c["thumbnail"]["src"],
            "https://storep-phinf.pstatic.net/motion2d_01/original_10.png"
        );
        assert_eq!(c["thumbnail"]["width"], 185);
        assert_eq!(c["thumbnail"]["height"], 160);
        assert_eq!(c["thumbnail"]["@ctype"], "thumbnail");
        assert_eq!(c["format"], "normal");
    }

    #[test]
    fn places_map_block_matches_spec() {
        let b = parse(
            r#"{"type":"placesMap","thumbnailSrc":"https://map/static.png","places":[{"placeId":"1621706163","name":"장소","address":"서울로1","latitude":"37.5","longitude":"127.0","searchType":"s","tel":"02-123"}]}"#,
        );
        let c = block_to_component(&b);
        assert_eq!(c["@ctype"], "placesMap");
        assert_eq!(c["layout"], "default");
        assert_eq!(c["align"], "left");
        assert_eq!(c["searchEngine"], "naver");
        assert_eq!(c["thumbnail"]["src"], "https://map/static.png");
        assert_eq!(c["thumbnail"]["@ctype"], "thumbnail");
        let places = c["places"].as_array().unwrap();
        assert_eq!(places.len(), 1);
        assert_eq!(places[0]["placeId"], "1621706163");
        assert_eq!(places[0]["name"], "장소");
        assert_eq!(places[0]["address"], "서울로1");
        assert_eq!(places[0]["latlng"]["latitude"], "37.5");
        assert_eq!(places[0]["latlng"]["longitude"], "127.0");
        assert_eq!(places[0]["latlng"]["@ctype"], "position");
        assert_eq!(places[0]["searchType"], "s");
        assert_eq!(places[0]["tel"], "02-123");
        assert_eq!(places[0]["@ctype"], "place");
    }

    #[test]
    fn blocks_to_components_preserves_order_and_ids_unique() {
        let blocks = vec![
            parse(r#"{"type":"text","text":"a"}"#),
            parse(r#"{"type":"code","code":"b"}"#),
        ];
        let comps = blocks_to_components(&blocks);
        assert_eq!(comps.len(), 2);
        assert_eq!(comps[0]["@ctype"], "text");
        assert_eq!(comps[1]["@ctype"], "code");
        assert_ne!(comps[0]["id"], comps[1]["id"]);
        assert!(comps[0]["id"].as_str().unwrap().starts_with("SE-"));
    }
}
