use crate::browse::{
    add_local_index_root, copy_into_hub, ensure_readable, ensure_under_kind, hub_status,
    is_local_virtual_root, item_from_path, list_browse_for_kind, list_local_index_roots,
    matching_local_root, parse_browse_kind, remove_local_index_root, root_for_kind, set_hub_root,
    ExternalItem, ExternalKind, ExternalListing, HubCopyResult, HubStatus, LocalIndexRoot,
    LOCAL_VIRTUAL_ROOT,
};
use crate::finder_tags::{self, FinderTag, FinderTagValue};
use crate::db;
use crate::library::{asset_abs_path, delete_asset, import_from_url, import_paths, library_info, AppState};
use crate::models::{
    Asset, CreateSource, DeleteAssetResult, Folder, GenerateRequest, GenJob, IngestImportRequest,
    LibraryInfo, SearchQuery, Source, Tag, UpdateAssetMeta,
};
use crate::pipeline::{color_distance, cosine_similarity, parse_hex_color};
use std::path::PathBuf;
use std::sync::Arc;
use tauri::State;

#[tauri::command]
pub fn get_library_info(state: State<'_, Arc<AppState>>) -> Result<LibraryInfo, String> {
    library_info(&state)
}

#[tauri::command]
pub fn list_assets(
    state: State<'_, Arc<AppState>>,
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

#[tauri::command]
pub fn list_folders(state: State<'_, Arc<AppState>>) -> Result<Vec<Folder>, String> {
    let conn = state.open_db().map_err(|e| e.to_string())?;
    db::list_folders(&conn).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn create_folder(
    state: State<'_, Arc<AppState>>,
    name: String,
    parent_id: Option<String>,
) -> Result<Folder, String> {
    crate::library::create_folder(&state, name, parent_id)
}

#[tauri::command]
pub fn delete_folder(
    state: State<'_, Arc<AppState>>,
    folder_id: String,
) -> Result<(usize, usize), String> {
    let conn = state.open_db().map_err(|e| e.to_string())?;
    db::delete_folder(&conn, &folder_id).map_err(|e| e.to_string())
}

#[derive(serde::Serialize)]
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

/// 虚拟文件夹内容：库内素材 + 外置/本机路径索引（不复制原文件）。
#[tauri::command]
pub fn list_folder_contents(
    state: State<'_, Arc<AppState>>,
    folder_id: String,
) -> Result<FolderContents, String> {
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
                let p = std::path::PathBuf::from(&row.item_ref);
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

/// 把已选素材/路径虚拟存入文件夹（只写索引，不复制文件）。
#[tauri::command]
pub fn add_to_folder(
    state: State<'_, Arc<AppState>>,
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
        let p = std::path::PathBuf::from(&path);
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

#[tauri::command]
pub fn remove_from_folder(
    state: State<'_, Arc<AppState>>,
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
        let p = std::path::PathBuf::from(&path);
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

#[tauri::command]
pub fn list_tags(state: State<'_, Arc<AppState>>) -> Result<Vec<Tag>, String> {
    let conn = state.open_db().map_err(|e| e.to_string())?;
    db::list_tags(&conn).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn create_tag(
    state: State<'_, Arc<AppState>>,
    name: String,
    color: Option<String>,
) -> Result<Tag, String> {
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

#[tauri::command]
pub fn set_asset_tags(
    state: State<'_, Arc<AppState>>,
    asset_id: String,
    tags: Vec<String>,
) -> Result<(), String> {
    let conn = state.open_db().map_err(|e| e.to_string())?;
    db::set_asset_tags(&conn, &asset_id, &tags).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn import_files(
    state: State<'_, Arc<AppState>>,
    paths: Vec<String>,
    folder_id: Option<String>,
) -> Result<crate::models::ImportResult, String> {
    import_paths(&state, paths, folder_id)
}

#[tauri::command]
pub fn delete_asset_cmd(
    state: State<'_, Arc<AppState>>,
    asset_id: String,
    delete_files: Option<bool>,
) -> Result<DeleteAssetResult, String> {
    delete_asset(&state, &asset_id, delete_files.unwrap_or(true))
}

#[tauri::command]
pub fn update_asset_meta(
    state: State<'_, Arc<AppState>>,
    meta: UpdateAssetMeta,
) -> Result<(), String> {
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

#[tauri::command]
pub fn get_asset_abs_path(
    state: State<'_, Arc<AppState>>,
    asset_id: String,
) -> Result<String, String> {
    asset_abs_path(&state, &asset_id)
}

#[tauri::command]
pub fn search_assets(
    state: State<'_, Arc<AppState>>,
    query: SearchQuery,
) -> Result<Vec<Asset>, String> {
    let conn = state.open_db().map_err(|e| e.to_string())?;
    let limit = query.limit.unwrap_or(100);
    let text = query.text.as_deref().unwrap_or("").trim();
    let color_hex = query.color_hex.as_deref().unwrap_or("").trim();
    let tag = query.tag.as_deref().unwrap_or("").trim();

    if !color_hex.is_empty() {
        let target = parse_hex_color(color_hex)
            .ok_or_else(|| format!("无效颜色: {color_hex}"))?;
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

#[tauri::command]
pub fn import_url(
    state: State<'_, Arc<AppState>>,
    req: IngestImportRequest,
) -> Result<crate::models::ImportResult, String> {
    import_from_url(
        &state,
        &req.image_url,
        req.page_url.as_deref(),
        req.title.as_deref(),
        req.notes.as_deref(),
        req.folder_id,
    )
}

#[tauri::command]
pub fn library_root(state: State<'_, Arc<AppState>>) -> String {
    state.library_path.display().to_string()
}

#[tauri::command]
pub fn get_browse_root(kind: Option<String>) -> String {
    let kind = parse_browse_kind(kind.as_deref());
    root_for_kind(kind).display().to_string()
}

#[tauri::command]
pub fn list_local_roots() -> Vec<LocalIndexRoot> {
    list_local_index_roots()
}

#[tauri::command]
pub fn add_local_root(path: String) -> Result<Vec<LocalIndexRoot>, String> {
    add_local_index_root(&PathBuf::from(path.trim()))
}

#[tauri::command]
pub fn remove_local_root(path: String) -> Result<Vec<LocalIndexRoot>, String> {
    remove_local_index_root(&PathBuf::from(path.trim()))
}

#[tauri::command]
pub fn get_hub_status() -> HubStatus {
    hub_status()
}

#[tauri::command]
pub fn set_hub_folder(path: String) -> Result<String, String> {
    let bound = set_hub_root(&PathBuf::from(path.trim()))?;
    Ok(bound.display().to_string())
}

#[tauri::command]
pub fn copy_to_hub(
    paths: Vec<String>,
    dest_dir: Option<String>,
) -> Result<HubCopyResult, String> {
    copy_into_hub(paths, dest_dir)
}

#[tauri::command]
pub async fn list_browse_dir(
    path: Option<String>,
    limit: Option<usize>,
    offset: Option<usize>,
    kind: Option<String>,
) -> Result<ExternalListing, String> {
    let kind = parse_browse_kind(kind.as_deref()).to_string();
    let target = path.filter(|p| !p.trim().is_empty()).map(PathBuf::from);
    let media_limit = limit.unwrap_or(72);
    let media_offset = offset.unwrap_or(0);
    tauri::async_runtime::spawn_blocking(move || {
        list_browse_for_kind(target, &kind, media_limit, media_offset)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub fn list_finder_tags() -> Vec<FinderTag> {
    finder_tags::default_finder_tags()
}

#[tauri::command]
pub async fn list_browse_by_tag(
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
        Some(p) if !p.trim().is_empty() => std::path::PathBuf::from(p),
        _ => root.clone(),
    };
    let media_limit = limit.unwrap_or(72);
    let media_offset = offset.unwrap_or(0);
    tauri::async_runtime::spawn_blocking(move || {
        if kind == "local" && is_local_virtual_root(&target) {
            let mut media = Vec::new();
            for r in list_local_index_roots() {
                let p = PathBuf::from(&r.path);
                let Ok(safe) = ensure_under_kind(&p, "local") else {
                    continue;
                };
                let scope = finder_tags::tag_scope_for(&safe);
                let Ok(listing) =
                    finder_tags::list_by_finder_tag(&scope, &tag, 4000, 0, &p)
                else {
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
                root: LOCAL_VIRTUAL_ROOT.into(),
                path: LOCAL_VIRTUAL_ROOT.into(),
                parent: None,
                folders: Vec::new(),
                media: page,
                truncated,
                total_media: total,
            });
        }
        let safe = ensure_under_kind(&target, &kind)?;
        if kind == "local" {
            let scope = finder_tags::tag_scope_for(&safe);
            let scope = ensure_under_kind(&scope, "local")?;
            let bound = matching_local_root(&safe).unwrap_or_else(|| safe.clone());
            let mut listing =
                finder_tags::list_by_finder_tag(&scope, &tag, media_limit, media_offset, &bound)?;
            listing.root = LOCAL_VIRTUAL_ROOT.to_string();
            return Ok(listing);
        }
        let scope = finder_tags::tag_scope_for(&safe);
        let scope = ensure_under_kind(&scope, &kind)?;
        finder_tags::list_by_finder_tag(&scope, &tag, media_limit, media_offset, &root)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub fn get_finder_tags(path: String) -> Result<Vec<FinderTagValue>, String> {
    let p = std::path::PathBuf::from(&path);
    let safe = ensure_readable(&p)?;
    finder_tags::get_path_tags(&safe)
}

#[tauri::command]
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

#[tauri::command]
pub async fn get_image_thumb(
    state: State<'_, Arc<AppState>>,
    path: String,
    max_side: Option<u32>,
) -> Result<String, String> {
    let library = state.library_path.clone();
    let side = max_side.unwrap_or(240);
    tauri::async_runtime::spawn_blocking(move || {
        let p = PathBuf::from(&path);
        let _ = ensure_readable(&p)?;
        let thumb = crate::thumb::ensure_image_thumb(&p, &library, side)?;
        Ok(thumb.display().to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub fn list_sources(state: State<'_, Arc<AppState>>) -> Result<Vec<Source>, String> {
    let conn = state.open_db().map_err(|e| e.to_string())?;
    db::list_sources(&conn).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn create_source(
    state: State<'_, Arc<AppState>>,
    req: CreateSource,
) -> Result<Source, String> {
    crate::subscribe::create_source(&state, req)
}

#[tauri::command]
pub fn delete_source(state: State<'_, Arc<AppState>>, source_id: String) -> Result<bool, String> {
    let conn = state.open_db().map_err(|e| e.to_string())?;
    let ok = db::delete_source(&conn, &source_id).map_err(|e| e.to_string())?;
    log::info!(target: "inspirex", "source_delete id={} ok={}", source_id, ok);
    Ok(ok)
}

#[tauri::command]
pub fn set_source_enabled(
    state: State<'_, Arc<AppState>>,
    source_id: String,
    enabled: bool,
) -> Result<(), String> {
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

#[tauri::command]
pub async fn pull_source(
    state: State<'_, Arc<AppState>>,
    source_id: String,
) -> Result<crate::models::ImportResult, String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || crate::subscribe::pull_source(&state, &source_id))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn generate_images(
    state: State<'_, Arc<AppState>>,
    req: GenerateRequest,
) -> Result<GenJob, String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || crate::imagegen::generate(&state, req))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn import_gen_job(
    state: State<'_, Arc<AppState>>,
    job_id: String,
    folder_id: Option<String>,
) -> Result<GenJob, String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        crate::imagegen::import_job(&state, &job_id, folder_id)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub fn list_gen_jobs(
    state: State<'_, Arc<AppState>>,
    limit: Option<i64>,
) -> Result<Vec<GenJob>, String> {
    let conn = state.open_db().map_err(|e| e.to_string())?;
    db::list_gen_jobs(&conn, limit.unwrap_or(50)).map_err(|e| e.to_string())
}
