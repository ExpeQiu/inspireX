#!/usr/bin/env bash
# 停止 InspireX 开发实例
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

stop_pid() {
  local pid="$1"
  if kill -0 "$pid" 2>/dev/null; then
    # 杀掉进程组内子进程（vite / cargo）
    pkill -P "$pid" 2>/dev/null || true
    kill "$pid" 2>/dev/null || true
    sleep 1
    kill -9 "$pid" 2>/dev/null || true
  fi
}

if [[ -f .run/inspirex.pid ]]; then
  PID="$(cat .run/inspirex.pid)"
  echo "[inspirex] stopping pid=$PID"
  stop_pid "$PID"
  rm -f .run/inspirex.pid
fi

# 清理残留 vite / tauri / ingest 端口
if command -v lsof >/dev/null 2>&1; then
  for port in 1420 18765; do
    PIDS=$(lsof -ti tcp:"$port" 2>/dev/null || true)
    if [[ -n "${PIDS}" ]]; then
      echo "[inspirex] free port $port: $PIDS"
      kill $PIDS 2>/dev/null || true
    fi
  done
fi

echo "[inspirex] stopped"
