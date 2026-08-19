# inspirex CLI

灵犀素材库命令行。查询走本机 `POST /v1/invoke`；`--demo` 离线可验。

## 安装

```bash
python3 -m venv cli/.venv
source cli/.venv/bin/activate
pip install -e "./cli[dev]"
inspirex --version
```

## 快速开始

```bash
inspirex --help
inspirex --demo --format json status
inspirex --demo --format json asset list
inspirex --demo --format json asset search 海报
inspirex config check
```

Live（需 `./scripts/start.sh`）：

```bash
inspirex -q --format json library info
inspirex -q --format json asset search 风景 --clip
inspirex --dry-run asset import ./photo.png
```

## 配置

优先级：CLI 参数 > 环境变量 > `~/.inspirex/config.yaml` > 项目 `.env` > 默认。

| 变量 | 含义 |
|------|------|
| `INSPIREX_INGEST_PORT` | 默认 18765 |
| `INSPIREX_INGEST_TOKEN` | Bearer（导入 REST 用） |
| `INSPIREX_CLI_BASE` | 覆盖后端 URL |
| `INSPIREX_LIBRARY_PATH` | 库目录 |

`inspirex config init` 写入 `~/.inspirex/config.yaml`。

## 退出码

0 成功 / 1 通用错误 / 2 检索无数据 / 3 入库失败。

## Agent 接入

见 `SKILL.md` 与 `agent/manifest.json`。

## 验证

```bash
./cli/verify.sh
# 或仓库根：./scripts/verify.sh
```
