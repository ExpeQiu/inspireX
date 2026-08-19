# ADR-007：标准化 CLI（模板 6 挂模板 1）

> 状态：已采纳  
> 日期：2026-08-19  
> 模板：6（CLI 工具集）⊂ 1（采集→管道→展示）

## 背景

ADR-001 已预留「可后续加采集 CLI」。浏览器主界面（ADR-006）把全部命令收敛到 `POST /v1/invoke`。需要一条可脚本化、可被 Agent 调用、离线可验的命令面。

## 选项

| # | 选项 | 优势 | 劣势 |
|---|------|------|------|
| A | 独立 `15CLI/inspirex-CLI` | 与其它 CLI 并列 | 与宿主 RPC 脱节，双仓同步 |
| B | 仓内 `cli/` + 命令 `inspirex` | 与 `/v1/invoke` 同仓、verify 一气 | 非 15CLI 目录名 |
| C | 仅 shell 包一层 curl | 零依赖 | 无 JSON 契约 / Agent manifest |

## 决策

选 **B**。

| 项 | 选择 |
|----|------|
| 包名 / 命令 | `inspirex-cli` / `inspirex` |
| 配置 | CLI 参数 > env > `~/.inspirex/config.yaml` > 项目 `.env` |
| 兄弟 CLI | 不依赖 |
| 主仓 | 本仓 `cli/`，不迁 15CLI |
| Mock | `--demo` 夹具；CLIP 仍用 `AI_MOCK_MODE` |

规范对齐 `/Volumes/Lexar/git/15CLI/CLI标准.md`；参考 `/Volumes/Lexar/git/15CLI/po-monitor-CLI`。

## 为何不用其他模板

- 2/3/4：不是场景合同或 Swarm
- 5：不是画布
- 单独模板 6 新仓：宿主已是模板 1，CLI 只做 Tool

## 后果

- 正：Agent / CI 可 `--demo` 闭环；live 复用同一套 rpc。
- 负：全局 flag 须写在子命令前（`inspirex --demo asset list`）。
