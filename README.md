# 灵犀 InspireX

个人全本地视觉素材库（Tauri 2 + React + SQLite + Mock/本地 CLIP）。

**架构**：模板 1（采集→管道→展示），裁剪为单机。开发可用浏览器或原生窗口。本机 Rust 进程常驻当后端（`127.0.0.1:18765`）。决策见 `ADR/001`、`ADR/006`。

## 快速开始

```bash
cp .env.example .env
./scripts/verify.sh
npm run tauri -- dev  # 直接打开原生窗口
./scripts/start.sh    # 隐藏窗口 + 浏览器主界面 http://127.0.0.1:1420
./scripts/stop.sh
./scripts/bundle-macos.sh   # 打本机 DMG → dist/macos/
```

安装：打开 `dist/macos/InspireX_0.3.0_aarch64.dmg`，把 InspireX 拖到「应用程序」。首次若被拦截，右键 → 打开。

点 Dock / 任务栏里的灵犀图标也会显示窗口（即使当时是浏览器模式）。

主界面一级入口在工具栏：**素材管理 | AI 生图**。侧栏主题（全部素材 / 来源 / 文件夹 / 标签）归属素材管理。子路径可收藏、新开标签：

| 地址 | 页面 |
|------|------|
| `/` 或 `/volumes` | 外置卷（素材管理） |
| `/library` | 全部素材（入库 + 外置卷/本地库/中转站链接图） |
| `/local` | 本地库 |
| `/hub` | 中转站 |
| `/subscribe` | 订阅源 |
| `/created` | AI创作（入库的生图，亦在全部素材） |
| `/generate` | AI 生图（填写、预览、入库） |
| `/generate/prompt` | 常用 prompt |
| `/generate/prompt/<id>` | prompt 三级分类 |
| `/generate/created` | AI作品集（生图模块内） |
| `/settings` | 设置（文本 / 图片 / 视频 AI 服务） |
| `/folders/<id>` | 灵犀分类 |

浏览目录会带 `?path=`。

默认库目录：`~/InspireX/Library`（可用 `INSPIREX_LIBRARY_PATH` 覆盖）。

> 若仓库在 Lexar 等外置卷：已通过 `.cargo/config.toml` 把 Rust `target` 指到本机 `~/.cache/inspirex-cargo-target`，避免 AppleDouble 污染构建。

## 当前能力

- 外置卷 / 本地库 / **中转站**（绑定 iCloud 等云盘文件夹，存入后多端共享，不入灵犀库）
- **订阅源**（RSS/Atom / 公开网页抽图 / URL 列表 / **Pinterest 看板自动盯梢**）定时或立即拉取入库
- **AI 生图**（默认 MiniMax `image-01`；无 Key 时回退 Mock）
- **常用 prompt / 片库同步**（VPromptStore 图片提示词进 AI生图-prompt，可重复同步）
- 分类 + 标签 + 出处/笔记
- FTS / 标签 / 颜色 / CLIP 以文搜图（Mock 或本地 transformers）
- 拖出 / Finder / 删除
- **浏览器插件采集**（单张 + 本页多选 + 盯梢已打开页面，`extension/` → `127.0.0.1:18765`）
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

无 Key 时自动回退占位图。也可在侧栏 **设置** 里配置文本 / 图片 / 视频服务（写入 `~/InspireX/ai-services.json`，优先于 `.env`）。Key 不要提交。生图需 [按量计费接口密钥](https://platform.minimaxi.com/user-center/basic-information/interface-key)。

## CLI

标准化命令 `inspirex`（模板 6，见 `ADR/007`）。全局选项在子命令前。

```bash
python3 -m venv cli/.venv && source cli/.venv/bin/activate
pip install -e "./cli[dev]"
inspirex --demo --format json asset list
inspirex --demo --format json asset search 海报
```

Live 需本机后端已启动。说明见 `cli/README.md`、`cli/SKILL.md`。

## 浏览器插件

见 `extension/README.md`。

## 下一步

- Eagle 库迁移
- 入库队列 UI / 重跑 embedding
- 视频多关键帧
