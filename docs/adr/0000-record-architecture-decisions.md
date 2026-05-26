# 0. Record architecture decisions

Date: 2026-05-26

## Status

Accepted

## Context

The project is starting to accumulate non-trivial cross-cutting decisions
(Trunk-Based Dev policy, hook-based task automation, enforcement rules,
Tauri capability scoping, etc.) that influence future work but are
discoverable today only by reading commit messages, PR descriptions, or
`CLAUDE.md`. As contributors (and future-Claude sessions) join, the
rationale behind these decisions fades.

## Decision

Record architecture-significant decisions as Architecture Decision
Records (ADRs) in `docs/adr/`, using the lightweight Michael Nygard
format (`Status / Context / Decision / Consequences`). Each ADR is
numbered sequentially (`NNNN-kebab-title.md`) and never edited once
Accepted — supersession is recorded by a new ADR pointing back.

`docs/adr/_template.md` is the starting point for new ADRs.

## Consequences

- Future readers (human or AI) can grep `docs/adr/` for the _why_ behind
  the current structure instead of archaeology through git history.
- Adds a tiny cost: one markdown file per genuinely cross-cutting
  decision. The bar is "would the next contributor be confused?".
- ADRs are not for transient or local decisions (those go in code
  comments or PR descriptions).
