# Skill: verify

Verify completion before claiming success. Run smallest relevant checks.

Rust (from `libraries/maps-v2`):
- `cargo test --workspace` — all 9 crates + golden hashes
- `cargo clippy -p maps2-units --all-targets -- -D warnings` — style

Lab (from `applications/maps-v2-lab`):
- `npm run typecheck` — tsc
- `npm run build` — wasm-pack + fixtures + vite
- `npx playwright test e2e/<spec>.spec.ts` — visual (needs chromium)

Ingest:
- `cargo run -p maps2-ingest -- verify-package <dir>` — after any package build/carve

Report actual command output. Do not claim pass without evidence.
