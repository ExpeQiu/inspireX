#!/usr/bin/env bash
# inspirex CLI 离线验收（--demo，不依赖 ingest）
set -euo pipefail
ROOT="$(cd "$(dirname "$0")" && pwd)"
REPO="$(cd "$ROOT/.." && pwd)"
cd "$ROOT"

fail() { echo "FAIL: $*" >&2; exit 1; }
ok() { echo "OK: $*"; }

command -v python3 >/dev/null || fail "需要 python3"

if [[ ! -d .venv ]]; then
  python3 -m venv .venv
fi
# shellcheck disable=SC1091
source .venv/bin/activate
python -m pip install -q -U pip
python -m pip install -q -e ".[dev]"
ok "pip install -e ./cli"

inspirex --version
inspirex --help >/dev/null
inspirex asset --help >/dev/null
ok "help / version"

echo "[verify-cli] config check --demo JSON"
CHECK=$(inspirex --demo -q --format json config check)
echo "$CHECK" | python3 -c "
import sys, json
d = json.load(sys.stdin)
assert d.get('module') == 'config-check', d
assert d.get('data_source') == 'demo'
assert d.get('items'), d
print('  ✓ config check')
"

echo "[verify-cli] asset list --demo JSON 契约"
LIST=$(inspirex --demo -q --format json asset list)
echo "$LIST" | python3 -c "
import sys, json
d = json.load(sys.stdin)
assert d.get('module') == 'asset-list'
assert d.get('data_source') == 'demo'
assert d.get('count') == 2
assert d['items'][0].get('filename') == 'sunset.png'
assert 'fetched_at' in d
print('  ✓ asset list demo')
"

echo "[verify-cli] asset search miss → exit 2"
set +e
inspirex --demo -q --format json asset search zzz-no-such >/dev/null
code=$?
set -e
[[ "$code" -eq 2 ]] || fail "search miss 期望 exit 2，实际 $code"
ok "search miss exit 2"

echo "[verify-cli] pytest"
python -m pytest -q tests
ok "pytest"

echo
echo "====== cli verify passed ======"
echo "Live: 先 $REPO/scripts/start.sh 再 inspirex -q --format json status"
