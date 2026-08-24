# ADR-008：VPromptStore 图片提示词同步进 AI 生图 prompt

> 状态：已采纳  
> 日期：2026-08-24  
> 模板：1（采集→管道→展示）

## 背景

VPromptStore 以 Markdown 为真源，维护分类图片提示词。InspireX 的「AI 生图 → prompt」需要这些条目可点选填入，但不能把整个仓库拷进素材库。

## 决策

| 项 | 选择 |
|----|------|
| 真源 | VPromptStore `图片提示词/<分类>/样本集合.md`，不复制仓库 |
| 落点 | 本地 SQLite `prompt_presets` / `prompt_categories` |
| 身份 | `source_kind=vprompt`，`source_uid=image:{分类}:{id}`，分类 `source_key=vprompt:image:{分类}` |
| 同步 | 按 uid upsert：更新正文、插入新条、删除片库已不存在的 vprompt 条；本地自建 prompt 不动 |
| 触发 | 界面「同步片库」；首次打开 prompt 且从未同步过则自动一次；CLI `inspirex prompt sync` |
| 绑定 | `VPROMPT_STORE_ROOT` 或 `~/InspireX/vprompt.json`；默认识别本机 VPromptStore 路径 |

## 为何不用其他做法

- 不把 1.4 万条全文拷进仓：样本集合才是当前仓库实存，且应继续由 VPromptStore 维护。
- 不每次打开都全量覆盖：用 uid 增量，避免冲掉用户给片库条目配的参考图。
- 不常驻同步进程：个人桌面，点一次或首次进入即可。

## 后果

- 正：prompt 页能按 VPromptStore 分类浏览并一键填入生图。
- 负：用户改过的片库条目，下次同步会被仓库正文覆盖；本地自建条目不受影响。
