#!/usr/bin/env bash
# Fails when a file's line coverage drops below the floor recorded for it.
#
# The floors below are where each file stands today, not where it should stand.
# Targets live in AGENTS.md and most files are short of them; a gate set at the
# target would fail on the first run and be switched off by the afternoon. This
# is a ratchet instead: it cannot go down, and raising a floor after writing
# tests is how it goes up.
#
#   check-coverage.sh maps     — the historical v1 SDK workspace
#   check-coverage.sh maps2    — the v2 alpha SDK workspace
#   check-coverage.sh ingest   — the pipeline
#   check-coverage.sh web      — the demo's host bridge
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
area="${1:?usage: check-coverage.sh maps|maps2|ingest|web}"

# file-name-fragment:minimum-line-percentage
maps_floors=(
  "map-core/src/globe.rs:100"
  "map-core/src/lib.rs:93"
  "map-core/src/mercator.rs:95"
  "map-core/src/projection.rs:98"
  "map-ffi/src/lib.rs:80"
  "map-mesh/src/lib.rs:97"
  "map-runtime/src/lib.rs:90"
  "map-style/src/lib.rs:90"
  "map-world/src/lib.rs:91"
  # Boundaries no unit test can reach: map-jni needs a JVM, map-render-webgl and
  # map-web need a browser with a GL context. They are exercised by the host
  # suites and by Playwright, and counted there rather than pretended at here.
  # map-web is not entirely boundary — where it decides something, such as where
  # a controller opens, that part is tested natively — but the file as a whole
  # is still mostly GL, so flooring it would floor the browser.
)
maps2_floors=(
  "maps2-camera/src/lib.rs:97"
  "maps2-fixtures/src/lib.rs:100"
  "maps2-fixtures/src/ridge.rs:99"
  "maps2-fixtures/src/roads.rs:97"
  "maps2-render/src/globe.rs:99"
  "maps2-render/src/labels.rs:100"
  "maps2-render/src/lib.rs:99"
  "maps2-render/src/line.rs:99"
  "maps2-render/src/residency.rs:97"
  "maps2-render/src/terrain.rs:100"
  "maps2-render/src/triangulate.rs:97"
  "maps2-style/src/lib.rs:91"
  "maps2-style/src/relief.rs:97"
  "maps2-text/src/atlas.rs:98"
  "maps2-text/src/collision.rs:99"
  "maps2-text/src/font.rs:99"
  "maps2-text/src/layout.rs:100"
  "maps2-tile/src/build.rs:100"
  "maps2-tile/src/heights.rs:94"
  "maps2-tile/src/lib.rs:97"
  "maps2-tile/src/varint.rs:96"
  "maps2-tile/src/view.rs:98"
  "maps2-units/src/lib.rs:91"
  "maps2-web/src/input.rs:94"
  "maps2-web/src/labels.rs:98"
  "maps2-web/src/transform.rs:100"
  # WebGL bridge files require a browser context and are covered by Playwright.
  # Binary generators are shell boundaries; their libraries are floored above.
)
ingest_floors=(
  "clip.rs:98"
  "bands.rs:98"
  "conversion.rs:70"
  "geometry.rs:94"
  "package.rs:94"
  "simplify.rs:98"
  "adapter.rs:75"
  "lib.rs:79"
  # main.rs is argument parsing around these; the pipeline is exercised end to
  # end through the library, and a floor on the shell would only be theatre.
)
web_floors=(
  "map-core-bridge.mjs:58"
  "tile-store.mjs:42"
  "demo-layers.mjs:100"
  "error-reporter.mjs:59"
  "map-gestures.mjs:95"
  "frame-stats.mjs:95"
  # use-map-input.mjs has no floor on purpose: it is a React hook, so a unit
  # test cannot load it and c8 never sees it. Its gate is the e2e suite, which
  # drives every gesture it wires up.
)
lab_floors=(
  "sdk.ts:92"
  "home.ts:86"
  "main.ts:64"
  "ui.ts:93"
  "bands.ts:100"
)

failed=0

# Reads "<name> ... <lines> <missed> <cover>%" rows and checks the ones we floor.
check_against() {
  local report="$1"
  local cover_field="$2"
  shift 2
  for floor in "$@"; do
    local file="${floor%%:*}"
    local minimum="${floor##*:}"
    local actual
    local row
    row="$(grep -F -- "${file}" "${report}" | head -1)"
    if [ "${cover_field}" = "llvm" ]; then
      # Regions, functions, then lines: three percentages, lines last.
      actual="$(printf '%s\n' "${row}" | grep -oE '[0-9]+\.[0-9]+%' | sed -n 3p | tr -d '%')"
    else
      # c8 draws a table: file | % stmts | % branch | % funcs | % lines
      actual="$(printf '%s\n' "${row}" | awk -F '|' '{gsub(/ /, "", $5); print $5}')"
    fi
    if [ -z "${actual}" ]; then
      echo "missing: ${file} is floored at ${minimum}% but the report does not mention it"
      failed=$((failed + 1))
      continue
    fi
    if [ "${actual%%.*}" -lt "${minimum}" ]; then
      echo "below floor: ${file} at ${actual}%, floor is ${minimum}%"
      failed=$((failed + 1))
    else
      echo "ok: ${file} at ${actual}% (floor ${minimum}%)"
    fi
  done
}

report="$(mktemp)"
trap 'rm -f "${report}"' EXIT

case "${area}" in
  maps)
    cargo llvm-cov --manifest-path "${root}/libraries/maps/Cargo.toml" --workspace \
      --summary-only > "${report}"
    check_against "${report}" llvm "${maps_floors[@]}"
    ;;
  maps2)
    cargo llvm-cov --manifest-path "${root}/libraries/maps-v2/Cargo.toml" --workspace \
      --summary-only > "${report}"
    check_against "${report}" llvm "${maps2_floors[@]}"
    ;;
  ingest)
    cargo llvm-cov --manifest-path "${root}/pipelines/maps-world-ingest/Cargo.toml" \
      --summary-only > "${report}"
    check_against "${report}" llvm "${ingest_floors[@]}"
    ;;
  web)
    (cd "${root}/applications/map-demo" && npx c8 --include 'app/**/*.mjs' \
      --reporter text node --test tests/*.test.mjs) > "${report}"
    check_against "${report}" c8 "${web_floors[@]}"
    ;;
  lab)
    (cd "${root}/applications/maps-v2-lab" && npx nyc report --temp-dir coverage/raw --reporter=text 2>&1) > "${report}"
    check_against "${report}" c8 "${lab_floors[@]}"
    ;;
  *)
    echo "unknown area: ${area}" >&2
    exit 2
    ;;
esac

if [ "${failed}" -gt 0 ]; then
  echo
  echo "${failed} file(s) below their floor. Coverage is a ratchet: add the tests,"
  echo "or say in the commit why the floor moves down."
  exit 1
fi
