#!/usr/bin/env bash
# InspireX 验收：依赖 / 编译 / 单测 / Mock 导入闭环
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

fail() { echo "FAIL: $*" >&2; exit 1; }
ok() { echo "OK: $*"; }

[[ -f .env.example ]] || fail "缺少 .env.example"
[[ -f ADR/001-personal-local-clip.md ]] || fail "缺少 ADR-001"
[[ -f ADR/002-source-adapters.md ]] || fail "缺少 ADR-002"
[[ -f ADR/003-source-link-no-copy.md ]] || fail "缺少 ADR-003"
[[ -f ADR/005-hub-cloud-folder.md ]] || fail "缺少 ADR-005"
[[ -f ADR/006-browser-primary-ui.md ]] || fail "缺少 ADR-006"
[[ -f ADR/007-cli-standard.md ]] || fail "缺少 ADR-007"
[[ -f ADR/008-vprompt-sync.md ]] || fail "缺少 ADR-008"
[[ -f ADR/009-reuse-photos-classify.md ]] || fail "缺少 ADR-009"
[[ -f guide/复用系统相册归类_20261025.md ]] || fail "缺少相册归类指南"
[[ -f src-tauri/Cargo.toml ]] || fail "缺少 src-tauri"
[[ -f cli/pyproject.toml ]] || fail "缺少 cli/pyproject.toml"
[[ -f cli/SKILL.md ]] || fail "缺少 cli/SKILL.md"
[[ -f cli/agent/manifest.json ]] || fail "缺少 cli/agent/manifest.json"

if [[ ! -f .env ]]; then
  cp .env.example .env
  ok "生成 .env"
fi

# shellcheck disable=SC1091
set -a
source .env
set +a
export AI_MOCK_MODE="${AI_MOCK_MODE:-true}"

# cargo test 会读环境变量；用户 .env 里的 Key/Provider 不能串进单测
unset IMAGE_GEN_PROVIDER IMAGE_GEN_API_KEY IMAGE_GEN_BASE_URL IMAGE_GEN_MODEL
unset MINIMAX_API_KEY MINIMAX_BASE_URL MINIMAX_IMAGE_MODEL
unset TEXT_GEN_PROVIDER TEXT_GEN_API_KEY TEXT_GEN_BASE_URL TEXT_GEN_MODEL
unset VIDEO_GEN_PROVIDER VIDEO_GEN_API_KEY VIDEO_GEN_BASE_URL VIDEO_GEN_MODEL
unset INSPIREX_AI_CONFIG INSPIREX_VPROMPT_CONFIG VPROMPT_STORE_ROOT

command -v npm >/dev/null || fail "需要 npm"
command -v cargo >/dev/null || fail "需要 cargo/rustc"

if [[ ! -d node_modules ]]; then
  npm install
fi
ok "node_modules"

echo "[verify] cargo test (mock CLIP + db)"
(
  cd src-tauri
  cargo test --quiet -- --test-threads=1
)
ok "cargo test"

echo "[verify] Mock 导入 / RSS 订阅 / 生图"
VERIFY_LIB="$(mktemp -d "${TMPDIR:-/tmp}/inspirex-verify.XXXXXX")"
export INSPIREX_LIBRARY_PATH="$VERIFY_LIB"
export AI_MOCK_MODE=true
export IMAGE_GEN_PROVIDER=mock
# 生成测试 PNG
python3 - <<'PY'
from pathlib import Path
import os, struct, zlib
out = Path(os.environ["INSPIREX_LIBRARY_PATH"]) / "sample.png"
# 最小 1x1 PNG
sig = b"\x89PNG\r\n\x1a\n"
def chunk(tag, data):
    return struct.pack(">I", len(data)) + tag + data + struct.pack(">I", zlib.crc32(tag + data) & 0xffffffff)
ihdr = struct.pack(">IIBBBBB", 8, 8, 8, 2, 0, 0, 0)
raw = b"".join(b"\x00" + bytes([i, 40, 80]) * 8 for i in range(8))
idat = zlib.compress(raw, 9)
png = sig + chunk(b"IHDR", ihdr) + chunk(b"IDAT", idat) + chunk(b"IEND", b"")
out.write_bytes(png)
print(out)
PY

(
  cd src-tauri
  cargo test --quiet import_pipeline_mock -- --nocapture --test-threads=1
  cargo test --quiet rss_pull_then_skip_duplicate -- --nocapture --test-threads=1
  cargo test --quiet html_page_pull_then_skip_duplicate -- --nocapture --test-threads=1
  cargo test --quiet html_page_respects_robots_disallow -- --nocapture --test-threads=1
  cargo test --quiet plugin_watch_create_and_rust_does_not_crawl -- --nocapture --test-threads=1
  cargo test --quiet pinterest_url_coerces_to_plugin_watch -- --nocapture --test-threads=1
  cargo test --quiet pinterest_host_is_login_wall -- --nocapture --test-threads=1
  cargo test --quiet mock_single_and_batch -- --nocapture --test-threads=1
)
ok "mock import / rss / imagegen"

echo "[verify] Pinterest 收集器（插件）"
node "$ROOT/extension/test-pinterest.mjs"
ok "pinterest collect"

# 前端可编译（不要求 GUI）
npm run build
ok "frontend build"

echo "[verify] 桌面桥 HTTP 回退规则"
node scripts/check-bridge.mjs
ok "bridge fallback"

echo "[verify] 子页面路由"
node scripts/check-routes.mjs
ok "app routes"

echo "[verify] inspirex CLI --demo"
bash "$ROOT/cli/verify.sh"
ok "inspirex CLI"

if curl -sf --max-time 2 "http://127.0.0.1:${INSPIREX_INGEST_PORT:-18765}/health" >/dev/null; then
  curl -sf --max-time 2 "http://127.0.0.1:${INSPIREX_INGEST_PORT:-18765}/v1/browse/root?kind=volumes" >/dev/null \
    || fail "ingest 已启动但 /v1/browse/root 失败"
  curl -sf --max-time 2 -X POST "http://127.0.0.1:${INSPIREX_INGEST_PORT:-18765}/v1/invoke" \
    -H "Content-Type: application/json" \
    -d '{"cmd":"get_library_info","args":{}}' >/dev/null \
    || fail "ingest 已启动但 /v1/invoke 失败"
  code="$(curl -s -o /dev/null -w "%{http_code}" --max-time 2 "http://127.0.0.1:${INSPIREX_INGEST_PORT:-18765}/v1/browse/thumb")"
  [[ "$code" == "400" ]] || fail "GET /v1/browse/thumb 无 path 应 400，实际 $code"
  ok "ingest browse + invoke + thumb contract"
else
  echo "SKIP: ingest 未在监听（未 start.sh 时正常）"
fi

echo
echo "====== verify passed ======"
echo "库路径示例: $VERIFY_LIB"
echo "下一步: ./scripts/start.sh"
