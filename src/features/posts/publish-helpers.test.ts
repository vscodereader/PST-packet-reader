import { describe, it, expect } from "vitest";

import { htmlToText, unreadyNaverAccountIds } from "./publish-helpers";

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

  it("collapses 3+ blank lines and trims", () => {
    expect(htmlToText("<p>a</p><p></p><p></p><p>b</p>")).toBe("a\n\nb");
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
