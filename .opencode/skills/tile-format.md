# Skill: tile-format

Change MT2 tile format (layout change requires version bump).

Steps:
1. Read `libraries/maps-v2/docs/tile-format.en.tldr.md` (not full RU doc) + `crates/maps2-tile/src/lib.rs:48` constants.
2. Bump `FORMAT_VERSION` (current 5) and add entry to version history table.
3. Update writer in `crates/maps2-tile/src/build.rs` + reader in `view.rs` / `lib.rs:74` to handle fallback (e.g. fill Unknown for old).
4. Update `libraries/maps-v2/docs/tile-format.md` (RU) — add to history table.
5. Regenerate fixtures: `cargo run -p gen-fixtures` via `applications/maps-v2-lab: npm run build:fixtures`, then update golden hash in `crates/maps2-fixtures/src/lib.rs:382` intentionally.
6. Verify: `cd libraries/maps-v2 && cargo test --workspace` — golden test will fail until hash updated.
