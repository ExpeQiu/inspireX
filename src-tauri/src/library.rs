use crate::aisettings::{self, AiEndpoint, AiServices};
use crate::clip::ClipService;
use crate::db::{self, DbResult};
use crate::models::{DeleteAssetResult, Folder, ImportResult, LibraryInfo};
use crate::pipeline::{
    analyze_image, content_hash_file, guess_mime, is_image, is_media, is_video, write_thumbnail,
};
use crate::video::{extract_keyframe, has_ffmpeg};
use parking_lot::RwLock;
use rusqlite::Connection;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use uuid::Uuid;

const IMPORT_DIR_LIMIT: usize = 2000;

fn is_hidden_file_name(name: &str) -> bool {
    name.starts_with('.') || name.starts_with("._")
}

fn collect_media_files(path: &Path) -> Result<Vec<PathBuf>, String> {
    if path.is_file() {
        return Ok(if is_media(path) {
            vec![path.to_path_buf()]
        } else {
            Vec::new()
        });
    }
    if !path.is_dir() {
        return Err(format!("路径不可用: {}", path.display()));
    }
    let mut files = Vec::new();
    for entry in walkdir::WalkDir::new(path)
        .follow_links(false)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        if !entry.file_type().is_file() {
            continue;
        }
        let p = entry.path();
        let name = p.file_name().and_then(|s| s.to_str()).unwrap_or("");
        if is_hidden_file_name(name) {
            continue;
        }
        if !is_media(p) {
            continue;
        }
        files.push(p.to_path_buf());
        if files.len() >= IMPORT_DIR_LIMIT {
            log::warn!(
                target: "inspirex",
                "import_dir_truncated path={} limit={}",
                path.display(),
                IMPORT_DIR_LIMIT
            );
            break;
        }
    }
    files.sort();
    Ok(files)
}

pub struct AppState {
    pub library_path: PathBuf,
    pub ai_mock_mode: bool,
    pub db_path: PathBuf,
    pub clip: Arc<ClipService>,
    pub ingest_port: u16,
    pub ingest_token: String,
    pub ai: RwLock<AiServices>,
    pub ai_config_path: PathBuf,
    pub subscribe_poll_min: i64,
}

impl AppState {
    pub fn from_env() -> Self {
        let library_path = std::env::var("INSPIREX_LIBRARY_PATH")
            .ok()
            .filter(|s| !s.trim().is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                dirs::home_dir()
                    .unwrap_or_else(|| PathBuf::from("."))
                    .join("InspireX")
                    .join("Library")
            });
        let ai_mock_mode = std::env::var("AI_MOCK_MODE")
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(true);
        let db_path = library_path.join("inspirex.db");
        let project_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..");
        let clip = ClipService::from_env(&project_root);
        let ingest_port = std::env::var("INSPIREX_INGEST_PORT")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(18765);
        let ingest_token = std::env::var("INSPIREX_INGEST_TOKEN")
            .unwrap_or_else(|_| "inspirex-local".to_string());
        let ai_config_path = aisettings::config_file();
        let ai = RwLock::new(aisettings::load(&ai_config_path));
        let subscribe_poll_min = std::env::var("SUBSCRIBE_POLL_MIN")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(360);
        Self {
            library_path,
            ai_mock_mode,
            db_path,
            clip,
            ingest_port,
            ingest_token,
            ai,
            ai_config_path,
            subscribe_poll_min,
        }
    }

    #[cfg(test)]
    pub fn for_test(library_path: PathBuf) -> Self {
        let db_path = library_path.join("inspirex.db");
        let project_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..");
        Self {
            library_path: library_path.clone(),
            ai_mock_mode: true,
            db_path,
            clip: ClipService::from_env(&project_root),
            ingest_port: 18765,
            ingest_token: "inspirex-local".into(),
            ai: RwLock::new(AiServices::mock_all()),
            ai_config_path: library_path.join("ai-services.json"),
            subscribe_poll_min: 360,
        }
    }

    pub fn ai_text(&self) -> AiEndpoint {
        self.ai.read().text.clone()
    }

    pub fn ai_image(&self) -> AiEndpoint {
        self.ai.read().image.clone()
    }

    pub fn ensure_dirs(&self) -> std::io::Result<()> {
        std::fs::create_dir_all(self.library_path.join("files"))?;
        std::fs::create_dir_all(self.library_path.join("thumbs"))?;
        std::fs::create_dir_all(self.library_path.join("inbox"))?;
        Ok(())
    }

    pub fn open_db(&self) -> DbResult<Connection> {
        self.ensure_dirs()
            .map_err(|e| db::DbError::Msg(e.to_string()))?;
        db::open_db(&self.db_path)
    }
}

pub fn library_info(state: &AppState) -> Result<LibraryInfo, String> {
    let conn = state.open_db().map_err(|e| e.to_string())?;
    let asset_count = db::asset_count(&conn).map_err(|e| e.to_string())?;
    Ok(LibraryInfo {
        library_path: state.library_path.display().to_string(),
        ai_mock_mode: state.ai_mock_mode,
        asset_count,
        ingest_port: state.ingest_port,
        clip_model: state.clip.model_name().to_string(),
        image_gen_provider: crate::imagegen::effective_provider(state).to_string(),
    })
}

pub fn create_folder(
    state: &AppState,
    name: String,
    parent_id: Option<String>,
) -> Result<Folder, String> {
    let name = name.trim().to_string();
    if name.is_empty() {
        return Err("文件夹名为空".into());
    }
    let folder = Folder {
        id: Uuid::new_v4().to_string(),
        name,
        parent_id: parent_id.filter(|s| !s.is_empty()),
        created_at: chrono::Utc::now().to_rfc3339(),
        item_count: 0,
    };
    let conn = state.open_db().map_err(|e| e.to_string())?;
    db::insert_folder(&conn, &folder).map_err(|e| e.to_string())?;
    log::info!(
        target: "inspirex",
        "folder_create id={} name={}",
        folder.id,
        folder.name
    );
    Ok(folder)
}

pub struct ImportOpts {
    pub folder_id: Option<String>,
    pub source_url: Option<String>,
    pub notes: Option<String>,
    pub source_kind: String,
    pub source_ref: Option<String>,
}

impl Default for ImportOpts {
    fn default() -> Self {
        Self {
            folder_id: None,
            source_url: None,
            notes: None,
            source_kind: "local".to_string(),
            source_ref: None,
        }
    }
}

pub fn import_paths(
    state: &AppState,
    paths: Vec<String>,
    folder_id: Option<String>,
) -> Result<ImportResult, String> {
    import_paths_with(
        state,
        paths,
        ImportOpts {
            folder_id,
            ..ImportOpts::default()
        },
    )
}

pub fn import_paths_with(
    state: &AppState,
    paths: Vec<String>,
    opts: ImportOpts,
) -> Result<ImportResult, String> {
    let run_id = Uuid::new_v4().to_string();
    let now = chrono::Utc::now().to_rfc3339();
    log::info!(
        target: "inspirex",
        "import_start run_id={} count={} mock={} source={:?}",
        run_id,
        paths.len(),
        state.ai_mock_mode,
        opts.source_url
    );

    let conn = state.open_db().map_err(|e| e.to_string())?;
    db::insert_pipeline_run(&conn, &run_id, "import", "running", "", &now)
        .map_err(|e| e.to_string())?;

    let mut imported = 0usize;
    let mut skipped = 0usize;
    let mut errors = Vec::new();
    let mut expanded: Vec<PathBuf> = Vec::new();

    for path_str in &paths {
        let path = PathBuf::from(path_str);
        match collect_media_files(&path) {
            Ok(files) => {
                if path.is_dir() {
                    log::info!(
                        target: "inspirex",
                        "import_expand run_id={} dir={} files={}",
                        run_id,
                        path.display(),
                        files.len()
                    );
                    if files.is_empty() {
                        errors.push(format!("文件夹内没有图片/视频: {path_str}"));
                    }
                }
                expanded.extend(files);
            }
            Err(e) => errors.push(e),
        }
    }

    for path in expanded {
        match import_one(state, &conn, &path, &opts, &now) {
            Ok(true) => imported += 1,
            Ok(false) => skipped += 1,
            Err(e) => {
                let path_str = path.display().to_string();
                log::warn!(
                    target: "inspirex",
                    "import_fail run_id={} path={} err={}",
                    run_id,
                    path_str,
                    e
                );
                errors.push(format!("{path_str}: {e}"));
            }
        }
    }

    let detail = format!("imported={imported} skipped={skipped} errors={}", errors.len());
    db::insert_pipeline_run(&conn, &format!("{run_id}-done"), "import", "ok", &detail, &now)
        .map_err(|e| e.to_string())?;
    log::info!(target: "inspirex", "import_done run_id={} {}", run_id, detail);

    Ok(ImportResult {
        imported,
        skipped,
        errors,
        run_id,
    })
}

/// 从 URL 下载到 inbox 再入库（浏览器插件）。
pub fn import_from_url(
    state: &AppState,
    image_url: &str,
    page_url: Option<&str>,
    title: Option<&str>,
    notes: Option<&str>,
    folder_id: Option<String>,
) -> Result<ImportResult, String> {
    import_from_url_bytes(state, image_url, page_url, title, notes, folder_id, None, None, None, None)
}

pub fn import_from_url_bytes(
    state: &AppState,
    image_url: &str,
    page_url: Option<&str>,
    title: Option<&str>,
    notes: Option<&str>,
    folder_id: Option<String>,
    inline: Option<&[u8]>,
    mime: Option<&str>,
    source_kind: Option<&str>,
    source_ref: Option<&str>,
) -> Result<ImportResult, String> {
    let note = notes.map(|s| s.to_string()).or_else(|| {
        title.map(|t| {
            if let Some(p) = page_url {
                format!("{t}\n{p}")
            } else {
                t.to_string()
            }
        })
    });
    let opts = ImportOpts {
        folder_id,
        source_url: page_url.or(Some(image_url)).map(|s| s.to_string()),
        notes: note,
        source_kind: source_kind
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or("plugin")
            .to_string(),
        source_ref: source_ref
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string()),
    };
    if let Some(bytes) = inline.filter(|b| !b.is_empty()) {
        return import_remote_bytes(state, image_url, bytes, mime, opts);
    }
    import_remote_image(state, image_url, opts)
}

fn ext_from_mime(mime: Option<&str>) -> Option<String> {
    let m = mime.unwrap_or("").split(';').next().unwrap_or("").trim().to_ascii_lowercase();
    match m.as_str() {
        "image/jpeg" | "image/jpg" => Some("jpg".into()),
        "image/png" => Some("png".into()),
        "image/webp" => Some("webp".into()),
        "image/gif" => Some("gif".into()),
        "image/bmp" => Some("bmp".into()),
        _ => None,
    }
}

fn import_remote_bytes(
    state: &AppState,
    hint_url: &str,
    bytes: &[u8],
    mime: Option<&str>,
    opts: ImportOpts,
) -> Result<ImportResult, String> {
    let run_id = Uuid::new_v4().to_string();
    log::info!(
        target: "inspirex",
        "import_bytes_start run_id={} bytes={} mime={:?} url={}",
        run_id,
        bytes.len(),
        mime,
        hint_url
    );
    state.ensure_dirs().map_err(|e| e.to_string())?;
    let ext = ext_from_mime(mime).unwrap_or_else(|| guess_ext_from_url(hint_url));
    let tmp = state
        .library_path
        .join("inbox")
        .join(format!("{run_id}.{ext}"));
    std::fs::write(&tmp, bytes).map_err(|e| e.to_string())?;
    let result = import_paths_with(state, vec![tmp.display().to_string()], opts);
    let _ = std::fs::remove_file(&tmp);
    result
}

pub fn decode_image_base64(raw: &str) -> Result<Vec<u8>, String> {
    let s = raw
        .split(',')
        .next_back()
        .unwrap_or(raw)
        .trim();
    base64::Engine::decode(&base64::engine::general_purpose::STANDARD, s)
        .or_else(|_| {
            base64::Engine::decode(&base64::engine::general_purpose::STANDARD_NO_PAD, s)
        })
        .map_err(|e| format!("base64 解码失败: {e}"))
}

/// 下载远程图片后走同一入库管道（订阅 / 插件 / 生图落盘前也可复用）。
pub fn import_remote_image(
    state: &AppState,
    image_url: &str,
    opts: ImportOpts,
) -> Result<ImportResult, String> {
    let run_id = Uuid::new_v4().to_string();
    log::info!(
        target: "inspirex",
        "import_url_start run_id={} url={} kind={} ref={:?}",
        run_id,
        image_url,
        opts.source_kind,
        opts.source_ref
    );
    state.ensure_dirs().map_err(|e| e.to_string())?;
    let ext = guess_ext_from_url(image_url);
    let tmp = state
        .library_path
        .join("inbox")
        .join(format!("{run_id}.{ext}"));
    download_file(image_url, &tmp)?;
    let result = import_paths_with(state, vec![tmp.display().to_string()], opts);
    let _ = std::fs::remove_file(&tmp);
    result
}

fn guess_ext_from_url(url: &str) -> String {
    let path = url.split('?').next().unwrap_or(url);
    Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .filter(|e| {
            matches!(
                e.as_str(),
                "jpg" | "jpeg" | "png" | "webp" | "gif" | "bmp" | "mp4" | "mov" | "webm"
            )
        })
        .unwrap_or_else(|| "jpg".to_string())
}

pub fn download_file(url: &str, dest: &Path) -> Result<(), String> {
    let resp = ureq::get(url)
        .set("User-Agent", "InspireX/0.1")
        .timeout(std::time::Duration::from_secs(15))
        .call()
        .map_err(|e| format!("下载失败: {e}"))?;
    let mut reader = resp.into_reader();
    let mut file = std::fs::File::create(dest).map_err(|e| e.to_string())?;
    std::io::copy(&mut reader, &mut file).map_err(|e| e.to_string())?;
    Ok(())
}

fn import_one(
    state: &AppState,
    conn: &Connection,
    path: &Path,
    opts: &ImportOpts,
    now: &str,
) -> Result<bool, String> {
    let hash = content_hash_file(path)?;
    if db::content_hash_exists(conn, &hash).map_err(|e| e.to_string())? {
        return Ok(false);
    }

    let id = Uuid::new_v4().to_string();
    let filename = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("asset")
        .to_string();
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("bin")
        .to_ascii_lowercase();
    let stored_rel = format!("files/{id}.{ext}");
    let stored_abs = state.library_path.join(&stored_rel);
    if let Some(parent) = stored_abs.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    std::fs::copy(path, &stored_abs).map_err(|e| e.to_string())?;

    let meta = std::fs::metadata(&stored_abs).map_err(|e| e.to_string())?;
    let mime = guess_mime(path);
    let video = is_video(path);

    db::insert_asset(
        conn,
        &id,
        &filename,
        Some(&path.display().to_string()),
        &stored_rel,
        mime,
        Some(meta.len() as i64),
        &hash,
        opts.folder_id.as_deref(),
        opts.source_url.as_deref(),
        opts.notes.as_deref(),
        if opts.source_kind.trim().is_empty() {
            "local"
        } else {
            opts.source_kind.as_str()
        },
        opts.source_ref.as_deref(),
        now,
    )
    .map_err(|e| e.to_string())?;

    let thumb_rel = format!("thumbs/{id}.jpg");
    let thumb_abs = state.library_path.join(&thumb_rel);
    let mut width = None;
    let mut height = None;
    let mut dominant = None;
    let mut colors_json = None;
    let mut embed_source = stored_abs.clone();

    if video {
        if has_ffmpeg() {
            match extract_keyframe(&stored_abs, &thumb_abs) {
                Ok(()) => {
                    embed_source = thumb_abs.clone();
                    if let Ok(meta_img) = analyze_image(&thumb_abs) {
                        width = Some(meta_img.width as i64);
                        height = Some(meta_img.height as i64);
                        dominant = Some(meta_img.dominant_color.clone());
                        colors_json =
                            Some(serde_json::to_string(&meta_img.colors).unwrap_or_default());
                    }
                }
                Err(e) => {
                    log::warn!(target: "inspirex", "video_keyframe_fail id={} err={}", id, e);
                }
            }
        } else {
            log::warn!(target: "inspirex", "ffmpeg missing; video id={} no thumb", id);
        }
    } else if is_image(path) {
        if let Ok(meta_img) = analyze_image(&stored_abs) {
            width = Some(meta_img.width as i64);
            height = Some(meta_img.height as i64);
            dominant = Some(meta_img.dominant_color.clone());
            colors_json = Some(serde_json::to_string(&meta_img.colors).unwrap_or_default());
        }
        let _ = write_thumbnail(&stored_abs, &thumb_abs, 400);
    }

    let clip_status = match state.clip.embed_image(&embed_source) {
        Ok(emb) => {
            let json = serde_json::to_string(&emb.vector).map_err(|e| e.to_string())?;
            db::upsert_embedding(conn, &id, emb.vector.len(), &json, &emb.model, now)
                .map_err(|e| e.to_string())?;
            if state.ai_mock_mode {
                "mock"
            } else {
                "ok"
            }
        }
        Err(e) => {
            log::warn!(
                target: "inspirex",
                "embed_fail asset_id={} err={} → pending",
                id,
                e
            );
            "pending"
        }
    };

    db::update_asset_pipeline(
        conn,
        &id,
        if thumb_abs.exists() {
            Some(thumb_rel.as_str())
        } else {
            None
        },
        width,
        height,
        dominant.as_deref(),
        colors_json.as_deref(),
        clip_status,
        now,
    )
    .map_err(|e| e.to_string())?;

    Ok(true)
}

pub fn delete_asset(
    state: &AppState,
    asset_id: &str,
    delete_files: bool,
) -> Result<DeleteAssetResult, String> {
    let conn = state.open_db().map_err(|e| e.to_string())?;
    let paths = db::delete_asset(&conn, asset_id).map_err(|e| e.to_string())?;
    let Some((stored, thumb)) = paths else {
        return Ok(DeleteAssetResult {
            deleted: false,
            files_removed: 0,
        });
    };

    let mut files_removed = 0usize;
    if delete_files {
        let stored_abs = state.library_path.join(&stored);
        if stored_abs.exists() {
            if let Err(e) = std::fs::remove_file(&stored_abs) {
                log::warn!(target: "inspirex", "delete_file_fail path={} err={}", stored_abs.display(), e);
            } else {
                files_removed += 1;
            }
        }
        if let Some(t) = thumb {
            let thumb_abs = state.library_path.join(&t);
            if thumb_abs.exists() {
                if let Err(e) = std::fs::remove_file(&thumb_abs) {
                    log::warn!(target: "inspirex", "delete_thumb_fail path={} err={}", thumb_abs.display(), e);
                } else {
                    files_removed += 1;
                }
            }
        }
    }

    log::info!(
        target: "inspirex",
        "delete_done asset_id={} delete_files={} files_removed={}",
        asset_id,
        delete_files,
        files_removed
    );
    Ok(DeleteAssetResult {
        deleted: true,
        files_removed,
    })
}

pub fn asset_abs_path(state: &AppState, asset_id: &str) -> Result<String, String> {
    let conn = state.open_db().map_err(|e| e.to_string())?;
    let asset = db::get_asset(&conn, asset_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("素材不存在: {asset_id}"))?;
    let abs = state.library_path.join(&asset.stored_path);
    if !abs.exists() {
        return Err(format!("文件缺失: {}", abs.display()));
    }
    Ok(abs.display().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use crate::pipeline::mock_embed_text;
    use image::{Rgb, RgbImage};
    use tempfile::tempdir;

    #[test]
    fn import_pipeline_mock() {
        let dir = tempdir().unwrap();
        let lib = dir.path().join("Library");
        std::env::set_var("INSPIREX_LIBRARY_PATH", &lib);
        std::env::set_var("AI_MOCK_MODE", "true");
        let state = AppState::from_env();
        state.ensure_dirs().unwrap();

        let img_path = dir.path().join("sample.png");
        let mut img = RgbImage::new(32, 24);
        for p in img.pixels_mut() {
            *p = Rgb([200, 80, 40]);
        }
        img.save(&img_path).unwrap();

        let result = import_paths(&state, vec![img_path.display().to_string()], None).unwrap();
        assert_eq!(result.imported, 1, "{result:?}");

        let conn = state.open_db().unwrap();
        assert_eq!(db::asset_count(&conn).unwrap(), 1);
        let assets = db::list_assets(&conn, None, 10).unwrap();
        assert_eq!(assets[0].clip_status, "mock");
        assert!(assets[0].dominant_color.is_some());

        let embeds = db::list_embeddings(&conn).unwrap();
        assert_eq!(embeds.len(), 1);
        let q = mock_embed_text("anything");
        let sim = crate::pipeline::cosine_similarity(&q, &embeds[0].1);
        assert!(sim.is_finite());

        let id = assets[0].id.clone();
        let del = delete_asset(&state, &id, true).unwrap();
        assert!(del.deleted);
        assert!(del.files_removed >= 1);
        assert_eq!(db::asset_count(&conn).unwrap(), 0);
    }

    #[test]
    fn create_folder_then_list() {
        let dir = tempdir().unwrap();
        let lib = dir.path().join("Library");
        std::env::set_var("INSPIREX_LIBRARY_PATH", &lib);
        std::env::set_var("AI_MOCK_MODE", "true");
        let state = AppState::from_env();
        let folder = create_folder(&state, "常用素材".into(), None).unwrap();
        assert_eq!(folder.name, "常用素材");
        let conn = state.open_db().unwrap();
        let list = db::list_folders(&conn).unwrap();
        assert!(list.iter().any(|f| f.id == folder.id));
    }

    #[test]
    fn import_directory_collects_images() {
        let dir = tempdir().unwrap();
        let lib = dir.path().join("Library");
        std::env::set_var("INSPIREX_LIBRARY_PATH", &lib);
        std::env::set_var("AI_MOCK_MODE", "true");
        let state = AppState::from_env();
        state.ensure_dirs().unwrap();

        let folder = dir.path().join("album");
        std::fs::create_dir(&folder).unwrap();
        let mut img = RgbImage::new(16, 16);
        for p in img.pixels_mut() {
            *p = Rgb([10, 20, 30]);
        }
        img.save(folder.join("a.png")).unwrap();
        img.save(folder.join("b.jpg")).unwrap();
        std::fs::write(folder.join("note.txt"), b"x").unwrap();

        let result = import_paths(&state, vec![folder.display().to_string()], None).unwrap();
        assert_eq!(result.imported, 2, "{result:?}");
        let conn = state.open_db().unwrap();
        assert_eq!(db::asset_count(&conn).unwrap(), 2);
    }

    #[test]
    fn import_video_with_ffmpeg_keyframe() {
        if !has_ffmpeg() {
            eprintln!("skip: no ffmpeg");
            return;
        }
        let dir = tempdir().unwrap();
        let lib = dir.path().join("Library");
        std::env::set_var("INSPIREX_LIBRARY_PATH", &lib);
        std::env::set_var("AI_MOCK_MODE", "true");
        let state = AppState::from_env();
        state.ensure_dirs().unwrap();

        let video = dir.path().join("sample.mp4");
        let status = std::process::Command::new("ffmpeg")
            .args([
                "-y",
                "-f",
                "lavfi",
                "-i",
                "color=c=red:s=64x48:d=1",
                "-pix_fmt",
                "yuv420p",
                &video.display().to_string(),
            ])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .expect("ffmpeg");
        assert!(status.success());

        let result = import_paths(&state, vec![video.display().to_string()], None).unwrap();
        assert_eq!(result.imported, 1, "{result:?}");
        let conn = state.open_db().unwrap();
        let assets = db::list_assets(&conn, None, 10).unwrap();
        assert!(assets[0].thumb_path.is_some(), "expected keyframe thumb");
        assert_eq!(assets[0].clip_status, "mock");
    }

    #[test]
    fn import_inline_bytes_skips_network() {
        let dir = tempdir().unwrap();
        let state = AppState::for_test(dir.path().join("Library"));
        state.ensure_dirs().unwrap();
        let mut img = RgbImage::new(16, 16);
        for p in img.pixels_mut() {
            *p = Rgb([10, 20, 30]);
        }
        let png_path = dir.path().join("x.png");
        img.save(&png_path).unwrap();
        let bytes = std::fs::read(&png_path).unwrap();
        let b64 = decode_image_base64(&base64::Engine::encode(
            &base64::engine::general_purpose::STANDARD,
            &bytes,
        ))
        .unwrap();
        let result = import_from_url_bytes(
            &state,
            "https://i.pinimg.com/736x/demo.png",
            Some("https://www.pinterest.com/board/"),
            Some("pin"),
            None,
            None,
            Some(&b64),
            Some("image/png"),
            Some("plugin"),
            None,
        )
        .unwrap();
        assert_eq!(result.imported, 1, "{result:?}");
    }
}
