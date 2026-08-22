# Skill: fix-render

Fix a rendering bug (roads, buildings, terrain, globe, labels) with token efficiency.

Steps:
1. Read AGENTS.md row for your area (e.g. Road width/join → maps2-style/lib.rs + maps2-render/line.rs:1).
2. `grep` for symbol, then `read` only that file with offset/limit.
3. Write minimal fix. Do not read full architecture.md — use architecture.tldr.md if needed.
4. Verify:
   - `cd libraries/maps-v2 && cargo test --workspace` (or clippy for units)
   - `cd applications/maps-v2-lab && npm run typecheck`
   - If visual: check e2e spec in `e2e/<area>.spec.ts` and plan golden update.
5. Output must include `file_path:line` refs and verification command results.
