#!/usr/bin/env bash
set -euo pipefail
# Optional: install a pre-push hook that runs the quick gate.
# The full gate (with e2e) is still mandatory before opening a PR — this hook
# is just a safety net for pushes. Run `bash scripts/install-hooks.sh` once.
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
hook="$root/.git/hooks/pre-push"
# Detect worktree vs main repo hooks path
if [ -n "${GIT_COMMON_DIR:-}" ]; then hook="$GIT_COMMON_DIR/hooks/pre-push"; fi
if [ ! -d "$(dirname "$hook")" ]; then hook="$root/.git/hooks/pre-push"; fi
cat > "$hook" <<'HOOK'
#!/usr/bin/env bash
set -euo pipefail
echo "pre-push: running quick gate (bash scripts/check.sh --quick)..."
bash scripts/check.sh --quick
HOOK
chmod +x "$hook"
echo "installed pre-push hook at $hook (runs --quick on push; full gate still required before PR)"
