# ADR-006：浏览器为主界面，桌面进程当本机后端

> 状态：已采纳  
> 日期：2026-08-19  
> 模板：1（采集→管道→展示）

## 背景

前端在 Cursor / Chrome 打开时没有 Tauri IPC。原先只覆盖部分 HTTP 回退，其余命令报「需要在灵犀窗口」。主界面应是浏览器；原生窗口留给以后单独封装的 App。

## 决策

| 项 | 选择 |
|----|------|
| 主界面 | 浏览器 `http://127.0.0.1:1420` |
| 一级入口 | 工具栏 **素材管理 \| AI 生图**（`/generate` 不在侧栏「来源」） |
| 素材管理侧栏 | 全部素材、来源（外置卷 / 本地库 / 中转站 / 订阅源 / **AI创作**）、分类、标签 |
| AI 生图侧栏 | prompt · AI生图 · AI作品集（`/generate/prompt` `/generate` `/generate/created`） |
| 生图分工 | `/generate` 填描述并生成/入库；`/generate/prompt` 管理常用提示词，可按参考图挖掘 |
| 子页面 | `/library` `/volumes` `/local` `/hub` `/subscribe` `/created` `/generate` `/generate/prompt` `/generate/created` `/settings` `/folders/:id` |
| 后端 | 本机 Tauri 进程常驻 |
| 窗口 | 直接启动 / 封装 App / 点 Dock 图标 → 显示原生窗口；`./scripts/start.sh` 设 `INSPIREX_SHOW_WINDOW=false` 隐藏并用浏览器 |
| RPC | 全部 `invoke` 走 `POST /v1/invoke`（`127.0.0.1:18765`） |
| 插件采集 | 仍用 `/v1/import` 等 REST + Bearer |
| 封装 App | 默认显示窗口；前端可继续用 IPC |

拖出到其它 App 仍依赖原生窗口。选文件夹、选参考图用应用内浏览器弹窗（`list_picker_dir`）；无弹窗时仍可粘贴绝对路径。

浏览网格缩略图走 `GET /v1/browse/thumb` 直接返回 JPEG（hash 文件名 `Cache-Control: immutable`）。ingest HTTP 用线程池处理请求，避免单张解码堵住发图；原文件 `/v1/browse/file` 支持 `Range`，不再整包读入内存。

## 后果

- 正：开发预览即主路径；命令只实现一次（`rpc`）。  
- 负：本机进程必须先起；`start.sh` 隐藏窗口时仍占 Dock，点击图标会拉起原生窗口。  
- 后续：独立 App 包、托盘化、无窗口 headless 二进制。
