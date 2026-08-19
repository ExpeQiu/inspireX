---
name: inspirex
description: 灵犀 InspireX 素材库 CLI。查询/检索/入库，对接本机 127.0.0.1:18765 /v1/invoke。
version: 0.1.0
metadata:
  cli_command: inspirex
  requires_install: true
  agent_safe_commands:
    - config-check
    - status
    - library-info
    - asset-list
    - asset-search
    - folder-list
    - tag-list
    - source-list
    - hub-status
  human_required_commands:
    - asset-import
    - asset-import-url
    - folder-create
    - tag-create
    - source-pull
---

# inspirex

灵犀个人视觉素材库命令行。宿主是 Tauri 本机后端；CLI 是薄客户端。

## 前置条件

```bash
cd "/Volumes/Lexar/git/10 baseTools/InspireX"
python3 -m venv cli/.venv && source cli/.venv/bin/activate
pip install -e "./cli[dev]"
inspirex --version
```

Live 查询需先 `./scripts/start.sh`。`--demo` 不依赖后端。

## Agent 推荐命令

全局选项放在子命令前：`--demo` `-q` `--format json`。

健康检查（agent_safe: true）：

```bash
inspirex --demo -q --format json status
```

- 退出码：0 成功 / 1 后端不可达
- 字段：`items[].ok` `items[].libraryPath` `items[].assetCount`

配置检查（后端未启动仍为 0）：

```bash
inspirex --demo -q --format json config check
```

列出素材：

```bash
inspirex -q --format json asset list --limit 20
```

- `--demo` 时 `data_source=demo`，固定 2 条夹具
- 退出码：0（空库也是 0）

检索：

```bash
inspirex -q --format json asset search "海报" --clip --limit 10
```

- 退出码：0 有命中 / 2 无数据 / 1 后端错误

库信息 / 文件夹 / 标签 / 订阅 / 中转站：

```bash
inspirex -q --format json library info
inspirex -q --format json folder list
inspirex -q --format json tag list
inspirex -q --format json source list
inspirex -q --format json hub status
```

## 长任务说明

无常驻子命令。`source pull` 可能较慢（订阅拉取），超时建议 120s，仍可前台调用。

## 禁止 Agent 自动调用

| 命令 | 原因 |
|------|------|
| `asset import` / `asset import-url` | 写入素材库，需用户确认 |
| `folder create` / `tag create` | 写库 |
| `source pull` | 拉取并可能入库 |

可用 `--dry-run` 预览 `asset import`。`--demo` 下写入类命令只返回夹具，不落盘。

## 输出解析

- stdout = JSON（`--format json`）
- stderr = 日志
- 契约字段：`module` `data_source` `items` `count` `fetched_at`
- 退出码：0 成功 / 1 通用错误 / 2 无数据 / 3 入库失败
