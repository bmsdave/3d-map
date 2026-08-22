# Fix P0/P1/P2 + Local Verify CLI Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix all P0 (version drift, local verify gate), P1 (Map God object, sync decode, frontend lifecycle, Rust invariants), and P2 (lint/size, QA floors, bus factor) while establishing a clone-free `verify` command that agents must run before any PR, all work done in a git worktree with token-efficient boundaries.

**Architecture:** Keep 9-crate DAG `units→camera/tile→style→render→web`; split `maps2-web/src/map.rs:61` into `TileStore`/`Renderer` modules without moving logic; add Worker-eligible CPU bucket path (no `wasm-bindgen-rayon` yet); enhance `scripts/check.sh:1` as single local gate; enforce version single-source `lib.rs:52` `FORMAT_VERSION=6`.

**Tech Stack:** Rust workspace `libraries/maps-v2/Cargo.toml:15` `pedantic`, `miniz_oxide` `heights.rs:140`, `wasm-bindgen 0.2` `maps2-web/Cargo.toml:28`, TS `strict` `tsconfig.json:7` `ES2022`, `vite 8` `playwright 1.62`, `cargo llvm-cov`, `eslint`+`size-limit` (new).

**Spec:** This plan implements the 5-perspective audit `2026-08-22` (Rust 8.5, Frontend 7.2, Arch 7.5, Tech Lead 7.5, QA 8.2) — P0 version drift `lib.rs:52=6` vs `tile-format.md:1=5`, P0 billing-blocked CI `scripts/check.sh:4`, P0 perf unenforced `playwright.perf.config.ts:13`, P1 `map.rs:445` sync decode, `types.ts:7` no teardown, `lib.rs:37` `Zoom::new` `debug_assert`, ingest `unwrap` `world_water.rs:119`, P2 no lint/size `package.json:12`, floored coverage `check-coverage.sh:37`, CODEOWNERS/dependabot missing.

## Global Constraints

- `FORMAT_VERSION = 6` `libraries/maps-v2/crates/maps2-tile/src/lib.rs:52` is single source of truth; all docs must claim `6` and `1..=6` readers; `TILE_EXTENT=65536` `maps2-units/src/lib.rs:18` immutable.
- `MAX_PACKAGE_TILES=50000` `MAX_TILE_BYTES=4MiB` `applications/maps-v2-lab/src/sdk.ts:205-206` and `MAX_ENUMERATED_TILES=50000` `bin/maps2-ingest.rs:410` immutable; `verify-package` `rs:550` checks digests before build.
- Clippy `cargo clippy --workspace --all-targets -- -D warnings` must stay green (`Cargo.toml:15` `pedantic = warn`); `cargo test --workspace` (295 tests) green.
- `sdk.ts:215` `version >=2 && <=6` reader range immutable; never panic on malformed bytes `lib.rs:74` `TileError`.
- Token efficiency: controller <2k, subagent <4k `AGENTS.md:7`; never read `README.md:1`/`architecture.md:1`/`sdk.ts:1` fully at bootstrap; prefer `grep`+`read offset/limit`; never read `target/`/`node_modules/`/`public/packages/`/`dist/`/`*.mt2`/`e2e/*snapshots/`.
- Verification before PR is **mandatory** (local gate replaces billing-blocked CI `scripts/check.sh:4`); failure blocks PR creation per `AGENTS.md`.

---

### Task 0: Isolated Worktree — All Work Happens Here

**Files:**
- Create: `.worktrees/` (project-local, ignored)
- Modify: `.gitignore:1` (ensure `.worktrees/` ignored, commit if needed)

**Interfaces:**
- Consumes: current `main` at `a9e6147`
- Produces: worktree at `.worktrees/fix-p0-p1-p2-verify` on branch `fix/p0-p1-p2-verify-map-hardening`

- [ ] **Step 1: Detect isolation**

```bash
GIT_DIR=$(cd "$(git rev-parse --git-dir)" 2>/dev/null && pwd -P); GIT_COMMON=$(cd "$(git rev-parse --git-common-dir)" 2>/dev/null && pwd -P); echo "$GIT_DIR vs $GIT_COMMON"; git rev-parse --show-superproject-working-tree 2>/dev/null || echo "not submodule"
```

- [ ] **Step 2: Create worktree (fallback, no native tool)**

```bash
git check-ignore -q .worktrees || { echo ".worktrees/" >> .gitignore; git add .gitignore; git commit -m "chore: ignore worktrees"; }
git worktree add .worktrees/fix-p0-p1-p2-verify -b fix/p0-p1-p2-verify-map-hardening
cd .worktrees/fix-p0-p1-p2-verify
```

- [ ] **Step 3: Setup + baseline**

```bash
cargo test --manifest-path libraries/maps-v2/Cargo.toml --workspace --quiet
bash scripts/check.sh rust 2>&1 | tail -20
```

- [ ] **Step 4: Commit**

```bash
git status
```

---

### Task 1: P0 — Fix MT2 Version Single-Source Truth

**Files:**
- Modify: `libraries/maps-v2/crates/maps2-tile/src/lib.rs:11-18` header comment `version (=5)` → `6`
- Modify: `libraries/maps-v2/docs/tile-format.md:1` title `версия 5` → `6`, `:4-6` preamble, `architecture.md:51,105,107,111,113,121,133,173,230`
- Modify: `libraries/maps-v2/docs/tile-format.en.tldr.md:8` `FORMAT_VERSION = 5` → `6`, `:33` add `6:` row, `:41` refs `lib.rs:52`, `README.md:43,404,419`, `libraries/maps-v2/README.md:33,49`, `docs/release-boundary.md:5,7,39`, `production-roadmap.md:26`, `opencode.json:10`, `.opencode/skills/tile-format.md:7`
- Create: `scripts/check-version.sh` (version gate)

**Interfaces:**
- Consumes: `lib.rs:52` `FORMAT_VERSION`
- Produces: `check-version.sh` exits 1 on drift; `Task 2` calls it before `cargo test`

- [ ] **Step 1: Write failing test for gate**

```bash
grep -q "FORMAT_VERSION = 6" libraries/maps-v2/docs/tile-format.en.tldr.md && echo ok || echo FAIL
grep -q "версия 6 — заморожен" libraries/maps-v2/docs/tile-format.md && echo ok || echo FAIL
```

- [ ] **Step 2: Create version gate script**

```bash
# scripts/check-version.sh
#!/usr/bin/env bash
set -euo pipefail
v=$(grep -oE 'pub const FORMAT_VERSION: u16 = [0-9]+' libraries/maps-v2/crates/maps2-tile/src/lib.rs | grep -oE '[0-9]+')
fail=0
grep -q "версия $v — заморожен" libraries/maps-v2/docs/tile-format.md || { echo "tile-format.md title drift"; fail=1; }
grep -q "FORMAT_VERSION = $v" libraries/maps-v2/docs/tile-format.en.tldr.md || { echo "tldr drift"; fail=1; }
[ "$fail" -eq 0 ] || exit 1
echo "version $v consistent"
```

- [ ] **Step 3: Fix all drifted prose (minimal edits, preserve RU where RU)**

- [ ] **Step 4: Verify**

```bash
bash scripts/check-version.sh
cargo test --manifest-path libraries/maps-v2/Cargo.toml --workspace -p maps2-tile --quiet
```

- [ ] **Step 5: Commit**

```bash
git add libraries/maps-v2/crates/maps2-tile/src/lib.rs libraries/maps-v2/docs/tile-format.md libraries/maps-v2/docs/tile-format.en.tldr.md libraries/maps-v2/docs/architecture.md README.md libraries/maps-v2/README.md docs/release-boundary.md libraries/maps-v2/docs/production-roadmap.md scripts/check-version.sh
git commit -m "fix: MT2 version single-source v6 (docs drift)"
```

---

### Task 2: P0 — Local Verify CLI (Replaces Billing-Blocked CI) + PR Requirement

**Files:**
- Modify: `scripts/check.sh:1` — add `--help`, `--quick`, `--perf`, `version` step, `perf` step; add `version_steps()` before `rust_steps`
- Modify: `applications/maps-v2-lab/package.json:10` — add `"verify": "bash ../../scripts/check.sh"`, `"verify:quick": "bash ../../scripts/check.sh --quick"`, `"verify:perf": "bash ../../scripts/check.sh --perf"`
- Modify: `AGENTS.md:37` Commands section — add verify line; add `## Before PR` section
- Modify: `CONTRIBUTING.md:1` — add `Verify before PR` paragraph
- Create: `scripts/install-hooks.sh` (optional pre-push hook)

**Interfaces:**
- Consumes: `scripts/check-version.sh` (Task1), `playwright.perf.config.ts:13`
- Produces: `check.sh --help` prints usage; `check.sh` default = `version+rust+packages+lab`; `--quick` = `version+rust+packages+lab build` (skip e2e); `--perf` adds `playwright perf`; agents MUST run before PR

- [ ] **Step 1: Design CLI contract (read-only prototype)**

```bash
./scripts/check.sh --help  # should print after impl:
# Usage: check.sh [all|rust|packages|lab] [--quick] [--perf] [--help]
#   all (default): version + rust + packages + lab (build+typecheck+e2e)
#   --quick: version + rust + packages + lab build (no e2e)
#   --perf: also run e2e/perf (workers 1, BLOCK_BUDGET 10ms)
```

- [ ] **Step 2: Edit `scripts/check.sh`**

```rust
// Add functions (keep existing step/report):
version_steps() { step "version consistency" bash scripts/check-version.sh; }
perf_steps() { step "playwright perf (port $LAB_PORT)" bash -c "cd '$lab' && npx playwright test --config playwright.perf.config.ts"; }
```

- [ ] **Step 3: Add npm proxies + AGENTS.md requirement**

```markdown
// AGENTS.md add before Commands table:
// ### Before PR (mandatory)
// Run `bash scripts/check.sh` (or `npm run verify` in lab) — it runs version+rust+packages+lab. `--quick` for fast loop, `--perf` before perf-related PR. Do not open PR if any step fails.
```

- [ ] **Step 4: Verify CLI**

```bash
bash scripts/check.sh --help
bash scripts/check.sh --quick 2>&1 | tail -30  # should skip e2e
bash scripts/check.sh 2>&1 | tail -30         # full gate (may take minutes)
```

- [ ] **Step 5: Commit**

```bash
git add scripts/check.sh applications/maps-v2-lab/package.json AGENTS.md CONTRIBUTING.md
git commit -m "feat: local verify CLI replaces CI gate, mandatory before PR"
```

---

### Task 3: P1 — Split `maps2-web/src/map.rs:61` God Object (No Behavior Change)

**Files:**
- Create: `libraries/maps-v2/crates/maps2-web/src/tile_store.rs` — `TileStore { tiles:HashMap, cpu, lines, buildings, names, heights, height_textures, source_levels, building_lod }` + `register_source_level`, `tile_paths`
- Create: `libraries/maps-v2/crates/maps2-web/src/renderer.rs` — `FrameRenderer { gl, programs, terrain, ground, viewport, frame, alpha state }` (extract `draw_ground/buildings/roads/labels`, `height_binding`)
- Modify: `libraries/maps-v2/crates/maps2-web/src/map.rs:61-126` — keep `Map { camera, plan, input, store: TileStore, renderer: FrameRenderer, ... }` delegating; keep `#[wasm_bindgen] impl` surface identical
- Modify: `libraries/maps-v2/crates/maps2-web/src/lib.rs:1` — `mod tile_store; mod renderer;`

**Interfaces:**
- Consumes: `lib.rs:52` `FORMAT_VERSION`, `residency.rs:212` `plan_residency`
- Produces: `TileStore::load_view(view)`, `Renderer::draw_*`; `Map` public JS API unchanged (`map.rs:242` `new`, `load_tile:445`, `render:652`)

- [ ] **Step 1: Write failing extraction test (native)**

```rust
// crates/maps2-web/tests/store_renderer_split.rs
#[test] fn tile_store_holds_tiles_without_gl() { let mut s = TileStore::new(); /* no Gl needed */ }
```

- [ ] **Step 2: Move TileStore (pure HashMaps, no Gl)**

- [ ] **Step 3: Move Renderer (Gl-bound) but keep Map delegating; `cargo test -p maps2-web` green, `wasm-pack` still builds**

- [ ] **Step 4: Verify**

```bash
cargo test --manifest-path libraries/maps-v2/Cargo.toml -p maps2-web --quiet
cargo clippy --manifest-path libraries/maps-v2/Cargo.toml -p maps2-web --all-targets -- -D warnings
cd applications/maps-v2-lab && npm run build --quiet
```

- [ ] **Step 5: Commit**

```bash
git add libraries/maps-v2/crates/maps2-web/src/tile_store.rs libraries/maps-v2/crates/maps2-web/src/renderer.rs libraries/maps-v2/crates/maps2-web/src/map.rs libraries/maps-v2/crates/maps2-web/src/lib.rs
git commit -m "refactor: split Map God object into TileStore/Renderer"
```

---

### Task 4: P1 — Make `load_tile` Worker-Eligible (CPU Off Main Thread)

**Files:**
- Create: `libraries/maps-v2/crates/maps2-web/src/decode.rs` — `decode_tile(bytes: Vec<u8>) -> DecodedTile { fills, buildings, lines, names, height }` (CPU only, no Gl)
- Modify: `libraries/maps-v2/crates/maps2-web/src/map.rs:445` — `load_tile` now calls `decode_tile` + `store.insert` + `renderer.upload`; keep sync path but isolate CPU work for future Worker post
- Modify: `applications/maps-v2-lab/src/sdk.ts:428` — keep `DECODE_SLICE_MS=6` slicing between tiles; add comment `// CPU decode is now in decode.rs, Worker move is step 2`

**Interfaces:**
- Consumes: `TileStore` (Task3), `build_fill_bucket` `lib.rs:79`, `heights.rs:150` `unpack`
- Produces: `decode_tile(bytes) -> Result<DecodedTile, TileError>` — testable without Gl; Task 5 Worker can call it via `Worker.postMessage`

- [ ] **Step 1: Write failing decode test**

```rust
fn decode_tile(bytes: Vec<u8>) -> Result<DecodedTile, TileError> { todo!() }
#[test] fn decode_golden_tile_without_gl() { let bytes = fixture("ridge"); assert!(decode_tile(bytes).is_ok()); }
```

- [ ] **Step 2: Extract CPU buckets + unpack into `decode.rs`**

- [ ] **Step 3: Wire `map.rs:445` to delegate; `upload_gpu` `map.rs:1119` stays on main thread**

- [ ] **Step 4: Verify (perf harness should not regress)**

```bash
cargo test -p maps2-web --quiet
cd applications/maps-v2-lab && npx playwright test --config playwright.perf.config.ts --reporter=list 2>&1 | tail -20
```

- [ ] **Step 5: Commit**

```bash
git add libraries/maps-v2/crates/maps2-web/src/decode.rs libraries/maps-v2/crates/maps2-web/src/map.rs applications/maps-v2-lab/src/sdk.ts
git commit -m "refactor: make load_tile Worker-eligible via decode.rs"
```

---

### Task 5: P1 — Frontend Lifecycle + WASM Types

**Files:**
- Modify: `applications/maps-v2-lab/src/cards/types.ts:7` — `mount(stage,panel): (() => void) | void` or `unmount?: () => void`
- Modify: `applications/maps-v2-lab/src/main.ts:25` `renderCard` + `home.ts:380` `release()` — call `unmount`/`destroy`, cancel `requestAnimationFrame`, lose contexts
- Create: `applications/maps-v2-lab/src/cards/navigation.ts` — `canvasPoint` `src/cards/mapReal.ts:14` + `attachNavigation(target, canvas, map, refresh)` extracted
- Modify: `src/cards/mapReal.ts:14,19`, `globeReal.ts:17,22`, `packageLoader.ts:7,12` — import from `navigation.ts`, delete 135 duplicated lines
- Modify: `applications/maps-v2-lab/package.json:14` — remove `--no-typescript`, regenerate `src/generated/maps2-web/maps2_web.d.ts`; `tsconfig.json:11` keep `skipLibCheck:false` for generated
- Modify: `src/sdk.ts:4` import typed `maps2_web.js` + `sdk.ts:470` `PackageMapApi` cast removed

**Interfaces:**
- Consumes: `CardSpec` (new lifecycle), `MapHandle` `sdk.ts:84`
- Produces: `navigation.ts` `export function canvasPoint`, `attachNavigation`; `types.ts` `mount` returns disposer

- [ ] **Step 1: Failing test for teardown**

```ts
// e2e/input.spec.ts add: navigate Board→card→Board, assert contexts <= LIVE_BUDGET=6 home.ts:16
```

- [ ] **Step 2: Extract navigation, fix CardSpec, fix router/home**

- [ ] **Step 3: Enable WASM types**

```bash
npm run build:wasm  # now generates .d.ts
npx tsc --noEmit   # should still pass
```

- [ ] **Step 4: Verify**

```bash
cd applications/maps-v2-lab && npm run typecheck && npm run build --quiet && npx playwright test --grep input --quiet
```

- [ ] **Step 5: Commit**

```bash
git add src/cards/types.ts src/cards/navigation.ts src/cards/mapReal.ts src/cards/globeReal.ts src/cards/packageLoader.ts src/main.ts src/home.ts src/sdk.ts applications/maps-v2-lab/package.json tsconfig.json
git commit -m "fix: CardSpec teardown, dedupe navigation, typed wasm"
```

---

### Task 6: P1 — Harden Rust Invariants (Zoom, unwrap, Fuzz)

**Files:**
- Modify: `libraries/maps-v2/crates/maps2-units/src/lib.rs:37` `Zoom::new` → `Option`+`new_unchecked`, add `level ≤22` assert in `locate:137`; `maps2-camera/src/lib.rs:156` `clamp_zoom` keeps fast path
- Modify: `libraries/maps-v2/crates/maps2-ingest/src/world_water.rs:119,140,157` + `natural_earth.rs:316` `unwrap()` → `expect("static field 'id'")`; enable `clippy::unwrap_used` deny for `maps2-ingest`
- Modify: `libraries/maps-v2/Cargo.toml:15` — add `[workspace.dependencies]` for `num-traits`, `sha2`, `miniz_oxide`, `wasm-bindgen` + change crates to `workspace=true`
- Create: `libraries/maps-v2/crates/maps2-tile/fuzz/fuzz_targets/parse.rs` — `fuzz_target!(|data:&[u8]| { let _ = TileView::parse(data); })` + `cargo fuzz` job (1 min in `check.sh --fuzz` optional)

**Interfaces:**
- Consumes: `Zoom`, `TileView::parse` `view.rs:31` never-panic contract `lib.rs:74`
- Produces: `Zoom::new_checked`, `workspace.dependencies` single pins

- [ ] **Step 1: Failing invariant test**

```rust
#[test] fn zoom_nan_rejected() { assert!(Zoom::new(f64::NAN).is_none()); }
#[test] fn locate_level_overflow_panics_or_clamps() { let _ = locate(point, 64); }
```

- [ ] **Step 2: Implement checked Zoom + centralize deps + expect**

- [ ] **Step 3: Add fuzz target (no CI yet, local only)**

- [ ] **Step 4: Verify**

```bash
cargo test -p maps2-units --quiet
cargo clippy -p maps2-ingest -- -D clippy::unwrap_used --quiet
cargo test --workspace --quiet
```

- [ ] **Step 5: Commit**

```bash
git add libraries/maps-v2/crates/maps2-units/src/lib.rs libraries/maps-v2/crates/maps2-ingest/src/world_water.rs libraries/maps-v2/Cargo.toml libraries/maps-v2/crates/*/Cargo.toml
git commit -m "fix: harden Zoom invariants, centralize deps, replace unwraps"
```

---

### Task 7: P2 — Lint + Size + QA Floors

**Files:**
- Create: `applications/maps-v2-lab/eslint.config.js`, `.prettierrc`, `size-limit.config.js` (wasm <400KiB)
- Modify: `applications/maps-v2-lab/package.json:12` — add `lint`, `format`, `size`, wire into `verify` (`npm run lint && npm run size`)
- Modify: `scripts/check-coverage.sh:37` — raise `maps2` floors where already 97→98, add `lab` floors enforcement in `scripts/check.sh:58` `check-coverage.sh lab` (currently only maps2); set `ingest:70→75`, `web:42→50`
- Modify: `e2e/labels.spec.ts:38` etc — document snapshot `-darwin` vs Linux, consider `maxDiffPixelRatio 0.01` stay

**Interfaces:**
- Consumes: `check.sh` verify pipeline
- Produces: `npm run lint` zero warnings, `size-limit` <400KiB wasm

- [ ] **Step 1: Write size budget test**

```bash
npx size-limit  # should fail if wasm >400KiB before fix, pass after dedupe
```

- [ ] **Step 2: Add configs, wire into check.sh lab_steps after `npm run build`**

- [ ] **Step 3: Verify**

```bash
cd applications/maps-v2-lab && npm run lint && npm run typecheck && npm run build --quiet
```

- [ ] **Step 4: Commit**

```bash
git add applications/maps-v2-lab/eslint.config.js .prettierrc size-limit.config.js scripts/check.sh
git commit -m "chore: lint + size budget, raise QA floors"
```

---

### Task 8: P2 — Bus Factor + Docs + Eviction

**Files:**
- Create: `.github/CODEOWNERS`, `.github/dependabot.yml` (cargo+npm)
- Modify: `.gitignore:1` add `.agents/ .opencode/ skills-lock.json coverage/` (today `git status` polluted with 12-commit planet work)
- Modify: `libraries/maps-v2/docs/implementation-plan.md:1` update status to v6/planet or archive to `docs/superpowers/plans/`; translate `ci.yml:45`/`pages.yml:42` RU comments to EN
- Modify: `libraries/maps-v2/crates/maps2-web/src/map.rs:74` `tiles:HashMap` — add `evict()` pressure check (least-recently-drawn) beyond `residency.rs:267`; add `tileStore.evictIfOver(50000)` guard

**Interfaces:**
- Consumes: all prior tasks
- Produces: repo hygiene, eviction prevents leak on long session without `unload_tile`

- [ ] **Step 1: Add CODEOWNERS + dependabot**

- [ ] **Step 2: Clean working tree, update stale docs**

- [ ] **Step 3: Add memory-pressure eviction (low-risk, feature-flagged)**

- [ ] **Step 4: Verify**

```bash
git status --porcelain # should be clean after task
bash scripts/check.sh --quick --quiet
```

- [ ] **Step 5: Commit**

```bash
git add .github/CODEOWNERS .github/dependabot.yml .gitignore libraries/maps-v2/docs/implementation-plan.md libraries/maps-v2/crates/maps2-web/src/map.rs
git commit -m "chore: bus factor, docs, memory-pressure eviction"
```

