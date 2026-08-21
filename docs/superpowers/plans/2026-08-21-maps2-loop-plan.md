# Maps2 Agent Loop — Token-Efficient Long-Running Work Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make 3d-map cheap for long-running agentic loops by isolating per-task context in subagents and enforcing progressive disclosure.

**Architecture:** Controller holds summaries only; each crate/card task runs in isolated subagent (50-100x saving). Bootstrap stays <2.5k tok via AGENTS.md trimmed + tldr docs. Repo-specific `maps2-loop` skill wires dispatching-parallel-agents + subagent-driven-development for this repo's 9 crates and 20 cards.

**Tech Stack:** Markdown (AGENTS.md, CLAUDE.md, docs), JSON (opencode.json:4), Agent skills (SKILL.md with YAML frontmatter name/description), Bash verification (wc -c, cargo test, npm run typecheck)

**Spec:** `docs/superpowers/specs/2026-08-21-maps2-loop-design.md`

## Global Constraints

- Bootstrap budget: AGENTS.md + one targeted file <2.5k tok (`opencode.json:12`) — heuristic 4 chars ≈ 1 tok, so <10k chars.
- Deny-list never read by controller or subagents: `libraries/maps-v2/target/`, `applications/maps-v2-lab/node_modules/`, `public/packages/`, `public/fixtures/`, `dist/`, `*.mt2`, `e2e/*snapshots/` (`opencode.json:14`).
- AGENTS.md must preserve `file_path:line` refs and crate map table (`AGENTS.md:13`).
- Verification: `cargo test --workspace` (libraries/maps-v2) and `npm run typecheck` (applications/maps-v2-lab) as applicable; no rendering goldens for docs/skill changes.
- Keep diffs minimal per `CONTRIBUTING.md:5`; no new Rust/TS logic.
- Token goal: controller per-task delta <500 tok (~2k chars summary).

---

## File Structure

**Modified:**
- `AGENTS.md:1` — trim 4924b → ~3000b, add bootstrap rule block + per-task isolation rule
- `CLAUDE.md:1` — mirror AGENTS.md (currently duplicate 4924b)
- `opencode.json:4` — add 2 delegation instructions
- `libraries/maps-v2/docs/architecture.md:1` — 1-line agent banner pointing to tldr
- `libraries/maps-v2/docs/tile-format.md:1` — 1-line agent banner pointing to tldr

**Created:**
- `.agents/skills/maps2-loop/SKILL.md` — repo-specific skill, YAML frontmatter name=maps2-loop, description triggered on "long loop"/"multiple tasks"/"agentic loop"

**Unchanged but referenced:**
- `libraries/maps-v2/docs/architecture.tldr.md:1` (30 lines, 1.7k) — canonical bootstrap alternative
- `libraries/maps-v2/docs/tile-format.en.tldr.md:1` (40 lines, 1.7k) — canonical bootstrap alternative

---

### Task 1: Harden Progressive Disclosure (AGENTS.md, CLAUDE.md, opencode.json)

**Files:**
- Modify: `AGENTS.md:1`
- Modify: `CLAUDE.md:1`
- Modify: `opencode.json:4`

**Interfaces:**
- Consumes: existing `AGENTS.md:1` content (4924b), `opencode.json:4` 8 instructions, `architecture.tldr.md:1` path, `tile-format.en.tldr.md:1` path
- Produces: trimmed AGENTS.md with bootstrap block consumable by controller in Task 3's skill example prompts

- [ ] **Step 1: Write failing check — measure current bootstrap bloat**

```bash
# In /Users/vadim/projects/3d-map
echo "=== current sizes (chars ≈ tok*4) ==="
wc -c AGENTS.md CLAUDE.md opencode.json libraries/maps-v2/docs/architecture.tldr.md libraries/maps-v2/docs/tile-format.en.tldr.md
echo "--- AGENTS.md line count ---"
wc -l AGENTS.md
echo "--- check for bootstrap rule ---"
grep -c "Bootstrap:" AGENTS.md || echo "MISSING: no Bootstrap rule"
grep -c "maps2-loop" AGENTS.md || echo "MISSING: no maps2-loop ref"
# Expected: MISSING both, AGENTS.md 4924c (>3000 target)
```

- [ ] **Step 2: Run check to verify it fails**

Run: `bash -c 'grep -c "Bootstrap:" AGENTS.md || echo "FAIL: no bootstrap rule"'`
Expected: FAIL with "MISSING" / "FAIL" — confirms rule not present, file >3k.

- [ ] **Step 3: Edit AGENTS.md — trim and add bootstrap block**

At top after `# AGENTS.md — 3D Maps SDK v2`, insert:

```markdown
## Token budget for agents

Bootstrap: `AGENTS.md` + ONE of `architecture.tldr.md:1` / `tile-format.en.tldr.md:1` / grep result. Never read `README.md:1` (20k), `architecture.md:1` (28k), `sdk.ts:1` (28k) fully at bootstrap. Controller <2k tok, subagent <4k. For long loops / multiple tasks: use `maps2-loop` skill — delegate per-crate/card work to subagents, controller keeps summaries only.
```

Then trim verbose sections: keep Stack table, Crate map, Lab map, Commands, MT2 quick ref (condense to 2 lines), Conflation, Manifest, Where-to-edit table verbatim. Remove duplicated prose already in tldr files. Target ~85 → ~65 lines, 4924b → ~3000b. Preserve all `file_path:line` refs per AGENTS.md contract.

- [ ] **Step 4: Mirror to CLAUDE.md**

```bash
cp AGENTS.md CLAUDE.md
```

- [ ] **Step 5: Edit opencode.json — add delegation instructions**

In `opencode.json:4` `instructions` array, append:

```json
"For multi-task or long-loop work: delegate per-crate/card work to isolated subagents via task tool; controller keeps summaries only, never re-reads file subagent summarized.",
"Dispatch reviewer subagent between tasks; do not review diffs inline in controller (burns context)."
```

Keep existing 8 instructions verbatim, add these 2. Validate JSON:

```bash
cat opencode.json | python3 -m json.tool > /dev/null && echo "JSON OK"
```

- [ ] **Step 6: Verify fix**

```bash
wc -c AGENTS.md CLAUDE.md
grep -c "Bootstrap:" AGENTS.md && echo "PASS: bootstrap rule present"
grep -c "maps2-loop" AGENTS.md && echo "PASS: skill ref present"
grep -c "maps2-loop" opencode.json && echo "PASS: opencode delegation present"
# Expect AGENTS.md ~3000c (~750 tok), CLAUDE.md same, bootstrap 750+425=1175 tok <2500
python3 -c "chars=open('AGENTS.md').read().__len__(); print(f'AGENTS.md {chars} chars ~{chars//4} tok')"
```

- [ ] **Step 7: Commit**

```bash
git add AGENTS.md CLAUDE.md opencode.json
git commit -m "feat: harden progressive disclosure, add loop delegation rules"
```

---

### Task 2: Add Agent Banners to Heavy Docs

**Files:**
- Modify: `libraries/maps-v2/docs/architecture.md:1`
- Modify: `libraries/maps-v2/docs/tile-format.md:1`

**Interfaces:**
- Consumes: Task 1's trimmed AGENTS.md bootstrap rule
- Produces: banner lines referenced by skill's progressive disclosure section

- [ ] **Step 1: Write failing check**

```bash
head -5 libraries/maps-v2/docs/architecture.md | grep -c "architecture.tldr" || echo "FAIL: no tldr banner in architecture.md"
head -5 libraries/maps-v2/docs/tile-format.md | grep -c "tile-format.en.tldr" || echo "FAIL: no tldr banner in tile-format.md"
```

Expected: FAIL both.

- [ ] **Step 2: Run check**

Run: `bash -c 'head -5 libraries/maps-v2/docs/architecture.md | grep -c "architecture.tldr" || echo "FAIL"'`
Expected: FAIL

- [ ] **Step 3: Edit architecture.md — add banner after title**

At `libraries/maps-v2/docs/architecture.md:1` after `# Architecture` line, insert:

```markdown
> For agents: start with `architecture.tldr.md` (30 lines, ~425 tok). Read this full doc only on demand via `grep` → `read offset/limit`.
```

- [ ] **Step 4: Edit tile-format.md — add banner**

At `libraries/maps-v2/docs/tile-format.md:1` after first heading, insert:

```markdown
> For agents: start with `tile-format.en.tldr.md` (40 lines, ~425 tok). Read this full doc only on demand via `grep` → `read offset/limit`.
```

- [ ] **Step 5: Verify**

```bash
head -5 libraries/maps-v2/docs/architecture.md
head -5 libraries/maps-v2/docs/tile-format.md
grep -c "For agents" libraries/maps-v2/docs/architecture.md && echo "PASS"
```

- [ ] **Step 6: Commit**

```bash
git add libraries/maps-v2/docs/architecture.md libraries/maps-v2/docs/tile-format.md
git commit -m "docs: add agent tldr banners to heavy docs"
```

---

### Task 3: Create maps2-loop Skill

**Files:**
- Create: `.agents/skills/maps2-loop/SKILL.md`
- Modify: `skills-lock.json:1` (if harness requires registration — check existence, update if needed; otherwise skip)

**Interfaces:**
- Consumes: Task 1's AGENTS.md bootstrap rule and opencode.json delegation instructions; AGENTS.md crate map + where-to-edit table for prompt examples
- Produces: discoverable skill for long-loopcheap execution; no Rust/TS code

- [ ] **Step 1: Write failing check — skill not discoverable**

```bash
test -f .agents/skills/maps2-loop/SKILL.md && echo "EXISTS" || echo "FAIL: skill missing"
grep -r "maps2-loop" .agents/skills/ 2>/dev/null | grep -c "description" || echo "FAIL: not in skill descriptions"
```

Expected: FAIL both.

- [ ] **Step 2: Run check**

Run: `bash -c 'test -f .agents/skills/maps2-loop/SKILL.md && echo PASS || echo FAIL: missing'`
Expected: FAIL: missing

- [ ] **Step 3: Create skill directory and SKILL.md**

```bash
mkdir -p .agents/skills/maps2-loop
```

Create `.agents/skills/maps2-loop/SKILL.md` with:

```markdown
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
```

Keep description short per `writing-skills` trap — body holds workflow.

- [ ] **Step 4: Verify creation and discovery**

```bash
cat .agents/skills/maps2-loop/SKILL.md | head -20
grep -c "^name: maps2-loop" .agents/skills/maps2-loop/SKILL.md && echo "PASS: frontmatter name"
grep -c "description:" .agents/skills/maps2-loop/SKILL.md && echo "PASS: description"
grep -c "When to use" .agents/skills/maps2-loop/SKILL.md && echo "PASS: body"
wc -l .agents/skills/maps2-loop/SKILL.md
# Expect <100 lines per Anthropic best-practices SKILL.md <500 lines
```

- [ ] **Step 5: Verify no placeholder and budget hint**

```bash
grep -E "TBD|TODO|FIXME" .agents/skills/maps2-loop/SKILL.md && echo "FAIL: placeholder" || echo "PASS: no placeholder"
grep -c "Bootstrap" .agents/skills/maps2-loop/SKILL.md && echo "PASS: bootstrap rule"
```

- [ ] **Step 6: Commit**

```bash
git add .agents/skills/maps2-loop/SKILL.md
git commit -m "feat: add maps2-loop skill for token-cheap long loops"
```

---

## Self-Review

- **Spec coverage:** §3.1 (AGENTS.md/opencode.json) → Task 1; §3.1 banners → Task 2; §3.2 skill → Task 3. §4 data flow and §5 error handling are codified in skill body. §6 verification mapped to each task's verify step. No gaps.
- **Placeholder scan:** No TBD/TODO in plan steps; all code blocks contain actual bash/markdown/JSON. Fixed.
- **Type consistency:** No code types to mismatch — docs/skill only. File paths match AGENTS.md crate map verbatim. Skill frontmatter name `maps2-loop` matches AGENTS.md reference and opencode.json delegation.
