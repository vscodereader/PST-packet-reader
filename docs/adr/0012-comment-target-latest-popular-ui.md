# 0012. 최신글/인기글 댓글 대상과 대상 글 개수(1/3/5/10) UI

Date: 2026-06-05

## Status

Accepted

Depends on:

- [ADR-0011](0011-cafe-article-list-api.md) — 게시글 목록 조회 API(이 UI가 소비).

Related:

- [ADR-0013](0013-comment-distribution-backend.md) — 댓글 분배 백엔드 이전.
  `comment-jobs.ts`를 두고 통합이 필요하다.

## Context

지금까지 댓글 대상은 사용자가 게시글 URL을 직접 붙여넣는 방식(`url`)만
지원했다. "최신글/인기글 중 N개에 댓글"을 쓰려면 카페 게시글 목록을 받아 상위
N개를 대상으로 골라야 한다. [ADR-0011](0011-cafe-article-list-api.md)이 백엔드
`list_cafe_articles`를 제공하지만, 이를 사용자 기능으로 노출하는 UI가 없다.

## Decision

게시 모달(`src/features/posts/publish-modal.tsx`)에 댓글 대상 모드
`latest`/`popular`/`url`을 두고, 최신/인기 선택 시 **대상 글 개수(1/3/5/10)**를
고르는 UI를 추가한다.

- `list_cafe_articles` IPC 래퍼(`listArticles`)로 목록을 조회한다.
- 상위 N개 선택(`topNArticles`)과 댓글 작업 생성(`buildArticleListCommentJobs`)을
  프론트 `comment-jobs.ts`에 둔다. 목록이 N보다 짧으면 있는 만큼만 대상으로
  삼는다(graceful fallback).
- 목록 fetch 실패는 조용히 넘기지 않고 red 알림으로 surface 한다.

## Consequences

- 쉬워지는 것: 사용자가 URL 없이 최신/인기글에 댓글을 달 수 있다. 개수
  선택으로 노출 범위를 조절한다.
- 어려워지는 것 / 위험:
  - top-N 선택·job 생성 로직이 프론트에 위치한다. →
    [ADR-0013](0013-comment-distribution-backend.md)이 댓글 분배를 백엔드로
    옮기면서 `comment-jobs.ts`가 충돌한다. `buildArticleListCommentJobs`도 통일
    IPC(`{targets, comments}`)를 쓰도록 재조정해야 한다.
  - popular은 주간 스코프(ADR-0011)라 글이 적은 카페는 빈 목록 → 개수만큼 채우지
    못할 수 있다.
  - 이 브랜치의 글목록 코드는 #96의 **추정 엔드포인트** 기반이다. #96 실측
    재작성(`7827ff5`) 위로 rebase 하면서 `lastPage` 바인딩 제거를 reconcile 해야
    한다.
- Follow-up: #98과 `comment-jobs.ts` 통합, #96 위로 rebase + 바인딩 reconcile.
- 검증: `comment-jobs.ts`의 top-N/fallback은 vitest, 백엔드 왕복은 #97 예제
  `comment_on_articles`(읽기 전용 DRY-RUN 기본)로 확인한다.
