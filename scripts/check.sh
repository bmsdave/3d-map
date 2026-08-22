#!/usr/bin/env bash
# The gate. Runs what CI used to run, in the order CI ran it.
#
# This project has no working CI — the account's Actions are billing-blocked —
# so "it passed" means someone ran this script and watched it finish. Every
# phase of the planet-package work ends here.
# BEFORE ANY PR: run `bash scripts/check.sh` (or `npm run verify` in lab).
# The verify gate is mandatory — see AGENTS.md "Before PR".
#
#   check.sh            — everything (version + rust + packages + lab)
#   check.sh --quick    — version + rust + packages + lab build (no e2e, fast loop)
#   check.sh --perf     — everything + perf suite (workers=1, 10ms budget)
#   check.sh rust       — Rust workspace only (tests, clippy, coverage)
#   check.sh lab        — browser lab only (build, typecheck, e2e)
#   check.sh packages   — tile package digests only
#   check.sh version    — MT2 version consistency only
#   check.sh --help     — usage
#
# Flags can be combined: `check.sh lab --quick`, `check.sh all --perf`
#
# Stopping at the first failure is deliberate: a clippy error usually means the
# e2e run below it would only be testing a build that should not exist yet.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
maps2="${root}/libraries/maps-v2/Cargo.toml"
lab="${root}/applications/maps-v2-lab"
area="all"
QUICK=0
PERF=0

usage() {
  cat <<'USAGE'
Usage: check.sh [all|rust|packages|lab|version] [--quick] [--perf] [--help]
  all (default): version + rust + packages + lab (build+typecheck+e2e)
  rust:         cargo test + clippy + coverage ratchet
  packages:     verify-package for trafalgar + trafalgar-city
  lab:          lab build + e2e (add --quick to skip e2e)
  version:      MT2 version consistency only
  --quick:     skip e2e (fast feedback loop)
  --perf:      also run perf suite (playwright.perf.config.ts, workers=1)
  --help:      this message
Examples:
  bash scripts/check.sh              # full gate before PR
  bash scripts/check.sh --quick      # fast loop during dev
  bash scripts/check.sh lab --quick  # only lab build
  bash scripts/check.sh --perf       # full + perf (before perf PRs)
USAGE
}

# Parse args: area is first non-flag, flags can follow in any order
for arg in "$@"; do
  case "$arg" in
    -h|--help) usage; exit 0 ;;
    --quick) QUICK=1 ;;
    --perf) PERF=1 ;;
    all|rust|packages|lab|version) area="$arg" ;;
    *) echo "unknown arg: $arg (want: all|rust|packages|lab|version|--quick|--perf|--help)" >&2; usage >&2; exit 2 ;;
  esac
done

started_all="$(date +%s)"
summary=()

# Each step announces itself before it runs, because half of these take minutes
# and a silent terminal is indistinguishable from a hung one.
step() {
  local name="$1"
  shift
  local started
  started="$(date +%s)"
  printf '\n\033[1m▶ %s\033[0m\n' "${name}"
  if "$@"; then
    local spent=$(( $(date +%s) - started ))
    summary+=("ok    ${name} (${spent}s)")
  else
    local status=$?
    summary+=("FAIL  ${name}")
    report
    printf '\n\033[31m%s failed (exit %d)\033[0m — fix this before the steps below it mean anything.\n' \
      "${name}" "${status}"
    exit "${status}"
  fi
}

report() {
  printf '\n\033[1m── summary ─────────────────────────────\033[0m\n'
  printf '%s\n' "${summary[@]}"
}

version_steps() {
  step "version consistency (MT2 v6)" \
    bash "${root}/scripts/check-version.sh"
}

rust_steps() {
  step "cargo test (maps-v2 workspace)" \
    cargo test --manifest-path "${maps2}" --workspace
  step "cargo clippy (-D warnings)" \
    cargo clippy --manifest-path "${maps2}" --workspace --all-targets -- -D warnings
  step "coverage ratchet (maps2)" \
    "${root}/scripts/check-coverage.sh" maps2
}

package_steps() {
  for package in trafalgar trafalgar-city; do
    step "verify-package ${package}" \
      cargo run --quiet --manifest-path "${maps2}" --bin maps2-ingest -- \
        verify-package "${lab}/public/packages/${package}"
  done
}

export LAB_PORT="${LAB_PORT:-5188}"

lab_steps() {
  step "lab build (wasm + fixtures + typecheck + vite)" \
    bash -c "cd '${lab}' && npm run build"
  if [ "$QUICK" -eq 1 ]; then
    printf '\n\033[2m… skipping e2e (--quick)\033[0m\n'
  else
    step "playwright e2e (port ${LAB_PORT})" \
      bash -c "cd '${lab}' && npx playwright test"
  fi
}

perf_steps() {
  step "playwright perf (port ${LAB_PORT}, workers=1)" \
    bash -c "cd '${lab}' && npx playwright test --config playwright.perf.config.ts"
}

case "${area}" in
  all)      version_steps; rust_steps; package_steps; lab_steps; [ "$PERF" -eq 0 ] || perf_steps ;;
  rust)     version_steps; rust_steps ;;
  packages) package_steps ;;
  lab)      lab_steps; [ "$PERF" -eq 0 ] || perf_steps ;;
  version)  version_steps ;;
  *)
    echo "unknown area: ${area} (want: all|rust|packages|lab|version)" >&2
    exit 2
    ;;
esac

report
printf '\n\033[32mall green\033[0m in %ds\n' "$(( $(date +%s) - started_all ))"
