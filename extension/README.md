# InspireX 浏览器插件（Chrome MV3）

## 安装

1. 启动桌面端：`./scripts/start.sh`（会监听 `127.0.0.1:18765`）
2. Chrome → 扩展程序 → 开发者模式 → 加载已解压的扩展程序 → 选本目录 `extension/`
3. 选项里确认 Token 与 `.env` 的 `INSPIREX_INGEST_TOKEN` 一致（默认 `inspirex-local`）
4. 若已装过旧版，点「重新加载」以启用本页多选

## 使用

- 单张：在网页图片上右键 → **保存到灵犀 InspireX**
- 多选：右键页面空白 / 点扩展图标 → **本页多选保存** → 勾选后保存（一次最多 30 张）

插件把图片 URL POST 到本机，桌面端下载并入库（带 pageUrl 出处）。登录墙站点请在已打开的页面上保存，不要指望无人值守爬取。

## API

```
GET  /health
POST /v1/import
POST /v1/import/batch
Authorization: Bearer <token>
{ "imageUrl": "..." } 或 { "imageUrls": ["..."], "pageUrl": "...", "title": "..." }
```
