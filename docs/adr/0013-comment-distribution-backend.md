# 0013. 계정별 댓글 분배를 백엔드(Rust)로 이전

Date: 2026-06-05

## Status

Accepted

Related:

- [ADR-0012](0012-comment-target-latest-popular-ui.md) — 최신/인기 댓글 UI.
  이 통일 IPC를 재사용할 대상이다.

## Context

UI 안내문은 "계정마다 다른 댓글이 무작위로 게시돼 더 자연스러워요"라고
약속하지만, 실제 `buildUrlCommentJobs`/`buildBothCommentJobs`는 (계정 × 모든
댓글)을 평면 전개해 **모든 계정이 같은 댓글을 같은 순서로** 달았다. 게다가 분배
로직이 프론트(`src/features/posts/comment-jobs.ts`)에 있어, 실제 게시
파이프라인(Rust)과 분리돼 있었다.

## Decision

댓글 분배 책임을 프론트에서 **백엔드(Rust)**로 이전한다.

- 신규 `naver_cafe::distribute`: `mulberry32`(시드 RNG — 프론트 구현을 Rust로 1:1
  포팅) + `shuffle`(Fisher–Yates) + `distribute_comments(count, comments, rng)`.
  댓글 풀을 셔플해 타깃마다 **1개씩** 배정한다.
- `run_comment_jobs` IPC가 분배된 잡 대신
  `CommentDistributionRequest { targets, comments }`를 받아, wall-clock 시드로
  내부에서 분배한 뒤 게시한다.
- `both`/`url` 두 경로를 **단일 IPC**(타깃 목록 + 댓글 풀)로 통일한다.
- 시드는 IPC에 노출하지 않는다(프로덕션은 wall-clock). 결정성은 Rust 단위
  테스트가 시드를 직접 주입해 검증한다.

## Consequences

- 쉬워지는 것:
  - 분배가 게시 파이프라인과 같은 곳에 모여 단일 책임이 된다.
  - `mulberry32` 포팅으로 시드 고정 시 결정적 → 단위 테스트가 가능하다.
  - 통일 IPC라 #97의 최신/인기 댓글도 **타깃만 만들면 동일 분배를 재사용**할 수
    있다.
- 어려워지는 것 / 위험:
  - #97과 `comment-jobs.ts` 충돌: #97의 `buildArticleListCommentJobs`(옛 분배)를
    통일 IPC로 합쳐야 한다.
  - `feat/98`은 master 직분기라 머지 시 rebase가 필요하다.
  - `gen:bindings`가 현 WSL 환경에서 repo 밖에 바인딩을 생성하는 회귀가 있어
    수동 `mv`로 우회했다(근본 수정은 별도).
- Follow-up: #97 `buildArticleListCommentJobs`를 통일 IPC로 통합, ts-rs 경로 회귀
  별도 처리.
- 검증: `distribute` 단위 테스트 10종 + 진단 예제 `distribute_comments`(DRY-RUN
  기본, `--commit` 실게시)로 실서버 E2E까지 확인했다(계정마다 다른 댓글 게시).
