#!/usr/bin/env bash
# 打本机可安装的 macOS DMG（不嵌入 .env / Key）
# Tauri 打完 dmg 会清掉 macos/*.app，以 .dmg 为验收产物。
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

LOG_DIR="$ROOT/.run"
mkdir -p "$LOG_DIR"
LOG="$LOG_DIR/bundle-macos.log"
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$HOME/.cache/inspirex-cargo-target}"
CARGO_TARGET="$CARGO_TARGET_DIR"
mkdir -p "$CARGO_TARGET"
BUNDLE_DIR="$CARGO_TARGET/release/bundle"
DIST_DIR="$ROOT/dist/macos"

{
  echo "===== inspirex bundle-macos $(date '+%Y-%m-%d %H:%M:%S') ====="
  echo "root=$ROOT"
  echo "cargo_target=$CARGO_TARGET"
  uname -m
  rustc -V
  echo "version=0.3.1 llm_text=MiniMax-M3 llm_image=image-01 key_in_bundle=false"
} | tee "$LOG"

if [[ ! -d node_modules ]]; then
  echo "[inspirex] npm install" | tee -a "$LOG"
  npm install >>"$LOG" 2>&1
fi

find "$ROOT/src-tauri" "$ROOT/src" -name '._*' -delete 2>/dev/null || true

echo "[inspirex] tauri build --bundles dmg（不打入 .env）" | tee -a "$LOG"
env -u IMAGE_GEN_API_KEY -u MINIMAX_API_KEY -u TEXT_GEN_API_KEY -u VIDEO_GEN_API_KEY \
  INSPIREX_SHOW_WINDOW=true \
  AI_MOCK_MODE="${AI_MOCK_MODE:-true}" \
  npm run tauri -- build --bundles dmg 2>&1 | tee -a "$LOG"

DMG="$(find "$BUNDLE_DIR/dmg" -maxdepth 1 -name '*.dmg' -print -quit 2>/dev/null || true)"
if [[ -z "$DMG" || ! -f "$DMG" ]]; then
  echo "[inspirex] FAIL: 未找到 .dmg，看 $LOG" >&2
  exit 1
fi

mkdir -p "$DIST_DIR"
cp -f "$DMG" "$DIST_DIR/"
OUT="$DIST_DIR/$(basename "$DMG")"

echo "[inspirex] verify dmg" | tee -a "$LOG"
if ! hdiutil verify "$OUT" >>"$LOG" 2>&1; then
  echo "[inspirex] FAIL: dmg 校验失败 $OUT" >&2
  exit 1
fi

ls -lh "$OUT" | tee -a "$LOG"
echo
echo "====== bundle ok ======"
echo "DMG: $OUT"
echo "安装: 打开 DMG，把 InspireX 拖到 Applications"
echo "首次若被拦截: 右键 → 打开"
echo "LLM: 文本 MiniMax-M3 / 生图 image-01；Key 不在包内，安装后读 ~/InspireX/ai-services.json 或设置页"
echo "日志: $LOG"
