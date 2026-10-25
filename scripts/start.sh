#!/usr/bin/env bash
# 启动本机后端 + 主界面（默认隐藏窗口、浏览器；INSPIREX_SHOW_WINDOW=true 开原生窗口）
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

mkdir -p .run
LOG=".run/inspirex.log"

log() {
  local line="[inspirex] $*"
  echo "$line"
  echo "$(date '+%Y-%m-%d %H:%M:%S') $line" >>"$LOG"
}

if [[ -f .run/inspirex.pid ]] && kill -0 "$(cat .run/inspirex.pid)" 2>/dev/null; then
  echo "InspireX 已在运行 pid=$(cat .run/inspirex.pid)"
  echo "主界面: http://localhost:${VITE_DEV_PORT:-1420}"
  exit 0
fi

# 先清掉封装版 / 残留端口，避免 health 命中旧进程
bash "$ROOT/scripts/stop.sh"

if [[ ! -f .env ]]; then
  cp .env.example .env
  echo "已从 .env.example 生成 .env"
fi

# shellcheck disable=SC1091
set -a
source .env
set +a

export AI_MOCK_MODE="${AI_MOCK_MODE:-true}"
export RUST_LOG="${RUST_LOG:-info}"
export INSPIREX_SHOW_WINDOW="${INSPIREX_SHOW_WINDOW:-false}"
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$HOME/.cache/inspirex-cargo-target}"
mkdir -p "$CARGO_TARGET_DIR"
log "cargo target-dir=${CARGO_TARGET_DIR}"

UI_PORT="${VITE_DEV_PORT:-1420}"
INGEST_PORT="${INSPIREX_INGEST_PORT:-18765}"
UI_URL="http://127.0.0.1:${UI_PORT}"
UI_URL_LOCAL="http://localhost:${UI_PORT}"

if [[ ! -d node_modules ]]; then
  npm install
fi

listener_cmd() {
  local port="$1"
  if ! command -v lsof >/dev/null 2>&1; then
    echo ""
    return
  fi
  local pid
  pid=$(lsof -ti tcp:"$port" -sTCP:LISTEN 2>/dev/null | head -1 || true)
  if [[ -z "$pid" ]]; then
    echo ""
    return
  fi
  ps -p "$pid" -o command= 2>/dev/null || echo ""
}

# 就绪条件：UI 可访问，且 18765 由本次开发态进程持有（禁止命中 /Applications 封装版）
is_dev_ingest() {
  local cmd
  cmd="$(listener_cmd "$INGEST_PORT")"
  [[ -z "$cmd" ]] && return 1
  if [[ "$cmd" == *"/Applications/InspireX.app/"* ]]; then
    return 1
  fi
  if [[ "$cmd" == *"inspirex-cargo-target"* ]] \
    || [[ "$cmd" == *"src-tauri/target"* ]] \
    || [[ "$cmd" == *"/inspirex"* && "$cmd" != *"/Applications/"* ]]; then
    return 0
  fi
  return 1
}

log "starting backend (clip_mock=${AI_MOCK_MODE} text_gen=${TEXT_GEN_PROVIDER:-mock} text_model=${TEXT_GEN_MODEL:-} image_gen=${IMAGE_GEN_PROVIDER:-mock} image_model=${IMAGE_GEN_MODEL:-} show_window=${INSPIREX_SHOW_WINDOW})"
: >"$LOG"
nohup npm run tauri -- dev >>"$LOG" 2>&1 &
echo $! >.run/inspirex.pid
disown $! 2>/dev/null || true
PARENT_PID="$(cat .run/inspirex.pid)"
log "launched pid=${PARENT_PID} log=${LOG}"

ok=0
# 冷编译可能超过 2 分钟；最多等 10 分钟，且父进程必须仍存活
for i in $(seq 1 600); do
  if ! kill -0 "$PARENT_PID" 2>/dev/null; then
    log "FAIL: 父进程 pid=${PARENT_PID} 已退出，看 ${LOG}"
    exit 1
  fi
  if curl -sf --max-time 1 "${UI_URL}" >/dev/null \
    || curl -sf --max-time 1 "${UI_URL_LOCAL}" >/dev/null; then
    if is_dev_ingest; then
      ok=1
      break
    fi
  fi
  if (( i % 30 == 0 )); then
    log "waiting… ${i}s ui/ingest 未齐（ingest=$(listener_cmd "$INGEST_PORT" | head -c 120))"
  fi
  sleep 1
done

if [[ "$ok" -eq 1 ]]; then
  log "后端就绪 ingest=:${INGEST_PORT} ui=${UI_URL_LOCAL} show_window=${INSPIREX_SHOW_WINDOW}"
  if [[ "${INSPIREX_SHOW_WINDOW}" != "true" && "${INSPIREX_SHOW_WINDOW}" != "1" ]]; then
    if command -v open >/dev/null 2>&1; then
      open "${UI_URL_LOCAL}"
    fi
    echo "主界面已用浏览器打开 ${UI_URL_LOCAL}"
  else
    echo "原生窗口模式 INSPIREX_SHOW_WINDOW=${INSPIREX_SHOW_WINDOW}"
  fi
else
  log "等待超时。看日志: tail -f ${LOG}"
  echo "就绪后打开 ${UI_URL_LOCAL}" >&2
  exit 1
fi
