# 0011. 카페 게시글 목록(최신글/인기글) 조회 API

Date: 2026-06-05

## Status

Accepted

Related:

- [ADR-0012](0012-comment-target-latest-popular-ui.md) — 이 API를 소비하는
  최신/인기 댓글 대상 UI.

## Context

댓글 대상을 사용자가 게시글 URL로 직접 붙여넣는 방식만 지원한다.
"최신글/인기글 중 N개"를 댓글 대상으로 삼으려면 카페의 게시글 목록을 받아와야
하는데, `src-tauri/src/naver_cafe/`에는 게시글 목록을 조회하는 모듈이 없다.

## Decision

신규 백엔드 모듈 `naver_cafe/article_list/`(client/models/parser/service —
`joined_cafes`/`menu` 구조 답습)를 추가한다.

- 프론트에서 네이버 API를 직접 부르지 않고 **백엔드**가 조회한다. 쿠키/세션이
  백엔드에만 있고 인증·CORS 처리가 클라이언트에 일원화돼 있기 때문이다.
- 정렬별로 **실측 엔드포인트**가 다르다: latest = `cafe-boardlist-api`,
  popular = 구 `cafe2` 계열 `WeeklyPopularArticleListV3`. base·envelope·스키마가
  달라 파서를 2갈래로 둔다(공개 `Article` 필드 형태는 통일).
- `list_cafe_articles(cafeId, sortBy, accountId)` IPC + ts-rs
  바인딩(`Article`/`ArticleListResponse`/`SortBy`)을 노출한다.
- 200-OK-with-error-body 처리는 기존 post/comment 클라이언트 패턴을 따른다.
- 쿠키/세션 값은 로그·에러·Debug 출력에 절대 노출하지 않는다.

## Consequences

- 쉬워지는 것: 최신/인기글을 댓글 대상으로 쓸 수 있는 기반 마련(#97 UI가 의존).
  인증이 백엔드 클라이언트에 일원화. wiremock으로 단위 검증 가능.
- 어려워지는 것 / 위험:
  - 네이버 **비공식 API** 의존 → 엔드포인트/스키마 변경 시 깨질 수 있다.
  - popular은 **주간 스코프**라 글이 적은 카페는 빈 응답(`{"result":[]}`)이 온다.
  - 멤버 전용 카페는 403(`apiErrorCode 45000`) → 가입한 카페로만 조회 가능.
  - 응답에 `pageInfo`/`lastPage`가 없어 페이지네이션 메타가 제한적이다.
- Follow-up: #97 UI가 이 API를 소비. 인기글 댓글TOP/좋아요TOP은 `SortBy` 확장
  여지로만 남긴다(미구현).
- 검증: wiremock 클라이언트 테스트(성공/에러바디/파싱실패) + 읽기 전용 예제
  `list_articles`로 실 네이버 검증(latest·popular 양쪽 정상 응답 확인).
