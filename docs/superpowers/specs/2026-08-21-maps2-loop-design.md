# Maps2 Agent Loop — Token-Efficient Long-Running Work (Design)

**Date:** 2026-08-21
**Status:** Approved (bounded → upgraded to architectural per hidden complexity)
**Goal:** Run multiple long-running tasks / agentic loops in `3d-map` while keeping token cost cheap. Target: coding agents (OpenCode, Muse, Cursor) working *in* this repo, not LLM consumers of the map runtime.
**Approach:** 2+1 — subagent-isolated loops (primary) + progressive disclosure hardening (supporting).

## 1. Context & Problem

`3d-map` is a Rust workspace (`libraries/maps-v2`, 9 crates) + browser lab (`applications/maps-v2-lab`, TS+Wasm). `AGENTS.md:1` is 4924b (~1.2k tok) and `opencode.json:12` already declares a 2.5k token bootstrap budget (AGENTS.md + one targeted file). `architecture.tldr.md:1` (1.7k, 30 lines) and `tile-format.en.tldr.md:1` (1.7k) exist as token-efficient alternatives to `architecture.md:1` (28k) / `tile-format.md:1` (8.9k) / `README.md:1` (20k) / `sdk.ts:1` (28k).

Pain: the budget is declared but not enforced. Long loops accumulate context — each iteration's file reads stay in history, parallel tasks pollute each other, and a single greedily-read `README.md` or `sdk.ts` blows the bootstrap 10x. `writing-skills/SKILL.md:236` claims subagents give 50-100x savings, but no repo-specific skill wires that for `3d-map`'s crate/card map. `.agents/skills/` contains generic superpowers skills; no `maps2-*` skill.

Success metric: controller bootstrap <2.5k tok; per-task controller delta <500 tok (summary only); subagent contexts isolated and discarded; parallel tasks do not share file ownership without serialization.

## 2. Architecture

### 2.1 Controller + isolated subagents

Controller is long-lived, cheap. It holds task list, summaries, and diffs. It never reads denied paths (`opencode.json:14`: `target/`, `node_modules/`, `public/packages/`, `public/fixtures/`, `dist/`, `*.mt2`, `e2e/*snapshots/`). Work is delegated:

```
Controller (2k tok) ──grep──► locate slice
   │
   ├──► Subagent A (isolated, 4k tok max): reads crate/card slice, writes, returns summary+diff
   ├──► Subagent B (isolated, parallel if disjoint files)
   └──► Reviewer subagent (isolated): checks diff vs AGENTS.md where-to-edit table, returns findings
```

Isolation guarantee: subagent context is constructed explicitly (target files + acceptance criteria), never inherits controller history. After return, subagent context is discarded; controller retains only summary. This is `subagent-driven-development` + `dispatching-parallel-agents` as already vendored.

### 2.2 Why not other shapes

- Docs-only (approach 1) solves bootstrap but not accumulation.
- Metering orchestrator (approach 3) is correct for 10h+ world-scale loops but overkill for 9 crates / 20 cards; heuristic token counting is fragile. Defer to v2 if loops exceed ~50 iterations or need persistence.

## 3. Components

### 3.1 Progressive disclosure hardening (edits)

| File | Change | Rationale |
|---|---|---|
| `AGENTS.md:1` | Trim 4924b → ~3000b. Add explicit bootstrap rule block at top: "Bootstrap: AGENTS.md + ONE of architecture.tldr.md / tile-format.en.tldr.md / grep result. Never read README.md / architecture.md / sdk.ts fully at bootstrap. Budget: controller <2k, subagent <4k. For long loops use maps2-loop skill." Add "Per-task isolation: controller keeps summaries, subagent holds file reads." | Makes 2.5k budget actionable, not aspirational. |
| `opencode.json:4` | Add 2 instructions: (a) "For multi-task or long-loop work, delegate per-crate/card work to subagents via task tool; controller keeps summaries only." (b) "Dispatch reviewer subagent between tasks; do not review diffs inline in controller." | Wires `task` tool into instruction layer so agents don't inline reviews (burns context per `requesting-code-review/SKILL.md:79`). |
| `libraries/maps-v2/docs/architecture.md:1` | Add 1-line banner: "Agents: start with architecture.tldr.md, read full doc only on demand via grep." | Zero churn, guides progressive disclosure. |
| `libraries/maps-v2/docs/tile-format.md:1` | Same banner pointing to `tile-format.en.tldr.md`. | Same. |
| `CLAUDE.md:1` | Mirror `AGENTS.md` changes (currently duplicate). | Keeps Codex/Cursor/Muse in sync. |

No changes to `sdk.ts:441`, `maps2-*` crate code, or fixtures. Rendering goldens unaffected.

### 3.2 Repo-specific skill (new)

**Path:** `.agents/skills/maps2-loop/SKILL.md` (cross-runtime via `.agents/skills/`, also symlinked or duplicated under `.opencode/skills/` if harness requires it — check `skills-lock.json:1` sync).

**Frontmatter:**

```yaml
---
name: maps2-loop
description: Use when running multiple long tasks or agentic loops in 3d-map to keep token cost low via subagent isolation.
---
```

Keep `description` short per `writing-skills/SKILL.md:154` trap — workflow detail belongs in body, not description, so agents must read the file.

**Body (~80 lines, progressive disclosure):**

- **When to use:** "multiple tasks", "long-running", "agentic loop", "cheap", "parallel tasks" — contrasts with single bounded fix (use normal flow).
- **Bootstrap rule:** controller reads AGENTS.md + tldr/grep only; quote deny-list from `opencode.json:14`.
- **Dispatch recipe:** for each task, craft subagent prompt with: crate/card owner from AGENTS.md crate map + key file + acceptance criteria + "read only this slice; use grep before read; emit file_path:line refs; verify with cargo test --workspace or npm run typecheck as per AGENTS.md where-to-edit table". Include example prompt for `maps2-tile` vs `roads-micro` card.
- **Reviewer recipe:** after each task, dispatch reviewer subagent with diff + context; controller consumes findings only.
- **Controller hygiene:** never re-read file subagent summarized; if needed, re-dispatch targeted subagent; parallel dispatch only if file sets disjoint (check crate map).
- **Anti-patterns table:** "inline review burns controller context" / "broad glob burns 50x" / "reading README.md at bootstrap violates budget".
- **Verification:** subagent runs `cargo test --workspace` (Rust) or `npm run typecheck` (lab) as relevant; controller checks `git diff --stat`.

Reference `writing-skills/anthropic-best-practices.md:237` structure — SKILL.md is table of contents, not exhaustive.

### 3.3 Optional helper (deferred)

`scripts/agent-loop.sh` — queue runner `task list → task dispatches → aggregate summaries`. Defer unless skill alone proves insufficient; YAGNI. If added, it must respect deny-list and call `task` tool, not inline reads.

## 4. Data Flow (per iteration)

1. Controller bootstrap: `read AGENTS.md` + `read architecture.tldr.md` OR `grep` for slice — <2.5k tok. No `README.md`/`architecture.md`/`sdk.ts` full reads.
2. Controller `grep` to locate crate/card slice (e.g., `maps2-render/building.rs` for 3D buildings per AGENTS.md where-to-edit).
3. Controller `task` dispatch: subagent prompt = task goal + exact file slice + acceptance criteria + deny-list reminder.
4. Subagent: `grep` → `read` offset/limit → edit → `bash cargo test --workspace` subset → returns summary (~300 tok) + diff stat.
5. Controller optionally dispatches reviewer subagent (diff → findings ~200 tok).
6. Controller merges summary, discards subagent context, proceeds to next task. Accumulation = O(summaries), not O(file contents).

Token accounting (heuristic 4 chars ≈ 1 tok): AGENTS.md 4924b → ~1230 tok → trimmed 3000b → ~750 tok. tldr 1700b → ~425 tok. Bootstrap 750+425=1175 tok (<2.5k). Per-task controller delta 300-500 tok. 20 tasks → controller ~10k tok total vs ~200k if files inlined — ~20x saving.

## 5. Error Handling

- **Subagent reads denied path:** deny-list in `opencode.json:14` + skill reminder; reviewer catches violations; if caught, controller re-dispatches with narrowed prompt.
- **Context creep in controller:** rule "never re-read summarized file; re-dispatch targeted subagent". If controller exceeds 4k tok, it must summarize and drop older summaries (keep last 3 + task list).
- **Parallel file collision:** controller checks AGENTS.md crate map / card ownership before parallel dispatch; overlapping file sets → sequential dispatch.
- **Subagent failure:** reviewer reports failure reason; controller re-dispatches with fix hint or escalates with enlarged context (single retry). No silent swallowing.
- **Skill not discovered:** `description` contains high-recall keywords ("long tasks", "agentic loop", "token"); `AGENTS.md` explicitly references `maps2-loop` skill by name.

## 6. Testing & Verification

- **Baseline (before):** run one multi-task loop without skill (inline reads). Measure bootstrap chars and per-iteration controller growth via `wc -c` heuristic. Record that reading `README.md`/`architecture.md` blows budget.
- **After (with skill):** same tasks via `maps2-loop` dispatch. Verify: bootstrap <2.5k tok (AGENTS.md trimmed + tldr), per-task delta <500 tok, deny-list respected (no reads of `target/` etc.), parallel dispatch isolated.
- **Functional:** `cargo test --workspace` (libraries/maps-v2) and `npm run typecheck` (applications/maps-v2-lab) inside subagents as per AGENTS.md where-to-edit. No visual goldens — docs/skill change only.
- **Self-review of spec:** scanned for TBD/TODO — none. Scope is single spec/plan cycle (skill + 2 doc edits). No contradictions with `architecture.md:20` build vs frame split or `tile-format.md` invariants.

## 7. Alternatives Considered

See §2.2. Docs-only is insufficient for accumulation; full metering orchestrator is deferred as over-engineered for current 9-crate scope.

## 8. Implementation Plan Reference

Next step: invoke `writing-plans` skill to produce `docs/superpowers/plans/2026-08-21-maps2-loop-plan.md` with tasks: (1) trim AGENTS.md/CLAUDE.md + opencode.json, (2) add architecture.md/tile-format.md banners, (3) create maps2-loop skill, (4) verify via baseline vs after measurement + cargo test/typecheck.

## 9. Open Questions (none blocking)

- Whether `.opencode/skills/` needs duplicate of `.agents/skills/maps2-loop/` for OpenCode discovery — verify harness skill resolution (check `skills-lock.json:1` and `opencode.json:1` `instructions`).
- Exact trimmed AGENTS.md wording — preserve crate map line refs per `AGENTS.md:13` verbatim.
