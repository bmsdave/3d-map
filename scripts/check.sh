#!/usr/bin/env bash
# The gate. Runs what CI used to run, in the order CI ran it.
#
# This project has no working CI — the account's Actions are billing-blocked —
# so "it passed" means someone ran this script and watched it finish. Every
# phase of the planet-package work ends here.
#
#   check.sh            — everything, in order, stopping at the first failure
#   check.sh rust       — the Rust workspace only (tests, clippy, coverage)
#   check.sh lab        — the browser lab only (build, typecheck, e2e)
#   check.sh packages   — tile package digests only
#
# Stopping at the first failure is deliberate: a clippy error usually means the
# e2e run below it would only be testing a build that should not exist yet.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
maps2="${root}/libraries/maps-v2/Cargo.toml"
lab="${root}/applications/maps-v2-lab"
area="${1:-all}"

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

rust_steps() {
  step "cargo test (maps-v2 workspace)" \
    cargo test --manifest-path "${maps2}" --workspace
  step "cargo clippy (-D warnings)" \
    cargo clippy --manifest-path "${maps2}" --workspace --all-targets -- -D warnings
  # The coverage ratchet is its own script and its own argument for existing;
  # this only makes sure nobody forgets to run it.
  step "coverage ratchet (maps2)" \
    "${root}/scripts/check-coverage.sh" maps2
}

package_steps() {
  # The packages are committed, so a corrupt tile would be a corrupt commit —
  # silently a hole in the map rather than a failure. Digests are checked before
  # anything builds against them.
  for package in trafalgar trafalgar-city; do
    step "verify-package ${package}" \
      cargo run --quiet --manifest-path "${maps2}" --bin maps2-ingest -- \
        verify-package "${lab}/public/packages/${package}"
  done
}

# A port of the gate's own. Playwright reuses an already-running dev server,
# and the one a person has open in a browser is being rebuilt by the step
# above — which reloads the page mid-test and fails something unrelated. The
# gate gets its own port so it never races the lab you are looking at.
export LAB_PORT="${LAB_PORT:-5188}"

lab_steps() {
  # `npm run build` is wasm-pack, the fixture generator, tsc and vite in one —
  # so a Rust change that breaks the wasm boundary fails here, not in the e2e
  # run twenty minutes later.
  step "lab build (wasm + fixtures + typecheck + vite)" \
    bash -c "cd '${lab}' && npm run build"
  step "playwright e2e (port ${LAB_PORT})" \
    bash -c "cd '${lab}' && npx playwright test"
}

case "${area}" in
  all)      rust_steps; package_steps; lab_steps ;;
  rust)     rust_steps ;;
  packages) package_steps ;;
  lab)      lab_steps ;;
  *)
    echo "unknown area: ${area} (want: all|rust|packages|lab)" >&2
    exit 2
    ;;
esac

report
printf '\n\033[32mall green\033[0m in %ds\n' "$(( $(date +%s) - started_all ))"
