# 灵犀 InspireX

个人全本地视觉素材库（Tauri 2 + React + SQLite + Mock/本地 CLIP）。

**架构**：模板 1（采集→管道→展示），裁剪为单机。开发以浏览器为主界面，本机 Rust 进程常驻当后端（`127.0.0.1:18765`）。后续可单独封装桌面 App。决策见 `ADR/001`、`ADR/006`。

## 快速开始

```bash
cp .env.example .env
./scripts/verify.sh
./scripts/start.sh    # 本机后端 + 浏览器主界面 http://127.0.0.1:1420
./scripts/stop.sh
```

默认库目录：`~/InspireX/Library`（可用 `INSPIREX_LIBRARY_PATH` 覆盖）。

> 若仓库在 Lexar 等外置卷：已通过 `.cargo/config.toml` 把 Rust `target` 指到本机 `~/.cache/inspirex-cargo-target`，避免 AppleDouble 污染构建。

## 当前能力

- 外置卷 / 本地库 / **中转站**（绑定 iCloud 等云盘文件夹，存入后多端共享，不入灵犀库）
- **订阅源**（RSS/Atom / 公开网页抽图 / URL 列表）定时或立即拉取入库
- **AI 生图**（默认 MiniMax `image-01`；无 Key 时回退 Mock）
- 文件夹 + 标签 + 出处/笔记
- FTS / 标签 / 颜色 / CLIP 以文搜图（Mock 或本地 transformers）
- 拖出 / Finder / 删除
- **浏览器插件采集**（单张 + 本页多选，`extension/` → `127.0.0.1:18765`）
- 无云、无账号

## 真实 CLIP

```bash
# .env
AI_MOCK_MODE=false
CLIP_MODEL=openai/clip-vit-base-patch32
```

需本机 `python3` + `torch` + `transformers`；sidecar：`scripts/clip_embed.py`。

## AI 生图（MiniMax）

CLIP Mock 与生图独立。`.env`：

```bash
AI_MOCK_MODE=true
IMAGE_GEN_PROVIDER=minimax
IMAGE_GEN_BASE_URL=https://api.minimaxi.com
IMAGE_GEN_API_KEY=          # 或复用 MINIMAX_API_KEY
IMAGE_GEN_MODEL=image-01
```

无 Key 时自动回退占位图。Key 只进 `.env`，不要提交。生图需 [按量计费接口密钥](https://platform.minimaxi.com/user-center/basic-information/interface-key)。

## 浏览器插件

见 `extension/README.md`。

## 下一步

- Eagle 库迁移
- 入库队列 UI / 重跑 embedding
- 视频多关键帧
