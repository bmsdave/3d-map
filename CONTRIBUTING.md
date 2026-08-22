# Contributing

Contributions must preserve deterministic fixtures and the alpha release
boundary. For behavior changes, add a focused failing test first, then make the
smallest implementation and run the relevant Rust and browser checks. Rendering
changes need an intentional, visually reviewed golden update.

Before opening a PR, run `bash scripts/check.sh` (or `npm run verify` in the lab) and ensure all steps are green. Use `--quick` for fast loops during development, `--perf` for perf-sensitive changes. No PR without a green verify gate — see `AGENTS.md:Before PR` and `scripts/check.sh --help`.

Do not commit real-world derived data, downloaded OSM/DEM inputs, credentials,
or large unrelated artifacts. Discuss API, MT2 format, and data-pipeline changes
in an issue before implementation.
