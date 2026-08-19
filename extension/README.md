# InspireX 浏览器插件（Chrome MV3）

## 安装

1. 启动桌面端：`./scripts/start.sh`（会监听 `127.0.0.1:18765`）
2. Chrome → 扩展程序 → 开发者模式 → 加载已解压的扩展程序 → 选本目录 `extension/`
3. 选项里确认 Token 与 `.env` 的 `INSPIREX_INGEST_TOKEN` 一致（默认 `inspirex-local`）
4. 若已装过旧版，点「重新加载」（当前 0.2.0，含盯梢）

## 使用

- 单张：在网页图片上右键 → **保存到灵犀 InspireX**
- 多选：右键页面 / 点扩展图标 → **本页多选保存**
- 盯梢：打开并登录目标页（如 Pinterest 看板）→ 扩展图标 **盯梢此页**。每 60 分钟扫描该标签上的新图并入库。关标签则该次跳过。灵犀「订阅源」里可看到盯梢状态和上次结果。

插件在 **Chrome 里下载图片** 再交给本机入库（避免后端直连 CDN 超时）。登录墙站点不要指望无人值守爬站。

## API

```
GET  /health
POST /v1/import   { imageUrl, imageBase64?, sourceRef? }
POST /v1/invoke   { cmd, args }   # create_source / report_source_run / …
Authorization: Bearer <token>（仅 /v1/import）
```
