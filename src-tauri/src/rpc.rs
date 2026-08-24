//! 桌面 IPC 与 HTTP `/v1/invoke` 共用的命令实现。
//! 浏览器为主界面时走本机 18765；封装 App 后仍可走 Tauri invoke。

use crate::browse::{
    add_index_root, add_local_index_root, copy_into_hub, ensure_readable, ensure_under_kind,
    hub_status, is_kind_virtual_root, item_from_path, linked_source_roots, list_browse_for_kind,
    list_index_roots, list_local_index_roots, list_picker_dir as browse_list_picker_dir,
    matching_index_root, parse_browse_kind, remove_index_root, remove_local_index_root,
    root_for_kind, set_hub_root, virtual_root_for, ExternalItem, ExternalKind, ExternalListing,
    HubCopyResult, HubStatus, IndexRoot, LocalIndexRoot,
};
use crate::db;
use crate::finder_tags::{self, FinderTag, FinderTagValue};
use crate::library::{asset_abs_path, delete_asset, import_paths, library_info, AppState};
use crate::models::{
    Asset, CreatePromptPreset, CreateSource, Folder, GenerateRequest, GenJob, IngestImportRequest,
    MinedPrompt, PromptCategory, PromptPreset, SearchQuery, Source, Tag, UpdateAssetMeta,
    UpdatePromptPreset,
};
use crate::pipeline::{color_distance, cosine_similarity, parse_hex_color};
use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde_json::{json, Value};
use std::path::PathBuf;

#[derive(serde::Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FolderContents {
    pub folder: Folder,
    pub assets: Vec<Asset>,
    pub media: Vec<ExternalItem>,
}

fn placeholder_media(path: &str) -> ExternalItem {
    let name = std::path::Path::new(path)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string());
    ExternalItem {
        path: path.to_string(),
        name,
        kind: ExternalKind::Image,
        file_size: None,
        modified_at: None,
        mime: None,
    }
}

fn json_ok<T: serde::Serialize>(v: T) -> Result<Value, String> {
    serde_json::to_value(v).map_err(|e| e.to_string())
}

fn parse<T: DeserializeOwned>(args: &Value) -> Result<T, String> {
    serde_json::from_value(if args.is_null() {
        json!({})
    } else {
        args.clone()
    })
    .map_err(|e| format!("参数错误: {e}"))
}

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct ListAssetsArgs {
    folder_id: Option<String>,
    source_kind: Option<String>,
    source_ref: Option<String>,
    limit: Option<i64>,
}

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct NameParentArgs {
    name: String,
    parent_id: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct FolderIdArgs {
    folder_id: String,
}

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct FolderItemsArgs {
    folder_id: String,
    asset_ids: Option<Vec<String>>,
    paths: Option<Vec<String>>,
}

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct CreateTagArgs {
    name: String,
    color: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct SetAssetTagsArgs {
    asset_id: String,
    tags: Vec<String>,
}

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct ImportFilesArgs {
    paths: Vec<String>,
    folder_id: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct DeleteAssetArgs {
    asset_id: String,
    delete_files: Option<bool>,
}

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct AssetIdArgs {
    asset_id: String,
}

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct SearchArgs {
    query: SearchQuery,
}

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct ImportUrlArgs {
    req: Option<IngestImportRequest>,
}

impl Default for SearchQuery {
    fn default() -> Self {
        Self {
            text: None,
            folder_id: None,
            tag: None,
            color_hex: None,
            use_clip: false,
            limit: None,
        }
    }
}

impl Default for IngestImportRequest {
    fn default() -> Self {
        Self {
            image_url: String::new(),
            page_url: None,
            title: None,
            notes: None,
            folder_id: None,
            image_base64: None,
            mime: None,
            source_ref: None,
        }
    }
}

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct KindArgs {
    kind: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct PathArgs {
    path: String,
}

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct VpromptSyncArgs {
    root: Option<String>,
    path: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct KindPathArgs {
    kind: Option<String>,
    path: String,
}

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct CopyHubArgs {
    paths: Vec<String>,
    dest_dir: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct BrowseDirArgs {
    path: Option<String>,
    limit: Option<usize>,
    offset: Option<usize>,
    kind: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct PickerDirArgs {
    path: Option<String>,
    kind: Option<String>,
    include_media: Option<bool>,
}

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct BrowseByTagArgs {
    path: Option<String>,
    tag: String,
    limit: Option<usize>,
    offset: Option<usize>,
    kind: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct ApplyFinderTagArgs {
    paths: Vec<String>,
    tag: String,
    color: Option<u8>,
    remove: Option<bool>,
}

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct ThumbArgs {
    path: String,
    max_side: Option<u32>,
}

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct CreateSourceArgs {
    req: Option<CreateSource>,
}

impl Default for CreateSource {
    fn default() -> Self {
        Self {
            name: String::new(),
            kind: String::new(),
            url: String::new(),
            interval_min: None,
            folder_id: None,
        }
    }
}

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct SourceIdArgs {
    source_id: String,
}

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct SourceEnabledArgs {
    source_id: String,
    enabled: bool,
}

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct SourceRunArgs {
    source_id: String,
    summary: String,
}

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct GenerateArgs {
    req: Option<GenerateRequest>,
}

impl Default for GenerateRequest {
    fn default() -> Self {
        Self {
            prompt: String::new(),
            n: None,
            size: None,
            folder_id: None,
        }
    }
}

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct ImportGenArgs {
    job_id: String,
    folder_id: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct LimitArgs {
    limit: Option<i64>,
}

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct CreatePromptPresetArgs {
    req: Option<CreatePromptPreset>,
}

impl Default for CreatePromptPreset {
    fn default() -> Self {
        Self {
            name: String::new(),
            body: String::new(),
            image_path: None,
            category_id: None,
        }
    }
}

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct UpdatePromptPresetArgs {
    req: Option<UpdatePromptPreset>,
}

impl Default for UpdatePromptPreset {
    fn default() -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            body: String::new(),
            image_path: None,
            category_id: None,
        }
    }
}

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct PromptPresetIdArgs {
    preset_id: String,
}

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct CreatePromptCategoryArgs {
    name: String,
}

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct PromptCategoryIdArgs {
    category_id: String,
}

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct MetaArgs {
    meta: Option<UpdateAssetMeta>,
}

impl Default for UpdateAssetMeta {
    fn default() -> Self {
        Self {
            asset_id: String::new(),
            folder_id: None,
            source_url: None,
            notes: None,
        }
    }
}

pub fn list_assets(
    state: &AppState,
    folder_id: Option<String>,
    source_kind: Option<String>,
    source_ref: Option<String>,
    limit: Option<i64>,
) -> Result<Vec<Asset>, String> {
    let conn = state.open_db().map_err(|e| e.to_string())?;
    db::list_assets_ex(
        &conn,
        folder_id.as_deref().filter(|s| !s.is_empty()),
        source_kind.as_deref().filter(|s| !s.is_empty()),
        source_ref.as_deref().filter(|s| !s.is_empty()),
        limit.unwrap_or(200),
    )
    .map_err(|e| e.to_string())
}

pub fn list_folders(state: &AppState) -> Result<Vec<Folder>, String> {
    let conn = state.open_db().map_err(|e| e.to_string())?;
    db::list_folders(&conn).map_err(|e| e.to_string())
}

pub fn delete_folder(state: &AppState, folder_id: String) -> Result<(usize, usize), String> {
    let conn = state.open_db().map_err(|e| e.to_string())?;
    db::delete_folder(&conn, &folder_id).map_err(|e| e.to_string())
}

pub fn list_folder_contents(state: &AppState, folder_id: String) -> Result<FolderContents, String> {
    let conn = state.open_db().map_err(|e| e.to_string())?;
    let folder = db::get_folder(&conn, &folder_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("文件夹不存在: {folder_id}"))?;
    let rows = db::list_folder_items(&conn, &folder_id).map_err(|e| e.to_string())?;
    let mut assets = Vec::new();
    let mut media = Vec::new();
    let mut seen_assets = std::collections::HashSet::new();
    for row in rows {
        match row.kind.as_str() {
            "asset" => {
                if let Some(a) = db::get_asset(&conn, &row.item_ref).map_err(|e| e.to_string())? {
                    seen_assets.insert(a.id.clone());
                    assets.push(a);
                }
            }
            "path" => {
                let p = PathBuf::from(&row.item_ref);
                media.push(item_from_path(&p).unwrap_or_else(|| placeholder_media(&row.item_ref)));
            }
            _ => {}
        }
    }
    for a in db::list_assets(&conn, Some(&folder_id), 2000).map_err(|e| e.to_string())? {
        if seen_assets.insert(a.id.clone()) {
            assets.push(a);
        }
    }
    log::info!(
        target: "inspirex",
        "folder_contents id={} name={} assets={} media={}",
        folder.id,
        folder.name,
        assets.len(),
        media.len()
    );
    Ok(FolderContents {
        folder,
        assets,
        media,
    })
}

pub fn add_to_folder(
    state: &AppState,
    folder_id: String,
    asset_ids: Option<Vec<String>>,
    paths: Option<Vec<String>>,
) -> Result<usize, String> {
    let conn = state.open_db().map_err(|e| e.to_string())?;
    if db::get_folder(&conn, &folder_id)
        .map_err(|e| e.to_string())?
        .is_none()
    {
        return Err(format!("文件夹不存在: {folder_id}"));
    }
    let mut items: Vec<(String, String)> = Vec::new();
    for id in asset_ids.unwrap_or_default() {
        let id = id.trim().to_string();
        if !id.is_empty() {
            items.push(("asset".into(), id));
        }
    }
    for path in paths.unwrap_or_default() {
        let path = path.trim().to_string();
        if path.is_empty() {
            continue;
        }
        let p = PathBuf::from(&path);
        let stored = p
            .canonicalize()
            .map(|c| c.display().to_string())
            .unwrap_or(path);
        items.push(("path".into(), stored));
    }
    if items.is_empty() {
        return Err("未选择要放入的图片".into());
    }
    let n = db::add_folder_items(&conn, &folder_id, &items).map_err(|e| e.to_string())?;
    log::info!(
        target: "inspirex",
        "folder_add id={} items={} added={}",
        folder_id,
        items.len(),
        n
    );
    Ok(n)
}

pub fn remove_from_folder(
    state: &AppState,
    folder_id: String,
    asset_ids: Option<Vec<String>>,
    paths: Option<Vec<String>>,
) -> Result<usize, String> {
    let conn = state.open_db().map_err(|e| e.to_string())?;
    let mut items: Vec<(String, String)> = Vec::new();
    for id in asset_ids.unwrap_or_default() {
        let id = id.trim().to_string();
        if !id.is_empty() {
            items.push(("asset".into(), id));
        }
    }
    for path in paths.unwrap_or_default() {
        let path = path.trim().to_string();
        if path.is_empty() {
            continue;
        }
        items.push(("path".into(), path.clone()));
        let p = PathBuf::from(&path);
        if let Ok(c) = p.canonicalize() {
            let canon = c.display().to_string();
            if canon != path {
                items.push(("path".into(), canon));
            }
        }
    }
    let n = db::remove_folder_items(&conn, &folder_id, &items).map_err(|e| e.to_string())?;
    log::info!(
        target: "inspirex",
        "folder_remove id={} removed={}",
        folder_id,
        n
    );
    Ok(n)
}

pub fn list_tags(state: &AppState) -> Result<Vec<Tag>, String> {
    let conn = state.open_db().map_err(|e| e.to_string())?;
    db::list_tags(&conn).map_err(|e| e.to_string())
}

pub fn create_tag(state: &AppState, name: String, color: Option<String>) -> Result<Tag, String> {
    let name = name.trim().to_string();
    if name.is_empty() {
        return Err("标签名为空".into());
    }
    let conn = state.open_db().map_err(|e| e.to_string())?;
    if let Some(existing) = db::find_tag_by_name(&conn, &name).map_err(|e| e.to_string())? {
        return Ok(existing);
    }
    let tag = Tag {
        id: uuid::Uuid::new_v4().to_string(),
        name,
        color,
        created_at: chrono::Utc::now().to_rfc3339(),
    };
    db::insert_tag(&conn, &tag).map_err(|e| e.to_string())?;
    log::info!(target: "inspirex", "tag_create name={} color={:?}", tag.name, tag.color);
    Ok(tag)
}

pub fn set_asset_tags(state: &AppState, asset_id: String, tags: Vec<String>) -> Result<(), String> {
    let conn = state.open_db().map_err(|e| e.to_string())?;
    db::set_asset_tags(&conn, &asset_id, &tags).map_err(|e| e.to_string())
}

pub fn update_asset_meta(state: &AppState, meta: UpdateAssetMeta) -> Result<(), String> {
    if meta.asset_id.trim().is_empty() {
        return Err("assetId 为空".into());
    }
    let conn = state.open_db().map_err(|e| e.to_string())?;
    db::update_asset_meta(
        &conn,
        &meta.asset_id,
        meta.folder_id,
        meta.source_url,
        meta.notes,
    )
    .map_err(|e| e.to_string())?;
    log::info!(
        target: "inspirex",
        "update_asset_meta asset_id={}",
        meta.asset_id
    );
    Ok(())
}

pub fn search_assets(state: &AppState, query: SearchQuery) -> Result<Vec<Asset>, String> {
    let conn = state.open_db().map_err(|e| e.to_string())?;
    let limit = query.limit.unwrap_or(100);
    let text = query.text.as_deref().unwrap_or("").trim();
    let color_hex = query.color_hex.as_deref().unwrap_or("").trim();
    let tag = query.tag.as_deref().unwrap_or("").trim();

    if !color_hex.is_empty() {
        let target = parse_hex_color(color_hex).ok_or_else(|| format!("无效颜色: {color_hex}"))?;
        let candidates = db::list_assets_with_color(&conn, 2000).map_err(|e| e.to_string())?;
        let mut scored: Vec<(Asset, f32)> = candidates
            .into_iter()
            .filter_map(|(asset, hex)| {
                let c = parse_hex_color(&hex)?;
                Some((asset, color_distance(target, c)))
            })
            .collect();
        scored.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
        scored.truncate(limit as usize);
        log::info!(
            target: "inspirex",
            "search_color hex={} hits={}",
            color_hex,
            scored.len()
        );
        return Ok(scored.into_iter().map(|(a, _)| a).collect());
    }

    if !tag.is_empty() {
        log::info!(target: "inspirex", "search_tag tag={}", tag);
        return db::list_assets_by_tag(&conn, tag, limit).map_err(|e| e.to_string());
    }

    if query.use_clip && !text.is_empty() {
        let emb = state.clip.embed_text(text)?;
        let qvec = emb.vector;
        let embeddings = db::list_embeddings(&conn).map_err(|e| e.to_string())?;
        let mut scored: Vec<(String, f32)> = embeddings
            .into_iter()
            .filter(|(_, v)| v.len() == qvec.len())
            .map(|(id, vec)| (id, cosine_similarity(&qvec, &vec)))
            .collect();
        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        scored.truncate(limit as usize);
        let ids: Vec<String> = scored.into_iter().map(|(id, _)| id).collect();
        log::info!(
            target: "inspirex",
            "search_clip model={} hits={}",
            emb.model,
            ids.len()
        );
        return db::get_assets_by_ids(&conn, &ids).map_err(|e| e.to_string());
    }

    if !text.is_empty() {
        return db::fts_search(&conn, text, limit).map_err(|e| e.to_string());
    }

    db::list_assets(&conn, query.folder_id.as_deref(), limit).map_err(|e| e.to_string())
}

pub fn import_url(state: &AppState, req: IngestImportRequest) -> Result<crate::models::ImportResult, String> {
    let inline = req
        .image_base64
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .map(crate::library::decode_image_base64)
        .transpose()?;
    crate::library::import_from_url_bytes(
        state,
        &req.image_url,
        req.page_url.as_deref(),
        req.title.as_deref(),
        req.notes.as_deref(),
        req.folder_id,
        inline.as_deref(),
        req.mime.as_deref(),
        Some("plugin"),
        req.source_ref.as_deref(),
    )
}

pub fn library_root(state: &AppState) -> String {
    state.library_path.display().to_string()
}

pub fn get_browse_root(kind: Option<String>) -> String {
    let kind = parse_browse_kind(kind.as_deref());
    root_for_kind(kind).display().to_string()
}

pub fn list_index_roots_rpc(kind: Option<String>) -> Vec<IndexRoot> {
    let kind = parse_browse_kind(kind.as_deref());
    list_index_roots(kind)
}

pub fn add_index_root_rpc(kind: Option<String>, path: String) -> Result<Vec<IndexRoot>, String> {
    let kind = parse_browse_kind(kind.as_deref());
    add_index_root(kind, &PathBuf::from(path.trim()))
}

pub fn remove_index_root_rpc(kind: Option<String>, path: String) -> Result<Vec<IndexRoot>, String> {
    let kind = parse_browse_kind(kind.as_deref());
    remove_index_root(kind, &PathBuf::from(path.trim()))
}

pub fn list_local_roots() -> Vec<LocalIndexRoot> {
    list_local_index_roots()
}

pub fn add_local_root(path: String) -> Result<Vec<LocalIndexRoot>, String> {
    add_local_index_root(&PathBuf::from(path.trim()))
}

pub fn remove_local_root(path: String) -> Result<Vec<LocalIndexRoot>, String> {
    remove_local_index_root(&PathBuf::from(path.trim()))
}

pub fn get_hub_status() -> HubStatus {
    hub_status()
}

pub fn set_hub_folder(path: String) -> Result<String, String> {
    let bound = set_hub_root(&PathBuf::from(path.trim()))?;
    Ok(bound.display().to_string())
}

pub fn copy_to_hub(paths: Vec<String>, dest_dir: Option<String>) -> Result<HubCopyResult, String> {
    copy_into_hub(paths, dest_dir)
}

pub fn list_browse_dir(
    path: Option<String>,
    limit: Option<usize>,
    offset: Option<usize>,
    kind: Option<String>,
) -> Result<ExternalListing, String> {
    let kind = parse_browse_kind(kind.as_deref()).to_string();
    let target = path.filter(|p| !p.trim().is_empty()).map(PathBuf::from);
    list_browse_for_kind(target, &kind, limit.unwrap_or(72), offset.unwrap_or(0))
}

pub fn list_picker_dir(
    path: Option<String>,
    kind: Option<String>,
    include_media: Option<bool>,
) -> Result<ExternalListing, String> {
    let kind = parse_browse_kind(kind.as_deref());
    let target = path.filter(|p| !p.trim().is_empty()).map(PathBuf::from);
    browse_list_picker_dir(target.as_deref(), kind, include_media.unwrap_or(false))
}

pub fn list_finder_tags() -> Vec<FinderTag> {
    finder_tags::default_finder_tags()
}

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct AllLinkedArgs {
    tag: Option<String>,
    limit: Option<usize>,
    offset: Option<usize>,
}

pub fn list_all_linked_media(
    tag: Option<String>,
    limit: Option<usize>,
    offset: Option<usize>,
) -> Result<ExternalListing, String> {
    let media_limit = limit.unwrap_or(72);
    let media_offset = offset.unwrap_or(0);
    let tag = tag
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    if let Some(tag) = tag {
        let t0 = std::time::Instant::now();
        let mut seen = std::collections::HashSet::new();
        let mut media = Vec::new();
        for (kind, root) in linked_source_roots() {
            if !root.exists() {
                continue;
            }
            let Ok(safe) = ensure_under_kind(&root, kind) else {
                continue;
            };
            let scope = finder_tags::tag_scope_for(&safe);
            let Ok(listing) =
                finder_tags::list_by_finder_tag(&scope, &tag, 4000, 0, &root)
            else {
                continue;
            };
            for item in listing.media {
                if seen.insert(item.path.clone()) {
                    media.push(item);
                }
            }
        }
        let total = media.len();
        let page: Vec<_> = media
            .into_iter()
            .skip(media_offset)
            .take(media_limit)
            .collect();
        let truncated = media_offset + page.len() < total;
        log::info!(
            target: "inspirex",
            "all_linked_tag tag={} total={} page={} offset={} truncated={} ms={}",
            tag,
            total,
            page.len(),
            media_offset,
            truncated,
            t0.elapsed().as_millis()
        );
        return Ok(ExternalListing {
            root: "inspirex://all".into(),
            path: "inspirex://all".into(),
            parent: None,
            folders: Vec::new(),
            media: page,
            truncated,
            total_media: total,
        });
    }
    Ok(crate::browse::list_all_linked_media(media_limit, media_offset))
}

pub fn list_browse_by_tag(
    path: Option<String>,
    tag: String,
    limit: Option<usize>,
    offset: Option<usize>,
    kind: Option<String>,
) -> Result<ExternalListing, String> {
    let tag = tag.trim().to_string();
    if tag.is_empty() {
        return Err("标签名为空".into());
    }
    let kind = parse_browse_kind(kind.as_deref()).to_string();
    let root = root_for_kind(&kind);
    let target = match path {
        Some(p) if !p.trim().is_empty() => PathBuf::from(p),
        _ => root.clone(),
    };
    let media_limit = limit.unwrap_or(72);
    let media_offset = offset.unwrap_or(0);
    if (kind == "local" || kind == "volumes") && is_kind_virtual_root(&kind, &target) {
        let virtual_root = virtual_root_for(&kind);
        let mut media = Vec::new();
        for r in list_index_roots(&kind) {
            let p = PathBuf::from(&r.path);
            let Ok(safe) = ensure_under_kind(&p, &kind) else {
                continue;
            };
            let scope = finder_tags::tag_scope_for(&safe);
            let Ok(listing) = finder_tags::list_by_finder_tag(&scope, &tag, 4000, 0, &p) else {
                continue;
            };
            media.extend(listing.media);
        }
        let total = media.len();
        let page: Vec<_> = media
            .into_iter()
            .skip(media_offset)
            .take(media_limit)
            .collect();
        let truncated = media_offset + page.len() < total;
        return Ok(ExternalListing {
            root: virtual_root.into(),
            path: virtual_root.into(),
            parent: None,
            folders: Vec::new(),
            media: page,
            truncated,
            total_media: total,
        });
    }
    let safe = ensure_under_kind(&target, &kind)?;
    if kind == "local" || kind == "volumes" {
        let scope = finder_tags::tag_scope_for(&safe);
        let scope = ensure_under_kind(&scope, &kind)?;
        let bound = matching_index_root(&kind, &safe).unwrap_or_else(|| safe.clone());
        let mut listing =
            finder_tags::list_by_finder_tag(&scope, &tag, media_limit, media_offset, &bound)?;
        listing.root = virtual_root_for(&kind).to_string();
        return Ok(listing);
    }
    let scope = finder_tags::tag_scope_for(&safe);
    let scope = ensure_under_kind(&scope, &kind)?;
    finder_tags::list_by_finder_tag(&scope, &tag, media_limit, media_offset, &root)
}

pub fn get_finder_tags(path: String) -> Result<Vec<FinderTagValue>, String> {
    let p = PathBuf::from(&path);
    let safe = ensure_readable(&p)?;
    finder_tags::get_path_tags(&safe)
}

pub fn apply_finder_tag(
    paths: Vec<String>,
    tag: String,
    color: Option<u8>,
    remove: Option<bool>,
) -> Result<usize, String> {
    let tag = tag.trim().to_string();
    if tag.is_empty() {
        return Err("标签名为空".into());
    }
    if paths.is_empty() {
        return Err("未选择文件".into());
    }
    let color = color.unwrap_or_else(|| {
        finder_tags::FINDER_PALETTE
            .iter()
            .find(|(n, _, _)| *n == tag)
            .map(|(_, c, _)| *c)
            .unwrap_or(0)
    });
    finder_tags::ensure_and_apply(&paths, &tag, color, remove.unwrap_or(false))
}

pub fn get_image_thumb(state: &AppState, path: String, max_side: Option<u32>) -> Result<String, String> {
    let p = PathBuf::from(&path);
    let _ = ensure_readable(&p)?;
    log::info!(
        target: "inspirex",
        "thumb_req path={} video={} max_side={}",
        p.display(),
        crate::pipeline::is_video(&p),
        max_side.unwrap_or(240)
    );
    let thumb = crate::thumb::ensure_image_thumb(&p, &state.library_path, max_side.unwrap_or(240))?;
    Ok(thumb.display().to_string())
}

pub fn list_sources(state: &AppState) -> Result<Vec<Source>, String> {
    let conn = state.open_db().map_err(|e| e.to_string())?;
    db::list_sources(&conn).map_err(|e| e.to_string())
}

pub fn delete_source(state: &AppState, source_id: String) -> Result<bool, String> {
    let conn = state.open_db().map_err(|e| e.to_string())?;
    let ok = db::delete_source(&conn, &source_id).map_err(|e| e.to_string())?;
    log::info!(target: "inspirex", "source_delete id={} ok={}", source_id, ok);
    Ok(ok)
}

pub fn set_source_enabled(state: &AppState, source_id: String, enabled: bool) -> Result<(), String> {
    let conn = state.open_db().map_err(|e| e.to_string())?;
    db::set_source_enabled(&conn, &source_id, enabled).map_err(|e| e.to_string())?;
    log::info!(
        target: "inspirex",
        "source_enabled id={} enabled={}",
        source_id,
        enabled
    );
    Ok(())
}

pub fn pull_source(state: &AppState, source_id: String) -> Result<crate::models::ImportResult, String> {
    crate::subscribe::pull_source(state, &source_id)
}

pub fn generate_images(state: &AppState, req: GenerateRequest) -> Result<GenJob, String> {
    crate::imagegen::generate(state, req)
}

pub fn import_gen_job(
    state: &AppState,
    job_id: String,
    folder_id: Option<String>,
) -> Result<GenJob, String> {
    crate::imagegen::import_job(state, &job_id, folder_id)
}

pub fn list_gen_jobs(state: &AppState, limit: Option<i64>) -> Result<Vec<GenJob>, String> {
    let conn = state.open_db().map_err(|e| e.to_string())?;
    db::list_gen_jobs(&conn, limit.unwrap_or(50)).map_err(|e| e.to_string())
}

fn normalize_preset(name: &str, body: &str) -> Result<(String, String), String> {
    let body = body.trim().to_string();
    if body.is_empty() {
        return Err("prompt 不能为空".into());
    }
    let name = if name.trim().is_empty() {
        body.chars().take(24).collect()
    } else {
        name.trim().to_string()
    };
    Ok((name, body))
}

fn normalize_image_path(path: Option<String>) -> Option<String> {
    path.map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

fn normalize_category_id(conn: &rusqlite::Connection, id: Option<String>) -> Result<Option<String>, String> {
    let id = id.map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
    let Some(id) = id else {
        return Ok(None);
    };
    if db::get_prompt_category(conn, &id)
        .map_err(|e| e.to_string())?
        .is_none()
    {
        return Err(format!("分类不存在: {id}"));
    }
    Ok(Some(id))
}

pub fn list_prompt_presets(state: &AppState) -> Result<Vec<PromptPreset>, String> {
    let conn = state.open_db().map_err(|e| e.to_string())?;
    db::list_prompt_presets(&conn).map_err(|e| e.to_string())
}

pub fn mine_prompt_from_image(state: &AppState, path: String) -> Result<MinedPrompt, String> {
    crate::promptmine::mine_from_image(state, path)
}

pub fn create_prompt_preset(state: &AppState, req: CreatePromptPreset) -> Result<PromptPreset, String> {
    let (name, body) = normalize_preset(&req.name, &req.body)?;
    let conn = state.open_db().map_err(|e| e.to_string())?;
    let now = chrono::Utc::now().to_rfc3339();
    let preset = PromptPreset {
        id: uuid::Uuid::new_v4().to_string(),
        name,
        body,
        image_path: normalize_image_path(req.image_path),
        category_id: normalize_category_id(&conn, req.category_id)?,
        created_at: now.clone(),
        updated_at: now,
        source_kind: "local".into(),
        source_uid: None,
    };
    db::insert_prompt_preset(&conn, &preset).map_err(|e| e.to_string())?;
    log::info!(
        target: "inspirex",
        "prompt_preset_create id={} name_len={} body_len={}",
        preset.id,
        preset.name.len(),
        preset.body.len()
    );
    Ok(preset)
}

pub fn update_prompt_preset(state: &AppState, req: UpdatePromptPreset) -> Result<PromptPreset, String> {
    let id = req.id.trim();
    if id.is_empty() {
        return Err("preset_id 不能为空".into());
    }
    let (name, body) = normalize_preset(&req.name, &req.body)?;
    let conn = state.open_db().map_err(|e| e.to_string())?;
    let mut preset = db::get_prompt_preset(&conn, id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("prompt 不存在: {id}"))?;
    preset.name = name;
    preset.body = body;
    preset.image_path = normalize_image_path(req.image_path);
    preset.category_id = normalize_category_id(&conn, req.category_id)?;
    preset.updated_at = chrono::Utc::now().to_rfc3339();
    db::update_prompt_preset(&conn, &preset).map_err(|e| e.to_string())?;
    log::info!(
        target: "inspirex",
        "prompt_preset_update id={} body_len={}",
        preset.id,
        preset.body.len()
    );
    Ok(preset)
}

pub fn delete_prompt_preset(state: &AppState, preset_id: String) -> Result<bool, String> {
    let conn = state.open_db().map_err(|e| e.to_string())?;
    let ok = db::delete_prompt_preset(&conn, &preset_id).map_err(|e| e.to_string())?;
    log::info!(
        target: "inspirex",
        "prompt_preset_delete id={} ok={}",
        preset_id,
        ok
    );
    Ok(ok)
}

pub fn list_prompt_categories(state: &AppState) -> Result<Vec<PromptCategory>, String> {
    let conn = state.open_db().map_err(|e| e.to_string())?;
    db::list_prompt_categories(&conn).map_err(|e| e.to_string())
}

pub fn create_prompt_category(state: &AppState, name: String) -> Result<PromptCategory, String> {
    let name = name.trim().to_string();
    if name.is_empty() {
        return Err("分类名称不能为空".into());
    }
    let cat = PromptCategory {
        id: uuid::Uuid::new_v4().to_string(),
        name,
        created_at: chrono::Utc::now().to_rfc3339(),
        item_count: 0,
        source_key: None,
    };
    let conn = state.open_db().map_err(|e| e.to_string())?;
    db::insert_prompt_category(&conn, &cat).map_err(|e| e.to_string())?;
    log::info!(
        target: "inspirex",
        "prompt_category_create id={} name={}",
        cat.id,
        cat.name
    );
    Ok(cat)
}

pub fn delete_prompt_category(state: &AppState, category_id: String) -> Result<bool, String> {
    let conn = state.open_db().map_err(|e| e.to_string())?;
    if let Some(cat) = db::get_prompt_category(&conn, &category_id).map_err(|e| e.to_string())? {
        if cat
            .source_key
            .as_deref()
            .unwrap_or("")
            .starts_with("vprompt:")
        {
            return Err("片库分类由同步维护，不能手动删除".into());
        }
    }
    let ok = db::delete_prompt_category(&conn, &category_id).map_err(|e| e.to_string())?;
    log::info!(
        target: "inspirex",
        "prompt_category_delete id={} ok={}",
        category_id,
        ok
    );
    Ok(ok)
}

pub fn get_vprompt_status() -> crate::vprompt::VpromptStatus {
    crate::vprompt::status()
}

pub fn set_vprompt_store_root(path: String) -> Result<crate::vprompt::VpromptStatus, String> {
    crate::vprompt::set_root(path)
}

pub fn sync_vprompt_prompts(
    state: &AppState,
    root: Option<String>,
) -> Result<crate::vprompt::VpromptSyncResult, String> {
    crate::vprompt::sync_prompts(state, root)
}

pub fn get_ai_settings(state: &AppState) -> crate::aisettings::AiServicesView {
    let svc = state.ai.read().clone();
    crate::aisettings::view_of(&svc, &state.ai_config_path)
}

pub fn set_ai_settings(
    state: &AppState,
    patch: crate::aisettings::AiServicesPatch,
) -> Result<crate::aisettings::AiServicesView, String> {
    let mut svc = state.ai.read().clone();
    crate::aisettings::apply_patch(&mut svc, patch);
    crate::aisettings::save(&state.ai_config_path, &svc)?;
    *state.ai.write() = svc.clone();
    log::info!(
        target: "inspirex",
        "ai_settings_save path={} text={} image={} video={}",
        state.ai_config_path.display(),
        crate::aisettings::effective_of(&svc.text.provider, &svc.text.api_key),
        crate::aisettings::effective_of(&svc.image.provider, &svc.image.api_key),
        crate::aisettings::effective_of(&svc.video.provider, &svc.video.api_key)
    );
    Ok(crate::aisettings::view_of(&svc, &state.ai_config_path))
}

/// 浏览器与其它 HTTP 客户端的统一入口。
pub fn dispatch(state: &AppState, cmd: &str, args: Value) -> Result<Value, String> {
    log::info!(target: "inspirex", "rpc_dispatch cmd={}", cmd);
    match cmd {
        "get_library_info" => json_ok(library_info(state)?),
        "list_assets" => {
            let a: ListAssetsArgs = parse(&args)?;
            json_ok(list_assets(state, a.folder_id, a.source_kind, a.source_ref, a.limit)?)
        }
        "list_folders" => json_ok(list_folders(state)?),
        "create_folder" => {
            let a: NameParentArgs = parse(&args)?;
            json_ok(crate::library::create_folder(state, a.name, a.parent_id)?)
        }
        "delete_folder" => {
            let a: FolderIdArgs = parse(&args)?;
            json_ok(delete_folder(state, a.folder_id)?)
        }
        "list_folder_contents" => {
            let a: FolderIdArgs = parse(&args)?;
            json_ok(list_folder_contents(state, a.folder_id)?)
        }
        "add_to_folder" => {
            let a: FolderItemsArgs = parse(&args)?;
            json_ok(add_to_folder(state, a.folder_id, a.asset_ids, a.paths)?)
        }
        "remove_from_folder" => {
            let a: FolderItemsArgs = parse(&args)?;
            json_ok(remove_from_folder(state, a.folder_id, a.asset_ids, a.paths)?)
        }
        "list_tags" => json_ok(list_tags(state)?),
        "create_tag" => {
            let a: CreateTagArgs = parse(&args)?;
            json_ok(create_tag(state, a.name, a.color)?)
        }
        "set_asset_tags" => {
            let a: SetAssetTagsArgs = parse(&args)?;
            set_asset_tags(state, a.asset_id, a.tags)?;
            Ok(Value::Null)
        }
        "import_files" => {
            let a: ImportFilesArgs = parse(&args)?;
            json_ok(import_paths(state, a.paths, a.folder_id)?)
        }
        "import_url" => {
            let a: ImportUrlArgs = parse(&args)?;
            let req = a.req.unwrap_or_default();
            json_ok(import_url(state, req)?)
        }
        "delete_asset_cmd" => {
            let a: DeleteAssetArgs = parse(&args)?;
            json_ok(delete_asset(state, &a.asset_id, a.delete_files.unwrap_or(true))?)
        }
        "update_asset_meta" => {
            let a: MetaArgs = parse(&args)?;
            let meta = a.meta.unwrap_or_default();
            update_asset_meta(state, meta)?;
            Ok(Value::Null)
        }
        "get_asset_abs_path" => {
            let a: AssetIdArgs = parse(&args)?;
            json_ok(asset_abs_path(state, &a.asset_id)?)
        }
        "search_assets" => {
            let a: SearchArgs = parse(&args)?;
            json_ok(search_assets(state, a.query)?)
        }
        "library_root" => json_ok(library_root(state)),
        "get_browse_root" => {
            let a: KindArgs = parse(&args)?;
            json_ok(get_browse_root(a.kind))
        }
        "list_index_roots" => {
            let a: KindArgs = parse(&args)?;
            json_ok(list_index_roots_rpc(a.kind))
        }
        "add_index_root" => {
            let a: KindPathArgs = parse(&args)?;
            json_ok(add_index_root_rpc(a.kind, a.path)?)
        }
        "remove_index_root" => {
            let a: KindPathArgs = parse(&args)?;
            json_ok(remove_index_root_rpc(a.kind, a.path)?)
        }
        "list_local_roots" => json_ok(list_local_roots()),
        "add_local_root" => {
            let a: PathArgs = parse(&args)?;
            json_ok(add_local_root(a.path)?)
        }
        "remove_local_root" => {
            let a: PathArgs = parse(&args)?;
            json_ok(remove_local_root(a.path)?)
        }
        "get_hub_status" => json_ok(get_hub_status()),
        "set_hub_folder" => {
            let a: PathArgs = parse(&args)?;
            json_ok(set_hub_folder(a.path)?)
        }
        "copy_to_hub" => {
            let a: CopyHubArgs = parse(&args)?;
            json_ok(copy_to_hub(a.paths, a.dest_dir)?)
        }
        "list_browse_dir" => {
            let a: BrowseDirArgs = parse(&args)?;
            let listing = list_browse_dir(a.path, a.limit, a.offset, a.kind)?;
            crate::thumb::spawn_warmup(
                state.library_path.clone(),
                listing.media.iter().map(|m| m.path.clone()).collect(),
                240,
            );
            json_ok(listing)
        }
        "list_picker_dir" => {
            let a: PickerDirArgs = parse(&args)?;
            json_ok(list_picker_dir(a.path, a.kind, a.include_media)?)
        }
        "list_finder_tags" => json_ok(list_finder_tags()),
        "list_browse_by_tag" => {
            let a: BrowseByTagArgs = parse(&args)?;
            let listing = list_browse_by_tag(a.path, a.tag, a.limit, a.offset, a.kind)?;
            crate::thumb::spawn_warmup(
                state.library_path.clone(),
                listing.media.iter().map(|m| m.path.clone()).collect(),
                240,
            );
            json_ok(listing)
        }
        "list_all_linked_media" => {
            let a: AllLinkedArgs = parse(&args)?;
            let listing = list_all_linked_media(a.tag, a.limit, a.offset)?;
            crate::thumb::spawn_warmup(
                state.library_path.clone(),
                listing.media.iter().map(|m| m.path.clone()).collect(),
                240,
            );
            json_ok(listing)
        }
        "get_finder_tags" => {
            let a: PathArgs = parse(&args)?;
            json_ok(get_finder_tags(a.path)?)
        }
        "apply_finder_tag" => {
            let a: ApplyFinderTagArgs = parse(&args)?;
            json_ok(apply_finder_tag(a.paths, a.tag, a.color, a.remove)?)
        }
        "get_image_thumb" => {
            let a: ThumbArgs = parse(&args)?;
            json_ok(get_image_thumb(state, a.path, a.max_side)?)
        }
        "list_sources" => json_ok(list_sources(state)?),
        "create_source" => {
            let a: CreateSourceArgs = parse(&args)?;
            json_ok(crate::subscribe::create_source(state, a.req.unwrap_or_default())?)
        }
        "delete_source" => {
            let a: SourceIdArgs = parse(&args)?;
            json_ok(delete_source(state, a.source_id)?)
        }
        "set_source_enabled" => {
            let a: SourceEnabledArgs = parse(&args)?;
            set_source_enabled(state, a.source_id, a.enabled)?;
            Ok(Value::Null)
        }
        "report_source_run" => {
            let a: SourceRunArgs = parse(&args)?;
            crate::subscribe::report_source_run(state, &a.source_id, &a.summary)?;
            Ok(Value::Null)
        }
        "pull_source" => {
            let a: SourceIdArgs = parse(&args)?;
            json_ok(pull_source(state, a.source_id)?)
        }
        "generate_images" => {
            let a: GenerateArgs = parse(&args)?;
            json_ok(generate_images(state, a.req.unwrap_or_default())?)
        }
        "import_gen_job" => {
            let a: ImportGenArgs = parse(&args)?;
            json_ok(import_gen_job(state, a.job_id, a.folder_id)?)
        }
        "list_gen_jobs" => {
            let a: LimitArgs = parse(&args)?;
            json_ok(list_gen_jobs(state, a.limit)?)
        }
        "list_prompt_presets" => json_ok(list_prompt_presets(state)?),
        "mine_prompt_from_image" => {
            let a: PathArgs = parse(&args)?;
            json_ok(mine_prompt_from_image(state, a.path)?)
        }
        "create_prompt_preset" => {
            let a: CreatePromptPresetArgs = parse(&args)?;
            json_ok(create_prompt_preset(state, a.req.unwrap_or_default())?)
        }
        "update_prompt_preset" => {
            let a: UpdatePromptPresetArgs = parse(&args)?;
            json_ok(update_prompt_preset(state, a.req.unwrap_or_default())?)
        }
        "delete_prompt_preset" => {
            let a: PromptPresetIdArgs = parse(&args)?;
            json_ok(delete_prompt_preset(state, a.preset_id)?)
        }
        "list_prompt_categories" => json_ok(list_prompt_categories(state)?),
        "create_prompt_category" => {
            let a: CreatePromptCategoryArgs = parse(&args)?;
            json_ok(create_prompt_category(state, a.name)?)
        }
        "delete_prompt_category" => {
            let a: PromptCategoryIdArgs = parse(&args)?;
            json_ok(delete_prompt_category(state, a.category_id)?)
        }
        "get_vprompt_status" => json_ok(get_vprompt_status()),
        "set_vprompt_store_root" => {
            let a: PathArgs = parse(&args)?;
            json_ok(set_vprompt_store_root(a.path)?)
        }
        "sync_vprompt_prompts" => {
            let a: VpromptSyncArgs = parse(&args)?;
            json_ok(sync_vprompt_prompts(state, a.root.or(a.path))?)
        }
        "get_ai_settings" => json_ok(get_ai_settings(state)),
        "set_ai_settings" => {
            let a: crate::aisettings::AiServicesPatch = parse(&args)?;
            json_ok(set_ai_settings(state, a)?)
        }
        other => Err(format!("未知命令: {other}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::AppState;

    #[test]
    fn dispatch_library_info_create_folder_and_tag() {
        let dir = tempfile::tempdir().unwrap();
        let state = AppState::for_test(dir.path().to_path_buf());
        state.ensure_dirs().unwrap();
        let info = dispatch(&state, "get_library_info", json!({})).unwrap();
        assert_eq!(info["assetCount"], 0);
        assert!(info["ingestPort"].as_u64().is_some());

        let folder = dispatch(&state, "create_folder", json!({"name": "素材"})).unwrap();
        assert_eq!(folder["name"], "素材");
        assert!(folder["id"].as_str().unwrap().len() > 8);

        let tag = dispatch(
            &state,
            "create_tag",
            json!({"name": "红", "color": "#FF5F57"}),
        )
        .unwrap();
        assert_eq!(tag["name"], "红");

        let folders = dispatch(&state, "list_folders", json!({})).unwrap();
        assert_eq!(folders.as_array().unwrap().len(), 1);

        let err = dispatch(&state, "no_such_cmd", json!({})).unwrap_err();
        assert!(err.contains("未知命令"), "{err}");
    }

    #[test]
    fn dispatch_prompt_preset_crud() {
        let dir = tempfile::tempdir().unwrap();
        let state = AppState::for_test(dir.path().to_path_buf());
        state.ensure_dirs().unwrap();
        let created = dispatch(
            &state,
            "create_prompt_preset",
            json!({"req": {"name": "静物", "body": "暖色陶罐静物", "imagePath": "/tmp/a.jpg"}}),
        )
        .unwrap();
        assert_eq!(created["name"], "静物");
        assert_eq!(created["imagePath"], "/tmp/a.jpg");
        let id = created["id"].as_str().unwrap().to_string();
        let listed = dispatch(&state, "list_prompt_presets", json!({})).unwrap();
        assert_eq!(listed.as_array().unwrap().len(), 1);
        dispatch(
            &state,
            "update_prompt_preset",
            json!({"req": {"id": id.clone(), "name": "静物", "body": "暖色陶罐", "imagePath": "/tmp/b.jpg"}}),
        )
        .unwrap();
        let ok = dispatch(
            &state,
            "delete_prompt_preset",
            json!({"presetId": id}),
        )
        .unwrap();
        assert_eq!(ok, serde_json::json!(true));
        let empty = dispatch(&state, "list_prompt_presets", json!({})).unwrap();
        assert!(empty.as_array().unwrap().is_empty());
    }

    #[test]
    fn dispatch_prompt_category_and_bind() {
        let dir = tempfile::tempdir().unwrap();
        let state = AppState::for_test(dir.path().to_path_buf());
        state.ensure_dirs().unwrap();
        let cat = dispatch(
            &state,
            "create_prompt_category",
            json!({"name": "静物"}),
        )
        .unwrap();
        assert_eq!(cat["name"], "静物");
        let cat_id = cat["id"].as_str().unwrap().to_string();
        let created = dispatch(
            &state,
            "create_prompt_preset",
            json!({"req": {"name": "暖色", "body": "陶罐", "categoryId": cat_id}}),
        )
        .unwrap();
        assert_eq!(created["categoryId"], cat_id);
        let listed = dispatch(&state, "list_prompt_categories", json!({})).unwrap();
        assert_eq!(listed.as_array().unwrap()[0]["itemCount"], 1);
        dispatch(
            &state,
            "delete_prompt_category",
            json!({"categoryId": cat_id}),
        )
        .unwrap();
        let presets = dispatch(&state, "list_prompt_presets", json!({})).unwrap();
        assert!(presets[0]["categoryId"].is_null());
    }

    #[test]
    fn dispatch_list_picker_dir() {
        let dir = tempfile::tempdir().unwrap();
        let state = AppState::for_test(dir.path().to_path_buf());
        let listing = dispatch(&state, "list_picker_dir", json!({"kind": "local"})).unwrap();
        assert!(listing["path"].as_str().unwrap().len() > 1);
        assert!(listing["folders"].is_array());
        assert!(listing["media"].as_array().unwrap().is_empty());
    }

    #[test]
    fn dispatch_list_all_linked_media() {
        let dir = tempfile::tempdir().unwrap();
        let state = AppState::for_test(dir.path().join("Library"));
        let listing = dispatch(&state, "list_all_linked_media", json!({"limit": 10})).unwrap();
        assert!(listing["media"].is_array());
        assert_eq!(listing["path"], "inspirex://all");
    }

    #[test]
    fn dispatch_search_args_camel_case() {
        let dir = tempfile::tempdir().unwrap();
        let state = AppState::for_test(dir.path().to_path_buf());
        state.ensure_dirs().unwrap();
        let list = dispatch(
            &state,
            "search_assets",
            json!({"query": {"text": "none", "useClip": true, "limit": 10}}),
        )
        .unwrap();
        assert!(list.as_array().unwrap().is_empty());
    }

    #[test]
    fn dispatch_mine_prompt_from_image_mock() {
        let dir = tempfile::tempdir().unwrap();
        let state = AppState::for_test(dir.path().to_path_buf());
        state.ensure_dirs().unwrap();
        let mut img = image::RgbImage::new(8, 8);
        for p in img.pixels_mut() {
            *p = image::Rgb([180, 60, 40]);
        }
        let png = dir.path().join("ref.png");
        img.save(&png).unwrap();
        let mined = dispatch(
            &state,
            "mine_prompt_from_image",
            json!({"path": png.display().to_string()}),
        )
        .unwrap();
        assert_eq!(mined["provider"], "mock");
        assert!(!mined["name"].as_str().unwrap().is_empty());
        assert!(mined["body"].as_str().unwrap().contains("mock"));
    }

    #[test]
    fn dispatch_ai_settings_save_and_mask() {
        let dir = tempfile::tempdir().unwrap();
        let state = AppState::for_test(dir.path().to_path_buf());
        let got = dispatch(&state, "get_ai_settings", json!({})).unwrap();
        assert_eq!(got["image"]["provider"], "mock");
        assert_eq!(got["image"]["apiKeySet"], false);

        let saved = dispatch(
            &state,
            "set_ai_settings",
            json!({
                "image": {
                    "provider": "minimax",
                    "baseUrl": "https://api.minimaxi.com",
                    "apiKey": "sk-live",
                    "model": "image-01"
                }
            }),
        )
        .unwrap();
        assert_eq!(saved["image"]["provider"], "minimax");
        assert_eq!(saved["image"]["apiKeySet"], true);
        assert_eq!(saved["image"]["effective"], "minimax");
        assert!(saved["image"].get("apiKey").is_none());

        let keep = dispatch(
            &state,
            "set_ai_settings",
            json!({"image": {"provider": "minimax", "apiKey": ""}}),
        )
        .unwrap();
        assert_eq!(keep["image"]["apiKeySet"], true);
        assert_eq!(state.ai_image().api_key, "sk-live");
    }

    #[test]
    fn dispatch_vprompt_sync_from_fixture() {
        let store = tempfile::tempdir().unwrap();
        let cat = store.path().join("图片提示词").join("Demo分类");
        std::fs::create_dir_all(&cat).unwrap();
        std::fs::write(
            cat.join("样本集合.md"),
            "## 1. Demo still\n\n**提示词：**\n暖色陶罐\n\n---\n",
        )
        .unwrap();
        let lib = tempfile::tempdir().unwrap();
        let cfg = lib.path().join("vprompt.json");
        let prev = std::env::var("INSPIREX_VPROMPT_CONFIG").ok();
        std::env::set_var("INSPIREX_VPROMPT_CONFIG", &cfg);
        let state = AppState::for_test(lib.path().join("Library"));
        state.ensure_dirs().unwrap();
        let synced = dispatch(
            &state,
            "sync_vprompt_prompts",
            json!({"root": store.path().display().to_string()}),
        )
        .unwrap();
        assert_eq!(synced["inserted"], 1);
        assert_eq!(synced["total"], 1);
        let listed = dispatch(&state, "list_prompt_presets", json!({})).unwrap();
        assert_eq!(listed.as_array().unwrap().len(), 1);
        match prev {
            Some(v) => std::env::set_var("INSPIREX_VPROMPT_CONFIG", v),
            None => std::env::remove_var("INSPIREX_VPROMPT_CONFIG"),
        }
    }
}
