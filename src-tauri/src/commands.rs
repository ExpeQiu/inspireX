//! Tauri IPC 薄包装：实现见 `rpc`。封装 App 时前端优先走 invoke；浏览器走 HTTP。

use crate::browse::{ExternalListing, HubCopyResult, HubStatus, IndexRoot, LocalIndexRoot};
use crate::finder_tags::{FinderTag, FinderTagValue};
use crate::library::AppState;
use crate::models::{
    Asset, CreatePromptPreset, CreateSource, DeleteAssetResult, Folder, GenerateRequest, GenJob,
    IngestImportRequest, LibraryInfo, MinedPrompt, PromptCategory, PromptPreset, SearchQuery,
    Source, Tag, UpdateAssetMeta, UpdatePromptPreset,
};
use crate::rpc::{self, FolderContents};
use std::sync::Arc;
use tauri::State;

#[tauri::command]
pub fn get_library_info(state: State<'_, Arc<AppState>>) -> Result<LibraryInfo, String> {
    crate::library::library_info(&state)
}

#[tauri::command]
pub fn list_assets(
    state: State<'_, Arc<AppState>>,
    folder_id: Option<String>,
    source_kind: Option<String>,
    source_ref: Option<String>,
    limit: Option<i64>,
) -> Result<Vec<Asset>, String> {
    rpc::list_assets(&state, folder_id, source_kind, source_ref, limit)
}

#[tauri::command]
pub fn list_folders(state: State<'_, Arc<AppState>>) -> Result<Vec<Folder>, String> {
    rpc::list_folders(&state)
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
    rpc::delete_folder(&state, folder_id)
}

#[tauri::command]
pub fn list_folder_contents(
    state: State<'_, Arc<AppState>>,
    folder_id: String,
) -> Result<FolderContents, String> {
    rpc::list_folder_contents(&state, folder_id)
}

#[tauri::command]
pub fn add_to_folder(
    state: State<'_, Arc<AppState>>,
    folder_id: String,
    asset_ids: Option<Vec<String>>,
    paths: Option<Vec<String>>,
) -> Result<usize, String> {
    rpc::add_to_folder(&state, folder_id, asset_ids, paths)
}

#[tauri::command]
pub fn remove_from_folder(
    state: State<'_, Arc<AppState>>,
    folder_id: String,
    asset_ids: Option<Vec<String>>,
    paths: Option<Vec<String>>,
) -> Result<usize, String> {
    rpc::remove_from_folder(&state, folder_id, asset_ids, paths)
}

#[tauri::command]
pub fn list_tags(state: State<'_, Arc<AppState>>) -> Result<Vec<Tag>, String> {
    rpc::list_tags(&state)
}

#[tauri::command]
pub fn create_tag(
    state: State<'_, Arc<AppState>>,
    name: String,
    color: Option<String>,
) -> Result<Tag, String> {
    rpc::create_tag(&state, name, color)
}

#[tauri::command]
pub fn set_asset_tags(
    state: State<'_, Arc<AppState>>,
    asset_id: String,
    tags: Vec<String>,
) -> Result<(), String> {
    rpc::set_asset_tags(&state, asset_id, tags)
}

#[tauri::command]
pub fn import_files(
    state: State<'_, Arc<AppState>>,
    paths: Vec<String>,
    folder_id: Option<String>,
) -> Result<crate::models::ImportResult, String> {
    crate::library::import_paths(&state, paths, folder_id)
}

#[tauri::command]
pub fn delete_asset_cmd(
    state: State<'_, Arc<AppState>>,
    asset_id: String,
    delete_files: Option<bool>,
) -> Result<DeleteAssetResult, String> {
    crate::library::delete_asset(&state, &asset_id, delete_files.unwrap_or(true))
}

#[tauri::command]
pub fn update_asset_meta(
    state: State<'_, Arc<AppState>>,
    meta: UpdateAssetMeta,
) -> Result<(), String> {
    rpc::update_asset_meta(&state, meta)
}

#[tauri::command]
pub fn get_asset_abs_path(
    state: State<'_, Arc<AppState>>,
    asset_id: String,
) -> Result<String, String> {
    crate::library::asset_abs_path(&state, &asset_id)
}

#[tauri::command]
pub fn search_assets(
    state: State<'_, Arc<AppState>>,
    query: SearchQuery,
) -> Result<Vec<Asset>, String> {
    rpc::search_assets(&state, query)
}

#[tauri::command]
pub fn import_url(
    state: State<'_, Arc<AppState>>,
    req: IngestImportRequest,
) -> Result<crate::models::ImportResult, String> {
    rpc::import_url(&state, req)
}

#[tauri::command]
pub fn library_root(state: State<'_, Arc<AppState>>) -> String {
    rpc::library_root(&state)
}

#[tauri::command]
pub fn get_browse_root(kind: Option<String>) -> String {
    rpc::get_browse_root(kind)
}

#[tauri::command]
pub fn list_index_roots(kind: Option<String>) -> Vec<IndexRoot> {
    rpc::list_index_roots_rpc(kind)
}

#[tauri::command]
pub fn add_index_root(kind: Option<String>, path: String) -> Result<Vec<IndexRoot>, String> {
    rpc::add_index_root_rpc(kind, path)
}

#[tauri::command]
pub fn remove_index_root(kind: Option<String>, path: String) -> Result<Vec<IndexRoot>, String> {
    rpc::remove_index_root_rpc(kind, path)
}

#[tauri::command]
pub fn list_local_roots() -> Vec<LocalIndexRoot> {
    rpc::list_local_roots()
}

#[tauri::command]
pub fn add_local_root(path: String) -> Result<Vec<LocalIndexRoot>, String> {
    rpc::add_local_root(path)
}

#[tauri::command]
pub fn remove_local_root(path: String) -> Result<Vec<LocalIndexRoot>, String> {
    rpc::remove_local_root(path)
}

#[tauri::command]
pub fn get_hub_status() -> HubStatus {
    rpc::get_hub_status()
}

#[tauri::command]
pub fn set_hub_folder(path: String) -> Result<String, String> {
    rpc::set_hub_folder(path)
}

#[tauri::command]
pub fn copy_to_hub(paths: Vec<String>, dest_dir: Option<String>) -> Result<HubCopyResult, String> {
    rpc::copy_to_hub(paths, dest_dir)
}

#[tauri::command]
pub async fn list_browse_dir(
    path: Option<String>,
    limit: Option<usize>,
    offset: Option<usize>,
    kind: Option<String>,
) -> Result<ExternalListing, String> {
    tauri::async_runtime::spawn_blocking(move || rpc::list_browse_dir(path, limit, offset, kind))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub fn list_picker_dir(
    path: Option<String>,
    kind: Option<String>,
    include_media: Option<bool>,
) -> Result<ExternalListing, String> {
    rpc::list_picker_dir(path, kind, include_media)
}

#[tauri::command]
pub fn list_finder_tags() -> Vec<FinderTag> {
    rpc::list_finder_tags()
}

#[tauri::command]
pub async fn list_browse_by_tag(
    path: Option<String>,
    tag: String,
    limit: Option<usize>,
    offset: Option<usize>,
    kind: Option<String>,
) -> Result<ExternalListing, String> {
    tauri::async_runtime::spawn_blocking(move || {
        rpc::list_browse_by_tag(path, tag, limit, offset, kind)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn list_all_linked_media(
    tag: Option<String>,
    limit: Option<usize>,
    offset: Option<usize>,
) -> Result<ExternalListing, String> {
    tauri::async_runtime::spawn_blocking(move || rpc::list_all_linked_media(tag, limit, offset))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub fn get_finder_tags(path: String) -> Result<Vec<FinderTagValue>, String> {
    rpc::get_finder_tags(path)
}

#[tauri::command]
pub fn apply_finder_tag(
    paths: Vec<String>,
    tag: String,
    color: Option<u8>,
    remove: Option<bool>,
) -> Result<usize, String> {
    rpc::apply_finder_tag(paths, tag, color, remove)
}

#[tauri::command]
pub async fn get_image_thumb(
    state: State<'_, Arc<AppState>>,
    path: String,
    max_side: Option<u32>,
) -> Result<String, String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || rpc::get_image_thumb(&state, path, max_side))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub fn list_sources(state: State<'_, Arc<AppState>>) -> Result<Vec<Source>, String> {
    rpc::list_sources(&state)
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
    rpc::delete_source(&state, source_id)
}

#[tauri::command]
pub fn set_source_enabled(
    state: State<'_, Arc<AppState>>,
    source_id: String,
    enabled: bool,
) -> Result<(), String> {
    rpc::set_source_enabled(&state, source_id, enabled)
}

#[tauri::command]
pub async fn pull_source(
    state: State<'_, Arc<AppState>>,
    source_id: String,
) -> Result<crate::models::ImportResult, String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || rpc::pull_source(&state, source_id))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn generate_images(
    state: State<'_, Arc<AppState>>,
    req: GenerateRequest,
) -> Result<GenJob, String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || rpc::generate_images(&state, req))
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
    tauri::async_runtime::spawn_blocking(move || rpc::import_gen_job(&state, job_id, folder_id))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub fn report_source_run(
    state: State<'_, Arc<AppState>>,
    source_id: String,
    summary: String,
) -> Result<(), String> {
    crate::subscribe::report_source_run(&state, &source_id, &summary)
}

#[tauri::command]
pub fn list_gen_jobs(
    state: State<'_, Arc<AppState>>,
    limit: Option<i64>,
) -> Result<Vec<GenJob>, String> {
    rpc::list_gen_jobs(&state, limit)
}

#[tauri::command]
pub fn list_prompt_presets(state: State<'_, Arc<AppState>>) -> Result<Vec<PromptPreset>, String> {
    rpc::list_prompt_presets(&state)
}

#[tauri::command]
pub async fn mine_prompt_from_image(
    state: State<'_, Arc<AppState>>,
    path: String,
) -> Result<MinedPrompt, String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || rpc::mine_prompt_from_image(&state, path))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub fn create_prompt_preset(
    state: State<'_, Arc<AppState>>,
    req: CreatePromptPreset,
) -> Result<PromptPreset, String> {
    rpc::create_prompt_preset(&state, req)
}

#[tauri::command]
pub fn update_prompt_preset(
    state: State<'_, Arc<AppState>>,
    req: UpdatePromptPreset,
) -> Result<PromptPreset, String> {
    rpc::update_prompt_preset(&state, req)
}

#[tauri::command]
pub fn delete_prompt_preset(
    state: State<'_, Arc<AppState>>,
    preset_id: String,
) -> Result<bool, String> {
    rpc::delete_prompt_preset(&state, preset_id)
}

#[tauri::command]
pub fn list_prompt_categories(
    state: State<'_, Arc<AppState>>,
) -> Result<Vec<PromptCategory>, String> {
    rpc::list_prompt_categories(&state)
}

#[tauri::command]
pub fn create_prompt_category(
    state: State<'_, Arc<AppState>>,
    name: String,
) -> Result<PromptCategory, String> {
    rpc::create_prompt_category(&state, name)
}

#[tauri::command]
pub fn delete_prompt_category(
    state: State<'_, Arc<AppState>>,
    category_id: String,
) -> Result<bool, String> {
    rpc::delete_prompt_category(&state, category_id)
}

#[tauri::command]
pub fn get_vprompt_status() -> Result<crate::vprompt::VpromptStatus, String> {
    Ok(rpc::get_vprompt_status())
}

#[tauri::command]
pub fn set_vprompt_store_root(path: String) -> Result<crate::vprompt::VpromptStatus, String> {
    rpc::set_vprompt_store_root(path)
}

#[tauri::command]
pub fn sync_vprompt_prompts(
    state: State<'_, Arc<AppState>>,
    root: Option<String>,
    path: Option<String>,
) -> Result<crate::vprompt::VpromptSyncResult, String> {
    rpc::sync_vprompt_prompts(&state, root.or(path))
}

#[tauri::command]
pub fn get_ai_settings(
    state: State<'_, Arc<AppState>>,
) -> Result<crate::aisettings::AiServicesView, String> {
    Ok(rpc::get_ai_settings(&state))
}

#[tauri::command]
pub fn set_ai_settings(
    state: State<'_, Arc<AppState>>,
    text: Option<crate::aisettings::AiEndpointPatch>,
    image: Option<crate::aisettings::AiEndpointPatch>,
    video: Option<crate::aisettings::AiEndpointPatch>,
) -> Result<crate::aisettings::AiServicesView, String> {
    rpc::set_ai_settings(
        &state,
        crate::aisettings::AiServicesPatch { text, image, video },
    )
}
