#!/usr/bin/env bash
# 启动 InspireX 桌面开发实例
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

mkdir -p .run
if [[ -f .run/inspirex.pid ]] && kill -0 "$(cat .run/inspirex.pid)" 2>/dev/null; then
  echo "InspireX 已在运行 pid=$(cat .run/inspirex.pid)"
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

if [[ ! -d node_modules ]]; then
  npm install
fi

echo "[inspirex] starting tauri dev (clip_mock=${AI_MOCK_MODE} image_gen=${IMAGE_GEN_PROVIDER:-mock})"
npm run tauri -- dev >.run/inspirex.log 2>&1 &
echo $! >.run/inspirex.pid
echo "已启动 pid=$(cat .run/inspirex.pid) 日志 .run/inspirex.log"
echo "健康观察：tail -f .run/inspirex.log"
