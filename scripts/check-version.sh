#!/usr/bin/env bash
set -euo pipefail
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
lib="$root/libraries/maps-v2/crates/maps2-tile/src/lib.rs"
v=$(grep -E '^pub const FORMAT_VERSION: u16 =' "$lib" | sed -n 's/.*= \([0-9]*\);/\1/p')
fail=0
# Title of tile-format.md must contain current version
if ! grep -q "версия $v — заморожен" "$root/libraries/maps-v2/docs/tile-format.md"; then
  echo "FAIL: tile-format.md title should contain 'версия $v — заморожен' (got: $(head -1 "$root/libraries/maps-v2/docs/tile-format.md"))"
  fail=1
fi
# TLDR must contain FORMAT_VERSION = v
if ! grep -q "FORMAT_VERSION = $v" "$root/libraries/maps-v2/docs/tile-format.en.tldr.md"; then
  echo "FAIL: tile-format.en.tldr.md should contain 'FORMAT_VERSION = $v' (got: $(grep FORMAT_VERSION "$root/libraries/maps-v2/docs/tile-format.en.tldr.md"))"
  fail=1
fi
# Header diagram version
if ! grep -q "version u16 LE (= $v)" "$lib"; then
  echo "FAIL: lib.rs header diagram should say version u16 LE (= $v)"
  fail=1
fi
# README current claim
if ! grep -q "MT2 v$v" "$root/README.md"; then
  echo "WARN: README.md does not mention MT2 v$v"
fi
if [ "$fail" -ne 0 ]; then exit 1; fi
echo "version $v consistent (lib.rs + tile-format.md + tldr)"
