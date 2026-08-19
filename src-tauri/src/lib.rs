mod browse;
mod clip;
mod commands;
mod db;
mod finder_tags;
mod imagegen;
mod ingest;
mod library;
mod models;
mod pipeline;
mod subscribe;
mod thumb;
mod video;

use library::AppState;
use std::sync::Arc;

fn load_dotenv() {
    let candidates = [
        std::path::PathBuf::from(".env"),
        std::path::PathBuf::from("../.env"),
    ];
    for p in candidates {
        if p.exists() {
            if let Ok(content) = std::fs::read_to_string(&p) {
                for line in content.lines() {
                    let line = line.trim();
                    if line.is_empty() || line.starts_with('#') {
                        continue;
                    }
                    if let Some((k, v)) = line.split_once('=') {
                        let k = k.trim();
                        let v = v.trim().trim_matches('"');
                        if std::env::var(k).is_err() {
                            std::env::set_var(k, v);
                        }
                    }
                }
            }
            break;
        }
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    load_dotenv();
    let _ = env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .try_init();

    let state = Arc::new(AppState::from_env());
    if let Err(e) = state.ensure_dirs() {
        log::error!(target: "inspirex", "ensure_dirs failed: {e}");
    }
    match state.open_db() {
        Ok(_) => log::info!(
            target: "inspirex",
            "library ready path={} mock={} model={} image_gen={} browse_volumes={} local_indexes={} browse_hub={} ingest=:{}",
            state.library_path.display(),
            state.ai_mock_mode,
            state.clip.model_name(),
            crate::imagegen::effective_provider(&state),
            browse::default_browse_root().display(),
            browse::list_local_index_roots().len(),
            browse::default_hub_root().display(),
            state.ingest_port
        ),
        Err(e) => log::error!(target: "inspirex", "open_db failed: {e}"),
    }

    ingest::spawn_ingest_server(state.clone());
    subscribe::spawn_subscribe_poller(state.clone());

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_drag::init())
        .manage(state)
        .invoke_handler(tauri::generate_handler![
            commands::get_library_info,
            commands::list_assets,
            commands::list_folders,
            commands::create_folder,
            commands::delete_folder,
            commands::list_folder_contents,
            commands::add_to_folder,
            commands::remove_from_folder,
            commands::list_tags,
            commands::create_tag,
            commands::set_asset_tags,
            commands::import_files,
            commands::import_url,
            commands::delete_asset_cmd,
            commands::update_asset_meta,
            commands::get_asset_abs_path,
            commands::search_assets,
            commands::library_root,
            commands::get_browse_root,
            commands::list_local_roots,
            commands::add_local_root,
            commands::remove_local_root,
            commands::get_hub_status,
            commands::set_hub_folder,
            commands::copy_to_hub,
            commands::list_browse_dir,
            commands::list_finder_tags,
            commands::list_browse_by_tag,
            commands::get_finder_tags,
            commands::apply_finder_tag,
            commands::get_image_thumb,
            commands::list_sources,
            commands::create_source,
            commands::delete_source,
            commands::set_source_enabled,
            commands::pull_source,
            commands::generate_images,
            commands::import_gen_job,
            commands::list_gen_jobs,
        ])
        .run(tauri::generate_context!())
        .expect("error while running InspireX");
}
