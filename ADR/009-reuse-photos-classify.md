# ADR-009：优先复用本机「照片」归类

> 状态：已采纳  
> 日期：2026-10-25

## 背景

手机/Mac 相册的人物、地点、场景由系统端侧 ML 算好并写入库。InspireX 需要同类筛选，但自研人脸聚类在 Intel + 8GB 上成本高，且系统相册已覆盖用户主图库。

## 决策

| 项 | 选择 |
|----|------|
| 系统相册来源 | **只读复用** `Photos.sqlite` 中的人物 / GPS 地点（离线城市标签）/ 关键词 |
| ML 引擎 | **不调用**；无法对外暴露 Photos Neural Engine |
| 非 Photos 素材 | 后续再补自建 EXIF/CLIP；本阶段不做 |
| 权限 | 需用户授予「完全磁盘访问」或可选中 `.photoslibrary`；读失败时 UI 提示 |

## 实现要点

- `photos_meta.rs`：探测 `ZDETECTEDFACE` 列名（`ZASSETFORFACE` / `ZPERSONFORFACE` 等）兼容多版本
- `browse` 列表 enrichment：`ExternalItem.people` / `place`
- RPC：`get_photos_taxonomy`、`list_browse_dir` 的 `person` / `place` 筛选
- 场景 labels（`ZSCENECLASSIFICATION` ID→文案）表映射复杂，暂不解析；关键词表有则列出

## 后果

- 正：零推理、人物质量接近系统相册、与现有链接浏览一致  
- 负：仅覆盖已进「照片」且已分析的图；schema 升级需跟进；无 FDA 时读库失败  
