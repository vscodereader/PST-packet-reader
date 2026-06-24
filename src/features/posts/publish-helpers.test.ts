import { describe, it, expect } from "vitest";

import {
  clampCommentCount,
  crawlToText,
  distributeStocksEvenly,
  htmlToText,
  parseBlogPostLink,
  parseCafeBoardLink,
  unreadyNaverAccountIds,
} from "./publish-helpers";

describe("distributeStocksEvenly", () => {
  const sizes = (codes: string[], buckets: number) =>
    distributeStocksEvenly(codes, buckets).map((b) => b.length);

  it("spreads the remainder to the FRONT buckets (#267-5: 5,5,4,4 not 5,5,5,3)", () => {
    const codes = Array.from({ length: 18 }, (_, i) => `c${i}`);
    expect(sizes(codes, 4)).toEqual([5, 5, 4, 4]);
  });

  it("divides evenly when divisible", () => {
    const codes = Array.from({ length: 12 }, (_, i) => `c${i}`);
    expect(sizes(codes, 3)).toEqual([4, 4, 4]);
  });

  it("keeps every code exactly once, in order, with no overlap", () => {
    const codes = ["a", "b", "c", "d", "e"];
    const out = distributeStocksEvenly(codes, 3);
    expect(out).toEqual([["a", "b"], ["c", "d"], ["e"]]);
    expect(out.flat()).toEqual(codes);
  });

  it("gives empty trailing buckets when fewer stocks than accounts", () => {
    expect(sizes(["a", "b"], 4)).toEqual([1, 1, 0, 0]);
  });

  it("returns [] for non-positive bucket counts", () => {
    expect(distributeStocksEvenly(["a"], 0)).toEqual([]);
  });
});

describe("clampCommentCount", () => {
  it("keeps an in-range integer (including non-preset values)", () => {
    expect(clampCommentCount(7)).toBe(7);
    expect(clampCommentCount(1)).toBe(1);
    expect(clampCommentCount(50)).toBe(50);
  });

  it("clamps below 1 and above 50 to the bounds", () => {
    expect(clampCommentCount(0)).toBe(1);
    expect(clampCommentCount(-5)).toBe(1);
    expect(clampCommentCount(100)).toBe(50);
  });

  it("floors fractional input", () => {
    expect(clampCommentCount(3.9)).toBe(3);
  });

  it("falls back to 1 for missing or non-finite input", () => {
    expect(clampCommentCount(undefined)).toBe(1);
    expect(clampCommentCount(null)).toBe(1);
    expect(clampCommentCount(NaN)).toBe(1);
  });
});

describe("htmlToText", () => {
  it("flattens block tags and <br> into newlines", () => {
    expect(htmlToText("<p>첫 줄</p><p>둘째 줄</p>")).toBe("첫 줄\n둘째 줄");
    expect(htmlToText("a<br>b<br/>c")).toBe("a\nb\nc");
  });

  it("inserts a newline between a bare first line and following <div> lines (#267-1)", () => {
    // contentEditable에서 엔터를 치면 첫 줄은 평문, 다음 줄부터 <div>로 감싸인다. 닫는 태그만
    // \n으로 바꾸면 줄 사이가 아니라 끝에만 붙어 "하...미치겠네"로 달라붙던 버그의 회귀 테스트.
    expect(htmlToText("하...<div>미치겠네</div>")).toBe("하...\n미치겠네");
    // 줄 뒤에 토큰/링크가 와도 줄바꿈이 유지된다(토큰은 백엔드에서 실제 링크로 치환됨).
    expect(htmlToText("본문<div>#{링크}</div>")).toBe("본문\n#{링크}");
    // 전부 <div>로 감싸인 형태도 동일하게 한 번씩만 줄바꿈.
    expect(htmlToText("<div>첫</div><div>둘</div>")).toBe("첫\n둘");
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

  it("decodes HTML entities so they don't post literally", () => {
    // The regression: contentEditable serializes spaces as `&nbsp;` into innerHTML,
    // and stripping only tags left the literal "&nbsp;" text in the posted body.
    expect(htmlToText("앞&nbsp;뒤")).toBe("앞 뒤");
    expect(htmlToText("<p>한&nbsp;줄</p><p>둘&nbsp;째</p>")).toBe(
      "한 줄\n둘 째",
    );
    // 다른 흔한 엔티티도 디코드된다.
    expect(htmlToText("&lt;태그&gt;")).toBe("<태그>");
    expect(htmlToText("&quot;인용&quot;")).toBe('"인용"');
    expect(htmlToText("it&#39;s")).toBe("it's");
    // `&amp;`는 마지막에 풀어 이중 디코드되지 않는다: `&amp;nbsp;` → "&nbsp;"(공백 아님).
    expect(htmlToText("1 &amp; 2")).toBe("1 & 2");
    expect(htmlToText("&amp;nbsp;")).toBe("&nbsp;");
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

describe("parseBlogPostLink", () => {
  it("parses the path form blog.naver.com/{blogId}/{logNo} (blogId is a string)", () => {
    expect(
      parseBlogPostLink("https://blog.naver.com/press02/224311392458"),
    ).toEqual({ blogId: "press02", logNo: "224311392458" });
    // blogId가 숫자처럼 보여도 문자열로 보존한다.
    expect(parseBlogPostLink("https://blog.naver.com/cho41004/100")).toEqual({
      blogId: "cho41004",
      logNo: "100",
    });
  });

  it("parses the PostView.naver query form", () => {
    expect(
      parseBlogPostLink(
        "https://blog.naver.com/PostView.naver?blogId=press02&logNo=224311392458",
      ),
    ).toEqual({ blogId: "press02", logNo: "224311392458" });
  });

  it("decodes URL-encoded iframe/encoded forms", () => {
    const url =
      "https://blog.naver.com/x?u=%2FPostView.naver%3FblogId%3Dpress02%26logNo%3D999";
    expect(parseBlogPostLink(url)).toEqual({
      blogId: "press02",
      logNo: "999",
    });
  });

  it("returns null when logNo is missing or non-numeric", () => {
    expect(parseBlogPostLink("https://blog.naver.com/press02")).toBeNull();
    expect(
      parseBlogPostLink("https://blog.naver.com/PostView.naver?blogId=press02"),
    ).toBeNull();
  });

  it("returns null for empty/undefined input", () => {
    expect(parseBlogPostLink(undefined)).toBeNull();
    expect(parseBlogPostLink("")).toBeNull();
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
