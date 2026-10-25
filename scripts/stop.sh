#!/usr/bin/env bash
# 停止 InspireX 开发实例与占用端口的本机进程（含 /Applications 封装版）
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

stop_pid() {
  local pid="$1"
  if kill -0 "$pid" 2>/dev/null; then
    # 杀掉进程组内子进程（vite / cargo / rustc）
    pkill -P "$pid" 2>/dev/null || true
    kill "$pid" 2>/dev/null || true
    sleep 1
    if kill -0 "$pid" 2>/dev/null; then
      kill -9 "$pid" 2>/dev/null || true
    fi
  fi
}

if [[ -f .run/inspirex.pid ]]; then
  PID="$(cat .run/inspirex.pid)"
  log "stopping pid=$PID"
  stop_pid "$PID"
  rm -f .run/inspirex.pid
fi

# 开发态残留：npm / tauri / vite / cargo run
pkill -f "npm run tauri -- dev" 2>/dev/null || true
pkill -f "tauri dev" 2>/dev/null || true
pkill -f "vite.*inspireX|inspireX.*vite" 2>/dev/null || true

# 封装 App / 任意 inspirex 二进制（避免占 18765 导致 start 误判就绪）
if command -v pgrep >/dev/null 2>&1; then
  while read -r pid; do
    [[ -z "$pid" ]] && continue
    log "kill inspirex pid=$pid"
    stop_pid "$pid"
  done < <(pgrep -f '/InspireX\.app/Contents/MacOS/inspirex|/inspirex-cargo-target/.*/inspirex|src-tauri/target/.*/inspirex' 2>/dev/null || true)
fi

# 清理残留端口
if command -v lsof >/dev/null 2>&1; then
  for port in 1420 18765; do
    PIDS=$(lsof -ti tcp:"$port" 2>/dev/null || true)
    if [[ -n "${PIDS}" ]]; then
      log "free port $port: $PIDS"
      # shellcheck disable=SC2086
      kill $PIDS 2>/dev/null || true
      sleep 0.5
      # shellcheck disable=SC2086
      kill -9 $PIDS 2>/dev/null || true
    fi
  done
fi

log "stopped"
