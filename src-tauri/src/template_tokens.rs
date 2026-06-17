//! 글 제목/본문/댓글의 변수 토큰을 실제 값으로 치환하는 순수 헬퍼.
//!
//! 프론트 미리보기(`resolveTemplate`, helpers.ts)와 **동일한 결과**를 실제 게시에서도
//! 내기 위한 백엔드 치환이다. 토큰은 작성기(writer-modal)가 정확히 아래 형태로 삽입한다.
//! - `#{종목명}` / `#{종목코드}` : 종목토론방 전용(종목별 name/code)
//! - `#{링크}` : 종토/카페/밴드 공통. 링크값(linkOverride)이 있으면 그 값, 비우면
//!   종목별 시세 링크(코드가 있을 때만). 카페/밴드는 코드가 없어 linkOverride만 쓴다.

const NAME_TOKEN: &str = "#{종목명}";
const CODE_TOKEN: &str = "#{종목코드}";
const LINK_TOKEN: &str = "#{링크}";

/// 종목 코드의 네이버 시세 링크. 코드가 비면 빈 문자열(프론트 `jobLink`와 동일 규칙).
pub fn stock_price_link(code: &str) -> String {
    let code = code.trim();
    if code.is_empty() {
        String::new()
    } else {
        format!("https://finance.naver.com/item/main.naver?code={code}")
    }
}

/// `#{링크}` 치환값: 사용자가 링크값을 넣었으면 그 값, 비웠으면 종목 시세 링크.
/// (프론트 `resolveTemplate`의 `linkOverride.trim() || jobLink(job)`와 동일.)
pub fn resolve_link(link_override: &str, code: &str) -> String {
    let o = link_override.trim();
    if o.is_empty() {
        stock_price_link(code)
    } else {
        o.to_owned()
    }
}

/// 종목토론방용: `#{종목명}`/`#{종목코드}`/`#{링크}` 를 모두 치환한다.
pub fn resolve_forum(text: &str, name: &str, code: &str, link: &str) -> String {
    text.replace(NAME_TOKEN, name)
        .replace(CODE_TOKEN, code)
        .replace(LINK_TOKEN, link)
}

/// 카페·밴드용: `#{링크}`는 실제 값으로 치환하고, 종목 토큰(`#{종목명}`/`#{종목코드}`)은
/// **빈 문자열로 지운다**. 카페·밴드엔 종목 개념이 없어, 변수명이 글에 그대로 남지 않도록
/// 빈값으로 대체한다(프론트 `resolveTemplate`의 비-forum 동작과 일치).
pub fn resolve_cafe_band(text: &str, link: &str) -> String {
    text.replace(NAME_TOKEN, "")
        .replace(CODE_TOKEN, "")
        .replace(LINK_TOKEN, link)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn price_link_from_code_or_empty() {
        assert_eq!(
            stock_price_link("005930"),
            "https://finance.naver.com/item/main.naver?code=005930"
        );
        assert_eq!(stock_price_link("  "), "");
        assert_eq!(stock_price_link(""), "");
    }

    #[test]
    fn link_uses_override_when_present_else_price_link() {
        // 링크값 입력 → 그 값
        assert_eq!(resolve_link("https://x.com/a", "005930"), "https://x.com/a");
        // 공백만 → 비운 것으로 보고 시세 링크
        assert_eq!(
            resolve_link("   ", "005930"),
            "https://finance.naver.com/item/main.naver?code=005930"
        );
        // 비움 + 코드 없음(카페/밴드) → 빈 문자열
        assert_eq!(resolve_link("", ""), "");
    }

    #[test]
    fn forum_replaces_all_three_tokens() {
        let text = "#{종목명}(#{종목코드}) 토론 보러가기 #{링크}";
        let out = resolve_forum(
            text,
            "삼성전자",
            "005930",
            "https://finance.naver.com/item/main.naver?code=005930",
        );
        assert_eq!(
            out,
            "삼성전자(005930) 토론 보러가기 https://finance.naver.com/item/main.naver?code=005930"
        );
    }

    #[test]
    fn forum_replaces_every_occurrence() {
        assert_eq!(
            resolve_forum("#{종목명} #{종목명}", "SK하이닉스", "000660", ""),
            "SK하이닉스 SK하이닉스"
        );
    }

    #[test]
    fn cafe_band_blanks_stock_tokens_and_keeps_link() {
        // 카페/밴드: #{링크}는 치환, #{종목명}/#{종목코드}는 빈값으로 지운다(변수명 미노출).
        let text = "#{종목명} 링크: #{링크} 코드 #{종목코드}";
        assert_eq!(
            resolve_cafe_band(text, "https://band.us/123"),
            " 링크: https://band.us/123 코드 "
        );
    }

    #[test]
    fn replaces_token_even_when_text_is_attached_without_space() {
        // 순수 변수 치환: #{링크} 앞뒤에 글자가 바로 붙어 있어도 토큰만 정확히 바뀐다.
        assert_eq!(
            resolve_forum("앞#{링크}뒤", "삼성", "005930", "http://x"),
            "앞http://x뒤"
        );
        assert_eq!(
            resolve_forum("보세요->#{종목명}(#{종목코드})!", "삼성전자", "005930", ""),
            "보세요->삼성전자(005930)!"
        );
        assert_eq!(
            resolve_cafe_band("링크#{링크}끝", "http://y"),
            "링크http://y끝"
        );
    }

    #[test]
    fn no_tokens_passes_through_unchanged() {
        assert_eq!(
            resolve_forum("그냥 본문", "삼성", "005930", "x"),
            "그냥 본문"
        );
        assert_eq!(resolve_cafe_band("그냥 본문", "x"), "그냥 본문");
    }
}
