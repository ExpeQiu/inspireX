#!/usr/bin/env bash
# 启动本机后端（隐藏窗口）+ 浏览器主界面
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

mkdir -p .run
if [[ -f .run/inspirex.pid ]] && kill -0 "$(cat .run/inspirex.pid)" 2>/dev/null; then
  echo "InspireX 已在运行 pid=$(cat .run/inspirex.pid)"
  echo "主界面: http://127.0.0.1:${VITE_DEV_PORT:-1420}"
  exit 0
fi

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
UI_PORT="${VITE_DEV_PORT:-1420}"
INGEST_PORT="${INSPIREX_INGEST_PORT:-18765}"
UI_URL="http://127.0.0.1:${UI_PORT}"

if [[ ! -d node_modules ]]; then
  npm install
fi

echo "[inspirex] starting backend (clip_mock=${AI_MOCK_MODE} image_gen=${IMAGE_GEN_PROVIDER:-mock} show_window=${INSPIREX_SHOW_WINDOW})"
npm run tauri -- dev >.run/inspirex.log 2>&1 &
echo $! >.run/inspirex.pid
echo "已启动 pid=$(cat .run/inspirex.pid) 日志 .run/inspirex.log"

ok=0
for i in $(seq 1 120); do
  if curl -sf --max-time 1 "http://127.0.0.1:${INGEST_PORT}/health" >/dev/null \
    && curl -sf --max-time 1 "${UI_URL}" >/dev/null; then
    ok=1
    break
  fi
  sleep 1
done

if [[ "$ok" -eq 1 ]]; then
  echo "[inspirex] 后端就绪 ingest=:${INGEST_PORT} ui=${UI_URL}"
  if [[ "${INSPIREX_SHOW_WINDOW}" != "true" && "${INSPIREX_SHOW_WINDOW}" != "1" ]]; then
    if command -v open >/dev/null 2>&1; then
      open "${UI_URL}"
    fi
    echo "主界面已用浏览器打开 ${UI_URL}"
  else
    echo "原生窗口模式 INSPIREX_SHOW_WINDOW=${INSPIREX_SHOW_WINDOW}"
  fi
else
  echo "[inspirex] 等待超时。看日志: tail -f .run/inspirex.log" >&2
  echo "就绪后打开 ${UI_URL}" >&2
  exit 1
fi
