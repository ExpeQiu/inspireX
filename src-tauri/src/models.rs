use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Asset {
    pub id: String,
    pub filename: String,
    pub stored_path: String,
    pub thumb_path: Option<String>,
    pub mime: Option<String>,
    pub width: Option<i64>,
    pub height: Option<i64>,
    pub file_size: Option<i64>,
    pub source_url: Option<String>,
    pub notes: Option<String>,
    pub folder_id: Option<String>,
    pub dominant_color: Option<String>,
    pub clip_status: String,
    pub created_at: String,
    pub tags: Vec<String>,
    #[serde(default)]
    pub source_kind: String,
    pub source_ref: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Folder {
    pub id: String,
    pub name: String,
    pub parent_id: Option<String>,
    pub created_at: String,
    /// 虚拟索引条数（folder_items；不复制原文件）
    #[serde(default)]
    pub item_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Tag {
    pub id: String,
    pub name: String,
    pub color: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryInfo {
    pub library_path: String,
    pub ai_mock_mode: bool,
    pub asset_count: i64,
    pub ingest_port: u16,
    pub clip_model: String,
    #[serde(default)]
    pub image_gen_provider: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IngestImportRequest {
    pub image_url: String,
    pub page_url: Option<String>,
    pub title: Option<String>,
    pub notes: Option<String>,
    pub folder_id: Option<String>,
    /// 插件在浏览器里下好的图，避免本机直连 CDN 超时
    #[serde(default)]
    pub image_base64: Option<String>,
    #[serde(default)]
    pub mime: Option<String>,
    #[serde(default)]
    pub source_ref: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IngestImportBatchRequest {
    pub image_urls: Vec<String>,
    pub page_url: Option<String>,
    pub title: Option<String>,
    pub notes: Option<String>,
    pub folder_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IngestCreateFolderRequest {
    pub name: String,
    pub parent_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IngestImportPathsRequest {
    pub paths: Vec<String>,
    pub folder_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportResult {
    pub imported: usize,
    pub skipped: usize,
    pub errors: Vec<String>,
    pub run_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchQuery {
    pub text: Option<String>,
    pub folder_id: Option<String>,
    pub tag: Option<String>,
    /// 以 #RRGGBB 主色相近搜图
    pub color_hex: Option<String>,
    #[serde(default)]
    pub use_clip: bool,
    pub limit: Option<i64>,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InvokeRequest {
    pub cmd: String,
    #[serde(default)]
    pub args: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateAssetMeta {
    pub asset_id: String,
    /// 外层 None=不改；内层 None=移出文件夹
    #[serde(default)]
    pub folder_id: Option<Option<String>>,
    #[serde(default)]
    pub source_url: Option<String>,
    #[serde(default)]
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteAssetResult {
    pub deleted: bool,
    pub files_removed: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Source {
    pub id: String,
    pub name: String,
    /// rss | url_list | html_page | plugin_watch
    pub kind: String,
    pub url: String,
    pub interval_min: i64,
    pub folder_id: Option<String>,
    pub enabled: bool,
    pub last_run_at: Option<String>,
    pub last_error: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateSource {
    pub name: String,
    pub kind: String,
    pub url: String,
    pub interval_min: Option<i64>,
    pub folder_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GenJob {
    pub id: String,
    pub prompt: String,
    pub n: i64,
    pub size: String,
    pub provider: String,
    pub model: String,
    pub status: String,
    pub folder_id: Option<String>,
    pub error: Option<String>,
    pub run_id: Option<String>,
    pub imported: i64,
    pub created_at: String,
    pub finished_at: Option<String>,
    /// 生成落盘路径；入库前可预览，入库后仍保留
    #[serde(default)]
    pub output_paths: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GenerateRequest {
    pub prompt: String,
    pub n: Option<i64>,
    pub size: Option<String>,
    pub folder_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PromptPreset {
    pub id: String,
    pub name: String,
    pub body: String,
    #[serde(default)]
    pub image_path: Option<String>,
    #[serde(default)]
    pub category_id: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreatePromptPreset {
    pub name: String,
    pub body: String,
    #[serde(default)]
    pub image_path: Option<String>,
    #[serde(default)]
    pub category_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdatePromptPreset {
    pub id: String,
    pub name: String,
    pub body: String,
    #[serde(default)]
    pub image_path: Option<String>,
    #[serde(default)]
    pub category_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PromptCategory {
    pub id: String,
    pub name: String,
    pub created_at: String,
    #[serde(default)]
    pub item_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MinedPrompt {
    pub name: String,
    pub body: String,
    pub provider: String,
}
