---
name: maps2-loop
description: Use when running multiple long tasks or agentic loops in 3d-map to keep token cost low via subagent isolation.
---

# Maps2 Loop — Token-Efficient Long-Running Work

## When to use

Multiple tasks, long-running, agentic loop, parallel crate/card work, "keep it cheap". For single bounded fix, use normal AGENTS.md flow without this skill.

## Bootstrap (controller)

Read `AGENTS.md` + ONE of `architecture.tldr.md` / `tile-format.en.tldr.md` / grep result. Never read `README.md` (20k) / `architecture.md` (28k) / `sdk.ts` (28k) fully. Deny-list per `opencode.json:14`: `target/`, `node_modules/`, `public/packages/`, `public/fixtures/`, `dist/`, `*.mt2`, `e2e/*snapshots/`. Controller budget <2k tok.

## Dispatch recipe (per task)

For each crate/card, dispatch isolated subagent via `task` with:

- Crate owner + key file from `AGENTS.md:13` crate map (e.g., `maps2-tile lib.rs:48`, `roads-micro` → `maps2-style/lib.rs` + `maps2-render/line.rs:1`)
- Acceptance: `file_path:line` refs, `grep` before `read`, `read` with `offset/limit`, minimal diff, verify via `cargo test --workspace` or `npm run typecheck` per where-to-edit table
- Deny-list reminder + "use `sdk.ts:292` loader for real packages" if relevant

Example prompt:

> Implement Task X: <goal>. Read only `crates/maps2-tile/src/lib.rs:48` + grep results. Emit `file:line` refs. Verify with `cargo test --workspace`. Return summary (≤300 tok) + diff stat + files touched.

## Reviewer recipe

After each subagent returns, dispatch reviewer subagent with diff + AGENTS.md where-to-edit table. Reviewer checks spec compliance then code quality, returns findings only (~200 tok). Controller never reviews inline.

## Controller hygiene

- Keep summaries only; never re-read file subagent summarized. If needed, re-dispatch targeted subagent.
- Parallel dispatch only if file sets disjoint (check crate map). Overlap → sequential.
- If controller >4k tok, summarize and drop older summaries (keep last 3 + task list).

## Anti-patterns

| Thought | Reality |
|---|---|
| "I'll review diff myself" | Burns controller context — dispatch reviewer |
| "Read whole sdk.ts to understand" | Use grep + offset/limit, cost 50x |
| "Read README for context" | Never at bootstrap — violates 2.5k budget |

## Verification

Subagent runs relevant check from AGENTS.md where-to-edit table. Controller runs `git diff --stat` and character count heuristic (`wc -c`, 4 chars≈1 tok) to confirm budget.
