import { describe, it, expect } from "vitest";

import {
  crawlToText,
  htmlToText,
  parseCafeBoardLink,
  unreadyNaverAccountIds,
} from "./publish-helpers";

describe("htmlToText", () => {
  it("flattens block tags and <br> into newlines", () => {
    expect(htmlToText("<p>첫 줄</p><p>둘째 줄</p>")).toBe("첫 줄\n둘째 줄");
    expect(htmlToText("a<br>b<br/>c")).toBe("a\nb\nc");
  });

  it("preserves <img> as a placeholder instead of dropping it", () => {
    // The regression: a chart/image-only body would otherwise flatten to "".
    expect(htmlToText('<img src="chart.png" alt="삼성전자 차트">')).toBe(
      "[이미지: 삼성전자 차트]",
    );
    expect(htmlToText('<img src="https://x/y.png">')).toBe(
      "[이미지: https://x/y.png]",
    );
    expect(htmlToText("<img>")).toBe("[이미지]");
  });

  it("keeps surrounding text when an image sits inside the body", () => {
    expect(htmlToText('<p>위</p><p><img alt="그림"></p><p>아래</p>')).toBe(
      "위\n[이미지: 그림]\n아래",
    );
  });

  it("preserves <a> links as '텍스트 (URL)' instead of dropping the href", () => {
    // The regression: 평문 변환이 <a>를 통째로 지워 링크 URL이 사라졌다.
    expect(htmlToText('<a href="https://naver.com">네이버</a>')).toBe(
      "네이버 (https://naver.com)",
    );
    // 표시 텍스트가 URL과 같으면 중복 없이 URL 하나만.
    expect(
      htmlToText('<a href="https://naver.com">https://naver.com</a>'),
    ).toBe("https://naver.com");
    // 본문 안에 섞여 있어도 주변 텍스트와 함께 보존.
    expect(
      htmlToText('<p>참고: <a href="https://x.com/a">여기</a> 클릭</p>'),
    ).toBe("참고: 여기 (https://x.com/a) 클릭");
  });

  it("collapses 3+ blank lines and trims", () => {
    expect(htmlToText("<p>a</p><p></p><p></p><p>b</p>")).toBe("a\n\nb");
  });
});

describe("crawlToText", () => {
  it("붙여넣은 일반 링크를 변환하지 않고 원문 그대로 둔다", () => {
    const url =
      "https://news.sbs.co.kr/news/endPage.do?news_id=N1008610332&plink=ORI&cooper=NAVER";
    expect(crawlToText(url, [])).toBe(url);
  });

  it("URL이 아닌 임의의 텍스트도 그대로 둔다", () => {
    expect(crawlToText("그냥 본문 내용", [])).toBe("그냥 본문 내용");
  });

  it("종목 6자리 코드 링크는 기존대로 시세 줄로 바꾼다(기능 유지)", () => {
    const stocks = [
      {
        code: "005930",
        name: "삼성전자",
        market: "코스피",
        posts: "0",
        price: "70,000",
        chg: 1.2,
      },
    ];
    expect(
      crawlToText(
        "https://finance.naver.com/item/main.naver?code=005930",
        stocks,
      ),
    ).toBe("삼성전자(005930) · 코스피 현재가 70,000 (▲1.2%)");
  });
});

describe("parseCafeBoardLink", () => {
  it("parses the SPA board URL (cafes/{id}/menus/{id})", () => {
    expect(
      parseCafeBoardLink("https://cafe.naver.com/f-e/cafes/31732304/menus/1"),
    ).toEqual({ cafeId: 31732304, menuId: 1 });
    // 글쓰기 URL 형태도 같은 패턴으로 잡힌다.
    expect(
      parseCafeBoardLink(
        "https://cafe.naver.com/ca-fe/cafes/31732304/menus/5/articles/write",
      ),
    ).toEqual({ cafeId: 31732304, menuId: 5 });
  });

  it("parses the query form (clubid/cafeId + menuId), case-insensitive", () => {
    expect(
      parseCafeBoardLink(
        "https://cafe.naver.com/ArticleList.nhn?search.clubid=31732304&search.menuid=7",
      ),
    ).toEqual({ cafeId: 31732304, menuId: 7 });
    expect(
      parseCafeBoardLink("https://cafe.naver.com/x?cafeId=999&menuId=3"),
    ).toEqual({ cafeId: 999, menuId: 3 });
  });

  it("decodes URL-encoded (even doubly) iframe_url_utf8 forms", () => {
    const url =
      "https://cafe.naver.com/myclub?iframe_url_utf8=%252FArticleList.nhn%253Fclubid%253D31732304%2526menuid%253D2";
    expect(parseCafeBoardLink(url)).toEqual({ cafeId: 31732304, menuId: 2 });
  });

  it("returns null when the menu (board) is missing — cafe-home link", () => {
    expect(parseCafeBoardLink("https://cafe.naver.com/myclub")).toBeNull();
    expect(
      parseCafeBoardLink("https://cafe.naver.com/f-e/cafes/31732304"),
    ).toBeNull();
  });

  it("returns null for empty/undefined input", () => {
    expect(parseCafeBoardLink(undefined)).toBeNull();
    expect(parseCafeBoardLink("")).toBeNull();
  });

  it("does not mis-match subclubid as the cafe id", () => {
    // `\b` 앵커가 subclubid의 뒷부분(clubid)을 cafeId로 오인하지 않게 한다.
    expect(
      parseCafeBoardLink("https://cafe.naver.com/x?subclubid=88&menuid=1"),
    ).toBeNull();
  });
});

describe("unreadyNaverAccountIds", () => {
  const accounts = [
    { id: "a1", platform: "naver" },
    { id: "a2", platform: "naver" },
    { id: "f1", platform: "forum" },
  ];

  it("flags selected 네이버 accounts whose board isn't resolved", () => {
    const picks = { a1: { boardName: "자유게시판" }, a2: { boardName: "" } };
    expect(
      unreadyNaverAccountIds(["a1", "a2"], accounts, picks, "post"),
    ).toEqual(["a2"]);
  });

  it("treats a missing pick as not-ready", () => {
    expect(unreadyNaverAccountIds(["a1"], accounts, {}, "both")).toEqual([
      "a1",
    ]);
  });

  it("ignores non-naver platforms", () => {
    expect(unreadyNaverAccountIds(["f1"], accounts, {}, "post")).toEqual([]);
  });

  it("is empty in comment mode (target is a URL/latest/popular, not a board)", () => {
    expect(
      unreadyNaverAccountIds(["a1", "a2"], accounts, {}, "comment"),
    ).toEqual([]);
  });

  it("is empty when every selected account has a board", () => {
    const picks = {
      a1: { boardName: "자유게시판" },
      a2: { boardName: "공지" },
    };
    expect(
      unreadyNaverAccountIds(["a1", "a2"], accounts, picks, "post"),
    ).toEqual([]);
  });
});
