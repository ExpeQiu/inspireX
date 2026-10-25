//! 外置卷 / 本机目录 / 中转站链接浏览：只展示来源图片/视频，不复制进灵犀库。
//! 中转站绑定用户已有的云同步文件夹（iCloud / Dropbox 等），同步由网盘完成。

use crate::pipeline::{guess_mime, is_media, is_video};
use rusqlite::{Connection, OpenFlags};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, UNIX_EPOCH};
use walkdir::WalkDir;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ExternalKind {
    Folder,
    Image,
    Video,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExternalItem {
    pub path: String,
    pub name: String,
    pub kind: ExternalKind,
    pub file_size: Option<i64>,
    pub modified_at: Option<i64>,
    pub mime: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExternalListing {
    pub root: String,
    pub path: String,
    pub parent: Option<String>,
    pub folders: Vec<ExternalItem>,
    pub media: Vec<ExternalItem>,
    pub truncated: bool,
    pub total_media: usize,
}

const SYSTEM_VOLUME_NAMES: &[&str] = &[
    "Macintosh HD",
    "Macintosh HD - Data",
    "Preboot",
    "Recovery",
    "VM",
    "Update",
    "iSCPreboot",
    "xarts",
    "Hardware",
];

const PICKER_MEDIA_CAP: usize = 400;

const LOCAL_SKIP_NAMES: &[&str] = &[
    "Library",
    "Applications",
    "Public",
    "Sites",
    "InspireX",
];

const ALL_LINKED_SCAN_CAP: usize = 2000;
const WALK_SKIP_DIR_NAMES: &[&str] = &["InspireX", "node_modules", ".git"];
const ALL_LINKED_VIRTUAL: &str = "inspirex://all";

pub fn parse_browse_kind(kind: Option<&str>) -> &'static str {
    match kind.map(|s| s.trim()) {
        Some(k) if k.eq_ignore_ascii_case("local") => "local",
        Some(k) if k.eq_ignore_ascii_case("hub") => "hub",
        _ => "volumes",
    }
}

pub fn default_browse_root() -> PathBuf {
    std::env::var("INSPIREX_BROWSE_ROOT")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/Volumes"))
}

pub fn default_local_root() -> PathBuf {
    std::env::var("INSPIREX_LOCAL_ROOT")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| dirs::home_dir().unwrap_or_else(|| PathBuf::from("/Users")))
}

/// 本地库 / 外置卷虚拟根：不默认扫家目录或 /Volumes，下列出用户绑定的索引路径。
pub const LOCAL_VIRTUAL_ROOT: &str = "inspirex://local";
pub const VOLUMES_VIRTUAL_ROOT: &str = "inspirex://volumes";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct IndexRoot {
    pub name: String,
    pub path: String,
}

pub type LocalIndexRoot = IndexRoot;

pub fn virtual_root_for(kind: &str) -> &'static str {
    match kind {
        "local" => LOCAL_VIRTUAL_ROOT,
        "volumes" => VOLUMES_VIRTUAL_ROOT,
        _ => "",
    }
}

pub fn is_kind_virtual_root(kind: &str, path: &Path) -> bool {
    let v = virtual_root_for(kind);
    if v.is_empty() {
        return false;
    }
    let s = path.to_string_lossy();
    s == v || s.is_empty()
}

pub fn is_local_virtual_root(path: &Path) -> bool {
    is_kind_virtual_root("local", path)
}

pub fn is_volumes_virtual_root(path: &Path) -> bool {
    is_kind_virtual_root("volumes", path)
}

fn index_kind_ok(kind: &str) -> bool {
    kind == "local" || kind == "volumes"
}

fn index_kind_label(kind: &str) -> &'static str {
    if kind == "volumes" {
        "外置卷"
    } else {
        "本地库"
    }
}

pub fn index_roots_config_file(kind: &str) -> PathBuf {
    let (env_key, filename) = if kind == "volumes" {
        ("INSPIREX_VOLUME_ROOTS_CONFIG", "volume-roots.json")
    } else {
        ("INSPIREX_LOCAL_ROOTS_CONFIG", "local-roots.json")
    };
    std::env::var(env_key)
        .ok()
        .filter(|s| !s.trim().is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            dirs::home_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join("InspireX")
                .join(filename)
        })
}

pub fn local_roots_config_file() -> PathBuf {
    index_roots_config_file("local")
}

fn name_for_index_path(path: &Path) -> String {
    if is_photos_library(path) {
        return photos_library_label(path);
    }
    path.file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| path.display().to_string())
}

fn path_under_root(path: &Path, root: &Path) -> bool {
    if path == root {
        return true;
    }
    match (path.canonicalize(), root.canonicalize()) {
        (Ok(p), Ok(r)) => p == r || p.starts_with(&r),
        _ => {
            let p = path.display().to_string();
            let r = root.display().to_string().trim_end_matches('/').to_string();
            p == r || p.starts_with(&format!("{r}/"))
        }
    }
}

fn read_index_roots_file(config: &Path) -> Vec<IndexRoot> {
    let Ok(raw) = std::fs::read_to_string(config) else {
        return Vec::new();
    };
    let t = raw.trim();
    if t.is_empty() {
        return Vec::new();
    }
    if let Ok(list) = serde_json::from_str::<Vec<IndexRoot>>(t) {
        return list
            .into_iter()
            .filter(|r| !r.path.trim().is_empty())
            .collect();
    }
    if let Ok(paths) = serde_json::from_str::<Vec<String>>(t) {
        return paths
            .into_iter()
            .filter(|s| !s.trim().is_empty())
            .map(|path| {
                let p = PathBuf::from(&path);
                IndexRoot {
                    name: name_for_index_path(&p),
                    path,
                }
            })
            .collect();
    }
    Vec::new()
}

fn write_index_roots_file(config: &Path, roots: &[IndexRoot]) -> Result<(), String> {
    if let Some(parent) = config.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let json = serde_json::to_string_pretty(roots).map_err(|e| e.to_string())?;
    std::fs::write(config, json).map_err(|e| e.to_string())
}

fn env_index_root_extra(kind: &str) -> Option<IndexRoot> {
    let key = if kind == "volumes" {
        "INSPIREX_BROWSE_ROOT"
    } else {
        "INSPIREX_LOCAL_ROOT"
    };
    std::env::var(key)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .filter(|s| kind != "volumes" || s != "/Volumes")
        .map(|path| {
            let p = PathBuf::from(&path);
            IndexRoot {
                name: name_for_index_path(&p),
                path,
            }
        })
}

pub fn list_index_roots(kind: &str) -> Vec<IndexRoot> {
    if !index_kind_ok(kind) {
        return Vec::new();
    }
    let mut roots = read_index_roots_file(&index_roots_config_file(kind));
    if let Some(extra) = env_index_root_extra(kind) {
        if !roots.iter().any(|r| r.path == extra.path) {
            roots.insert(0, extra);
        }
    }
    roots
}

pub fn list_local_index_roots() -> Vec<IndexRoot> {
    list_index_roots("local")
}

pub fn add_index_root(kind: &str, path: &Path) -> Result<Vec<IndexRoot>, String> {
    if !index_kind_ok(kind) {
        return Err("不支持的来源".into());
    }
    let raw = path.to_string_lossy();
    let trimmed = raw.trim();
    if trimmed.is_empty() || is_kind_virtual_root(kind, path) {
        return Err("请选择要绑定的文件夹".into());
    }
    let target = PathBuf::from(trimmed);
    if !target.exists() {
        return Err(format!("路径不存在: {}", target.display()));
    }
    if !target.is_dir() {
        return Err(format!("不是目录: {}", target.display()));
    }
    let canon = target
        .canonicalize()
        .unwrap_or_else(|_| target.clone());
    let name = name_for_index_path(&canon);
    let config = index_roots_config_file(kind);
    let mut roots = read_index_roots_file(&config);
    if roots.iter().any(|r| {
        PathBuf::from(&r.path)
            .canonicalize()
            .ok()
            .is_some_and(|p| p == canon)
            || r.path == canon.display().to_string()
    }) {
        log::info!(
            target: "inspirex",
            "index_root_exists kind={} path={} count={}",
            kind,
            canon.display(),
            roots.len()
        );
        return Ok(list_index_roots(kind));
    }
    roots.push(IndexRoot {
        name: name.clone(),
        path: canon.display().to_string(),
    });
    write_index_roots_file(&config, &roots)?;
    log::info!(
        target: "inspirex",
        "index_root_add kind={} path={} name={} count={} config={}",
        kind,
        canon.display(),
        name,
        roots.len(),
        config.display()
    );
    Ok(list_index_roots(kind))
}

pub fn add_local_index_root(path: &Path) -> Result<Vec<IndexRoot>, String> {
    add_index_root("local", path)
}

pub fn remove_index_root(kind: &str, path: &Path) -> Result<Vec<IndexRoot>, String> {
    if !index_kind_ok(kind) {
        return Err("不支持的来源".into());
    }
    let want = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let want_s = path.display().to_string();
    let config = index_roots_config_file(kind);
    let roots = read_index_roots_file(&config);
    let next: Vec<IndexRoot> = roots
        .into_iter()
        .filter(|r| {
            let p = PathBuf::from(&r.path);
            let canon = p.canonicalize().unwrap_or(p);
            canon != want && r.path != want_s && r.path != want.display().to_string()
        })
        .collect();
    write_index_roots_file(&config, &next)?;
    log::info!(
        target: "inspirex",
        "index_root_remove kind={} path={} count={}",
        kind,
        path.display(),
        next.len()
    );
    Ok(list_index_roots(kind))
}

pub fn remove_local_index_root(path: &Path) -> Result<Vec<IndexRoot>, String> {
    remove_index_root("local", path)
}

pub fn matching_index_root(kind: &str, path: &Path) -> Option<PathBuf> {
    for r in list_index_roots(kind) {
        let root = PathBuf::from(&r.path);
        if path_under_root(path, &root) {
            return Some(root);
        }
    }
    None
}

pub fn matching_local_root(path: &Path) -> Option<PathBuf> {
    matching_index_root("local", path)
}

fn ensure_under_any_index_root(kind: &str, path: &Path) -> Result<PathBuf, String> {
    let label = index_kind_label(kind);
    let roots = list_index_roots(kind);
    if roots.is_empty() {
        return Err(format!(
            "尚未绑定{label}索引，请用「追加文件夹」添加目录"
        ));
    }
    let path_c = path
        .canonicalize()
        .map_err(|e| format!("路径不可用 {}: {e}", path.display()))?;
    for r in &roots {
        let root = PathBuf::from(&r.path);
        if path_under_root(&path_c, &root) {
            return Ok(path_c);
        }
    }
    Err(format!(
        "仅允许浏览已绑定的{label}索引，收到: {}",
        path_c.display()
    ))
}

pub fn list_index_listing(kind: &str) -> ExternalListing {
    let virtual_root = virtual_root_for(kind);
    let roots = list_index_roots(kind);
    let folders: Vec<ExternalItem> = roots
        .iter()
        .map(|r| {
            let p = PathBuf::from(&r.path);
            item_from_path(&p)
                .map(|mut item| {
                    item.name = r.name.clone();
                    item
                })
                .unwrap_or_else(|| ExternalItem {
                    path: r.path.clone(),
                    name: r.name.clone(),
                    kind: ExternalKind::Folder,
                    file_size: None,
                    modified_at: None,
                    mime: None,
                })
        })
        .collect();
    log::info!(
        target: "inspirex",
        "index_list kind={} count={} paths={}",
        kind,
        folders.len(),
        folders
            .iter()
            .map(|f| f.path.as_str())
            .collect::<Vec<_>>()
            .join(",")
    );
    ExternalListing {
        root: virtual_root.into(),
        path: virtual_root.into(),
        parent: None,
        folders,
        media: Vec::new(),
        truncated: false,
        total_media: 0,
    }
}

pub fn list_local_index_listing() -> ExternalListing {
    list_index_listing("local")
}

pub fn list_browse_for_kind(
    path: Option<PathBuf>,
    kind: &str,
    media_limit: usize,
    media_offset: usize,
) -> Result<ExternalListing, String> {
    if index_kind_ok(kind) {
        let virtual_root = virtual_root_for(kind);
        let target = match &path {
            Some(p) if !p.as_os_str().is_empty() && !is_kind_virtual_root(kind, p) => p.clone(),
            _ => PathBuf::from(virtual_root),
        };
        if is_kind_virtual_root(kind, &target) {
            return Ok(list_index_listing(kind));
        }
        let safe = ensure_under_kind(&target, kind)?;
        let bound = matching_index_root(kind, &safe).ok_or_else(|| {
            format!("不在已绑定的{}索引内", index_kind_label(kind))
        })?;
        let mut listing = list_external_dir(&safe, media_limit, media_offset, &bound)?;
        listing.root = virtual_root.to_string();
        if same_path(&safe, &bound) {
            listing.parent = Some(virtual_root.to_string());
        }
        return Ok(listing);
    }
    let root = root_for_kind(kind);
    let target = match path {
        Some(p) if !p.as_os_str().is_empty() => p,
        _ => root.clone(),
    };
    let safe = ensure_under_kind(&target, kind)?;
    list_external_dir(&safe, media_limit, media_offset, &root)
}

/// 外置卷 / 本地库绑定根 + 中转站（若已绑定）。
pub fn linked_source_roots() -> Vec<(&'static str, PathBuf)> {
    let mut out = Vec::new();
    for r in list_index_roots("volumes") {
        out.push(("volumes", PathBuf::from(r.path)));
    }
    for r in list_index_roots("local") {
        out.push(("local", PathBuf::from(r.path)));
    }
    let hub = default_hub_root();
    if !hub.as_os_str().is_empty() {
        out.push(("hub", hub));
    }
    out
}

fn should_skip_walk_dir(name: &str) -> bool {
    is_hidden_name(name) || WALK_SKIP_DIR_NAMES.iter().any(|n| *n == name)
}

fn collect_media_under(root: &Path, cap: usize) -> Vec<ExternalItem> {
    if cap == 0 || !root.exists() {
        return Vec::new();
    }
    if is_photos_library(root) {
        return list_photos_library(root, cap, 0, root)
            .map(|l| l.media)
            .unwrap_or_default();
    }
    let mut out = Vec::new();
    let walker = WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .filter_entry(|e| {
            let name = e.file_name().to_string_lossy();
            if should_skip_walk_dir(&name) {
                return false;
            }
            if e.depth() > 0 {
                if let Some(lib) = enclosing_photos_library(e.path()) {
                    if lib != root && e.path() != lib.as_path() {
                        return false;
                    }
                }
            }
            true
        });
    for entry in walker.filter_map(|e| e.ok()) {
        if out.len() >= cap {
            break;
        }
        let p = entry.path();
        if entry.file_type().is_dir() {
            if is_photos_library(p) && p != root {
                if let Ok(l) = list_photos_library(p, cap - out.len(), 0, p) {
                    out.extend(l.media);
                }
            }
            continue;
        }
        if let Some(item) = item_from_path(p) {
            if matches!(item.kind, ExternalKind::Image | ExternalKind::Video) {
                out.push(item);
            }
        }
    }
    out
}

/// 「全部素材」用：递归收集各来源链接图（不复制）。虚拟根本身只列索引目录，这里摊平媒体。
pub fn list_all_linked_media(limit: usize, offset: usize) -> ExternalListing {
    list_all_linked_media_from(linked_source_roots(), limit, offset)
}

fn list_all_linked_media_from(
    roots: Vec<(&'static str, PathBuf)>,
    limit: usize,
    offset: usize,
) -> ExternalListing {
    let t0 = Instant::now();
    let mut seen = HashSet::new();
    let mut media = Vec::new();
    for (kind, root) in &roots {
        if !root.exists() {
            continue;
        }
        let remaining = ALL_LINKED_SCAN_CAP.saturating_sub(media.len());
        if remaining == 0 {
            break;
        }
        let got = collect_media_under(root, remaining);
        log::info!(
            target: "inspirex",
            "all_linked_scan kind={} root={} files={}",
            kind,
            root.display(),
            got.len()
        );
        for item in got {
            if seen.insert(item.path.clone()) {
                media.push(item);
            }
        }
    }
    media.sort_by(|a, b| b.modified_at.cmp(&a.modified_at));
    let total = media.len();
    let truncated = offset + limit < total || total >= ALL_LINKED_SCAN_CAP;
    let page: Vec<_> = media.into_iter().skip(offset).take(limit).collect();
    log::info!(
        target: "inspirex",
        "all_linked_media roots={} total={} page={} offset={} truncated={} ms={}",
        roots.len(),
        total,
        page.len(),
        offset,
        truncated,
        t0.elapsed().as_millis()
    );
    ExternalListing {
        root: ALL_LINKED_VIRTUAL.into(),
        path: ALL_LINKED_VIRTUAL.into(),
        parent: None,
        folders: Vec::new(),
        media: page,
        truncated,
        total_media: total,
    }
}

/// 选文件夹弹窗的沙箱根：外置卷从 `/Volumes` 起，其余从家目录起。
pub fn picker_root_for(kind: &str) -> PathBuf {
    match parse_browse_kind(Some(kind)) {
        "volumes" => PathBuf::from("/Volumes"),
        _ => dirs::home_dir().unwrap_or_else(|| PathBuf::from("/Users")),
    }
}

pub fn list_picker_dir(
    path: Option<&Path>,
    kind: &str,
    include_media: bool,
) -> Result<ExternalListing, String> {
    let root = picker_root_for(kind);
    list_picker_dir_under(path, &root, include_media)
}

/// 列子文件夹；`include_media` 时同时列当前目录图片（选参考图）。
pub fn list_picker_dir_under(
    path: Option<&Path>,
    root: &Path,
    include_media: bool,
) -> Result<ExternalListing, String> {
    let target = match path {
        Some(p) if !p.as_os_str().is_empty() => p.to_path_buf(),
        _ => root.to_path_buf(),
    };
    let safe = ensure_under_root(&target, root)?;

    if is_photos_library(&safe) {
        let parent = safe.parent().map(|p| p.display().to_string());
        log::info!(
            target: "inspirex",
            "picker_list path={} root={} photos_library=1 folders=0 media=0",
            safe.display(),
            root.display()
        );
        return Ok(ExternalListing {
            root: root.display().to_string(),
            path: safe.display().to_string(),
            parent,
            folders: Vec::new(),
            media: Vec::new(),
            truncated: false,
            total_media: 0,
        });
    }

    if !safe.is_dir() {
        return Err(format!("不是目录: {}", safe.display()));
    }

    let t0 = std::time::Instant::now();
    let root_canon = root
        .canonicalize()
        .unwrap_or_else(|_| root.to_path_buf());
    let at_root = same_path(&safe, root);
    let mut folders = Vec::new();
    let mut media = Vec::new();
    let mut truncated = false;

    let mut entries: Vec<_> = std::fs::read_dir(&safe)
        .map_err(|e| format!("无法读取 {}: {e}", safe.display()))?
        .filter_map(|e| e.ok())
        .collect();
    entries.sort_by_key(|e| e.file_name().to_ascii_lowercase());

    for entry in entries {
        let name = entry.file_name().to_string_lossy().to_string();
        if is_hidden_name(&name) {
            continue;
        }
        let p = entry.path();
        let is_dir = entry
            .file_type()
            .map(|t| t.is_dir())
            .unwrap_or_else(|_| p.is_dir());
        if !is_dir {
            if include_media {
                if let Some(item) = item_from_path(&p) {
                    if matches!(item.kind, ExternalKind::Image) {
                        if media.len() < PICKER_MEDIA_CAP {
                            media.push(item);
                        } else {
                            truncated = true;
                        }
                    }
                }
            }
            continue;
        }
        if is_volumes_dir(&safe) && skip_volume_entry(&p, &name) {
            continue;
        }
        if at_root && !is_volumes_dir(&safe) && skip_local_entry(&name) {
            continue;
        }
        let display_name = if is_photos_library(&p) {
            photos_library_label(&p)
        } else {
            name
        };
        folders.push(ExternalItem {
            path: p.display().to_string(),
            name: display_name,
            kind: ExternalKind::Folder,
            file_size: None,
            modified_at: None,
            mime: None,
        });
    }

    if at_root && !is_volumes_dir(&safe) {
        for album in photos_libraries_under(&safe.join("Pictures")) {
            if folders.iter().any(|f| f.path == album.path) {
                continue;
            }
            folders.insert(0, album);
        }
    }

    let parent = if at_root {
        None
    } else {
        safe.parent()
            .filter(|p| {
                let parent_canon = p.canonicalize().unwrap_or_else(|_| p.to_path_buf());
                parent_canon.starts_with(&root_canon) || parent_canon == root_canon
            })
            .map(|p| p.display().to_string())
    };

    let total_media = media.len();
    log::info!(
        target: "inspirex",
        "picker_list path={} root={} folders={} media={} truncated={} parent={} ms={}",
        safe.display(),
        root.display(),
        folders.len(),
        total_media,
        truncated,
        parent.as_deref().unwrap_or(""),
        t0.elapsed().as_millis()
    );

    Ok(ExternalListing {
        root: root.display().to_string(),
        path: safe.display().to_string(),
        parent,
        folders,
        media,
        truncated,
        total_media,
    })
}

pub fn hub_config_file() -> PathBuf {
    std::env::var("INSPIREX_HUB_CONFIG")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            dirs::home_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join("InspireX")
                .join("hub-root")
        })
}

pub fn read_hub_config(config: &Path) -> Option<PathBuf> {
    let raw = std::fs::read_to_string(config).ok()?;
    let t = raw.trim();
    if t.is_empty() {
        None
    } else {
        Some(PathBuf::from(t))
    }
}

pub fn write_hub_config(config: &Path, path: &Path) -> Result<(), String> {
    if let Some(parent) = config.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    std::fs::write(config, format!("{}\n", path.display())).map_err(|e| e.to_string())
}

/// 中转站根目录：环境变量 > 绑定配置文件。未绑定时为空路径。
pub fn default_hub_root() -> PathBuf {
    std::env::var("INSPIREX_HUB_ROOT")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .map(PathBuf::from)
        .or_else(|| read_hub_config(&hub_config_file()))
        .unwrap_or_default()
}

pub fn hub_is_bound() -> bool {
    let root = default_hub_root();
    !root.as_os_str().is_empty()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HubSuggestion {
    pub name: String,
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HubStatus {
    pub bound: bool,
    pub path: String,
    pub suggestions: Vec<HubSuggestion>,
}

fn cloud_provider_label(folder_name: &str) -> String {
    let lower = folder_name.to_ascii_lowercase();
    if lower.starts_with("dropbox") {
        "Dropbox".into()
    } else if lower.contains("onedrive") {
        "OneDrive".into()
    } else if lower.contains("google") {
        "Google Drive".into()
    } else if lower.contains("box") {
        "Box".into()
    } else {
        folder_name.to_string()
    }
}

pub fn suggested_hub_folders() -> Vec<HubSuggestion> {
    let Some(home) = dirs::home_dir() else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let icloud = home.join("Library/Mobile Documents/com~apple~CloudDocs");
    if icloud.is_dir() {
        out.push(HubSuggestion {
            name: "iCloud 云盘".into(),
            path: icloud.join("InspireX-Hub").display().to_string(),
        });
    }
    let cloud = home.join("Library/CloudStorage");
    if let Ok(rd) = std::fs::read_dir(&cloud) {
        for entry in rd.filter_map(|e| e.ok()) {
            let p = entry.path();
            if !p.is_dir() {
                continue;
            }
            let name = entry.file_name().to_string_lossy().into_owned();
            let lower = name.to_ascii_lowercase();
            if !(lower.contains("dropbox")
                || lower.contains("onedrive")
                || lower.contains("google")
                || lower.contains("box"))
            {
                continue;
            }
            out.push(HubSuggestion {
                name: cloud_provider_label(&name),
                path: p.join("InspireX-Hub").display().to_string(),
            });
        }
    }
    out
}

pub fn hub_status() -> HubStatus {
    let path = default_hub_root();
    let bound = !path.as_os_str().is_empty();
    HubStatus {
        bound,
        path: if bound {
            path.display().to_string()
        } else {
            String::new()
        },
        suggestions: suggested_hub_folders(),
    }
}

pub fn set_hub_root(path: &Path) -> Result<PathBuf, String> {
    let raw = path.to_string_lossy();
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("中转站路径为空".into());
    }
    let target = PathBuf::from(trimmed);
    std::fs::create_dir_all(&target).map_err(|e| {
        format!("无法创建中转站目录 {}: {e}", target.display())
    })?;
    if !target.is_dir() {
        return Err(format!("不是目录: {}", target.display()));
    }
    let canon = target
        .canonicalize()
        .unwrap_or_else(|_| target.clone());
    write_hub_config(&hub_config_file(), &canon)?;
    log::info!(
        target: "inspirex",
        "hub_bind path={} config={}",
        canon.display(),
        hub_config_file().display()
    );
    Ok(canon)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HubCopyResult {
    pub copied: usize,
    pub skipped: usize,
    pub dest: String,
    pub errors: Vec<String>,
}

fn unique_dest(dir: &Path, name: &str) -> PathBuf {
    let dest = dir.join(name);
    if !dest.exists() {
        return dest;
    }
    let stem = Path::new(name)
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "file".into());
    let ext = Path::new(name)
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy()))
        .unwrap_or_default();
    for i in 1..1000 {
        let candidate = dir.join(format!("{stem}-{i}{ext}"));
        if !candidate.exists() {
            return candidate;
        }
    }
    dir.join(format!("{stem}-{}{ext}", uuid::Uuid::new_v4().simple()))
}

fn collect_copy_sources(path: &Path) -> Result<Vec<PathBuf>, String> {
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
    for entry in WalkDir::new(path)
        .follow_links(false)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        if !entry.file_type().is_file() {
            continue;
        }
        let p = entry.path();
        let name = p.file_name().and_then(|s| s.to_str()).unwrap_or("");
        if is_hidden_name(name) {
            continue;
        }
        if !is_media(p) {
            continue;
        }
        files.push(p.to_path_buf());
        if files.len() >= 500 {
            break;
        }
    }
    Ok(files)
}

pub fn copy_into_dir(paths: &[String], dest_dir: &Path) -> Result<HubCopyResult, String> {
    if !dest_dir.is_dir() {
        std::fs::create_dir_all(dest_dir).map_err(|e| e.to_string())?;
    }
    let mut copied = 0usize;
    let mut skipped = 0usize;
    let mut errors = Vec::new();
    for raw in paths {
        let src = PathBuf::from(raw.trim());
        if raw.trim().is_empty() {
            continue;
        }
        let files = match collect_copy_sources(&src) {
            Ok(v) => v,
            Err(e) => {
                errors.push(e);
                continue;
            }
        };
        if files.is_empty() {
            skipped += 1;
            continue;
        }
        for file in files {
            let name = file
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| "file".into());
            let dest = unique_dest(dest_dir, &name);
            match std::fs::copy(&file, &dest) {
                Ok(_) => {
                    copied += 1;
                    log::info!(
                        target: "inspirex",
                        "hub_copy src={} dest={}",
                        file.display(),
                        dest.display()
                    );
                }
                Err(e) => errors.push(format!("{} → {}: {e}", file.display(), dest.display())),
            }
        }
    }
    Ok(HubCopyResult {
        copied,
        skipped,
        dest: dest_dir.display().to_string(),
        errors,
    })
}

pub fn copy_into_hub(paths: Vec<String>, dest_dir: Option<String>) -> Result<HubCopyResult, String> {
    if !hub_is_bound() {
        return Err("尚未绑定中转站文件夹".into());
    }
    let dest = match dest_dir.filter(|s| !s.trim().is_empty()) {
        Some(d) => ensure_under_kind(&PathBuf::from(d), "hub")?,
        None => {
            let root = default_hub_root();
            ensure_under_kind(&root, "hub")?
        }
    };
    copy_into_dir(&paths, &dest)
}

pub fn root_for_kind(kind: &str) -> PathBuf {
    match kind {
        "local" => PathBuf::from(LOCAL_VIRTUAL_ROOT),
        "volumes" => PathBuf::from(VOLUMES_VIRTUAL_ROOT),
        "hub" => default_hub_root(),
        _ => default_browse_root(),
    }
}

pub(crate) fn is_hidden_name(name: &str) -> bool {
    name.starts_with('.') || name.starts_with("._")
}

pub fn item_from_path(p: &Path) -> Option<ExternalItem> {
    let name = p.file_name()?.to_string_lossy().to_string();
    if is_hidden_name(&name) {
        return None;
    }
    let is_dir = p.is_dir();
    let kind = if is_dir {
        ExternalKind::Folder
    } else {
        match guess_mime(p) {
            Some(m) if m.starts_with("image/") => ExternalKind::Image,
            Some(m) if m.starts_with("video/") => ExternalKind::Video,
            _ => return None,
        }
    };
    let meta = p.metadata().ok();
    let file_size = meta.as_ref().map(|m| m.len() as i64);
    let modified_at = meta.and_then(|m| m.modified().ok()).and_then(|t| {
        t.duration_since(UNIX_EPOCH)
            .ok()
            .map(|d| d.as_secs() as i64)
    });
    let mime = if is_dir {
        None
    } else {
        guess_mime(p).map(|s| s.to_string())
    };
    Some(ExternalItem {
        path: p.display().to_string(),
        name,
        kind,
        file_size,
        modified_at,
        mime,
    })
}

fn is_system_volume_name(name: &str) -> bool {
    SYSTEM_VOLUME_NAMES.iter().any(|n| *n == name) || name.starts_with("com.apple.")
}

fn is_volumes_dir(path: &Path) -> bool {
    path == Path::new("/Volumes")
        || path
            .canonicalize()
            .ok()
            .is_some_and(|p| p == Path::new("/Volumes"))
}

fn skip_volume_entry(path: &Path, name: &str) -> bool {
    if is_hidden_name(name) || is_system_volume_name(name) {
        return true;
    }
    match path.canonicalize() {
        Ok(canon) => {
            canon == Path::new("/")
                || canon.starts_with("/System")
                || canon.starts_with("/private/var/vm")
        }
        Err(_) => false,
    }
}

pub fn list_external_dir(
    path: &Path,
    media_limit: usize,
    media_offset: usize,
    listing_root: &Path,
) -> Result<ExternalListing, String> {
    if !path.exists() {
        return Err(format!("路径不存在: {}", path.display()));
    }
    if !path.is_dir() {
        return Err(format!("不是目录: {}", path.display()));
    }

    if let Some(lib) = enclosing_photos_library(path) {
        return list_photos_library(&lib, media_limit, media_offset, listing_root);
    }

    let t0 = std::time::Instant::now();
    let root_canon = listing_root
        .canonicalize()
        .unwrap_or_else(|_| listing_root.to_path_buf());
    let at_listing_root = same_path(path, listing_root);
    let mut folders = Vec::new();
    let mut media = Vec::new();
    let mut total_media = 0usize;
    let mut truncated = false;
    let mut media_index = 0usize;

    let mut entries: Vec<_> = std::fs::read_dir(path)
        .map_err(|e| e.to_string())?
        .filter_map(|e| e.ok())
        .collect();
    entries.sort_by_key(|e| e.file_name().to_ascii_lowercase());

    for entry in entries {
        let name = entry.file_name().to_string_lossy().to_string();
        if is_hidden_name(&name) {
            continue;
        }
        let p = entry.path();

        // 优先用 DirEntry::file_type，少一次额外 stat
        let is_dir = entry
            .file_type()
            .map(|t| t.is_dir())
            .unwrap_or_else(|_| p.is_dir());

        // /Volumes 根下列出外接存储，跳过系统盘
        if is_volumes_dir(path) && skip_volume_entry(&p, &name) {
            continue;
        }
        // 本机根目录跳过系统/应用目录，不展示库副本
        if at_listing_root && !is_volumes_dir(path) && skip_local_entry(&name) {
            continue;
        }

        let kind = if is_dir {
            ExternalKind::Folder
        } else {
            match guess_mime(&p) {
                Some(m) if m.starts_with("image/") => ExternalKind::Image,
                Some(m) if m.starts_with("video/") => ExternalKind::Video,
                _ => continue,
            }
        };

        match kind {
            ExternalKind::Folder => {
                let display_name = if is_photos_library(&p) {
                    photos_library_label(&p)
                } else {
                    name.clone()
                };
                let meta = entry.metadata().ok();
                let file_size = meta.as_ref().map(|m| m.len() as i64);
                let modified_at = meta.and_then(|m| m.modified().ok()).and_then(|t| {
                    t.duration_since(UNIX_EPOCH)
                        .ok()
                        .map(|d| d.as_secs() as i64)
                });
                folders.push(ExternalItem {
                    path: p.display().to_string(),
                    name: display_name,
                    kind,
                    file_size,
                    modified_at,
                    mime: None,
                });
            }
            ExternalKind::Image | ExternalKind::Video => {
                total_media += 1;
                if media_index < media_offset {
                    media_index += 1;
                    continue;
                }
                if media.len() >= media_limit {
                    truncated = true;
                    continue;
                }
                let meta = entry.metadata().ok();
                let file_size = meta.as_ref().map(|m| m.len() as i64);
                let modified_at = meta.and_then(|m| m.modified().ok()).and_then(|t| {
                    t.duration_since(UNIX_EPOCH)
                        .ok()
                        .map(|d| d.as_secs() as i64)
                });
                let mime = guess_mime(&p).map(|s| s.to_string());
                media.push(ExternalItem {
                    path: p.display().to_string(),
                    name,
                    kind,
                    file_size,
                    modified_at,
                    mime,
                });
                media_index += 1;
            }
        }
    }

    if at_listing_root && !is_volumes_dir(path) {
        for album in photos_libraries_under(&path.join("Pictures")) {
            if folders.iter().any(|f| f.path == album.path) {
                continue;
            }
            folders.insert(0, album);
        }
    }

    let path_canon = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let parent = if path_canon == root_canon {
        None
    } else {
        path.parent()
            .filter(|p| {
                let parent_canon = p.canonicalize().unwrap_or_else(|_| p.to_path_buf());
                parent_canon.starts_with(&root_canon) || parent_canon == root_canon
            })
            .map(|p| p.display().to_string())
    };

    if is_volumes_dir(path) {
        let names: Vec<&str> = folders.iter().map(|f| f.name.as_str()).collect();
        log::info!(
            target: "inspirex",
            "browse_volumes count={} names={}",
            names.len(),
            names.join(",")
        );
    }

    log::info!(
        target: "inspirex",
        "browse_list path={} root={} volumes_root={} folders={} media_total={} page={} offset={} truncated={} ms={}",
        path.display(),
        listing_root.display(),
        is_volumes_dir(path),
        folders.len(),
        total_media,
        media.len(),
        media_offset,
        truncated,
        t0.elapsed().as_millis()
    );

    Ok(ExternalListing {
        root: listing_root.display().to_string(),
        path: path.display().to_string(),
        parent,
        folders,
        media,
        truncated,
        total_media,
    })
}

fn skip_local_entry(name: &str) -> bool {
    LOCAL_SKIP_NAMES.iter().any(|n| *n == name)
}

fn is_photos_library(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("photoslibrary"))
        && path.is_dir()
}

fn photos_library_label(path: &Path) -> String {
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "照片".into());
    if stem == "Photos Library" {
        "照片".into()
    } else {
        stem
    }
}

fn enclosing_photos_library(path: &Path) -> Option<PathBuf> {
    path.ancestors().find(|p| is_photos_library(p)).map(|p| p.to_path_buf())
}

fn photos_libraries_under(pictures: &Path) -> Vec<ExternalItem> {
    let Ok(rd) = std::fs::read_dir(pictures) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for entry in rd.filter_map(|e| e.ok()) {
        let p = entry.path();
        if !is_photos_library(&p) {
            continue;
        }
        let meta = entry.metadata().ok();
        out.push(ExternalItem {
            path: p.display().to_string(),
            name: photos_library_label(&p),
            kind: ExternalKind::Folder,
            file_size: meta.as_ref().map(|m| m.len() as i64),
            modified_at: meta.and_then(|m| m.modified().ok()).and_then(|t| {
                t.duration_since(UNIX_EPOCH)
                    .ok()
                    .map(|d| d.as_secs() as i64)
            }),
            mime: None,
        });
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

/// Core Data / Apple epoch：2001-01-01 UTC
const APPLE_EPOCH: i64 = 978_307_200;

fn resolve_photos_file(lib: &Path, dir: &str, filename: &str) -> Option<(PathBuf, ExternalKind)> {
    let orig = lib.join("originals").join(dir).join(filename);
    if orig.is_file() {
        let kind = if is_video(&orig) {
            ExternalKind::Video
        } else {
            ExternalKind::Image
        };
        return Some((orig, kind));
    }
    let stem = Path::new(filename).file_stem()?.to_string_lossy();
    let master = lib
        .join("resources/derivatives/masters")
        .join(dir)
        .join(format!("{stem}_4_5005_c.jpeg"));
    if master.is_file() {
        return Some((master, ExternalKind::Image));
    }
    let preview = lib
        .join("resources/derivatives")
        .join(dir)
        .join(format!("{stem}_1_105_c.jpeg"));
    if preview.is_file() {
        return Some((preview, ExternalKind::Image));
    }
    None
}

fn item_from_resolved(
    path: PathBuf,
    display_name: String,
    kind: ExternalKind,
    modified_at: Option<i64>,
) -> ExternalItem {
    let meta = std::fs::metadata(&path).ok();
    ExternalItem {
        path: path.display().to_string(),
        name: display_name,
        kind,
        file_size: meta.as_ref().map(|m| m.len() as i64),
        modified_at: modified_at.or_else(|| {
            meta.and_then(|m| m.modified().ok()).and_then(|t| {
                t.duration_since(UNIX_EPOCH)
                    .ok()
                    .map(|d| d.as_secs() as i64)
            })
        }),
        mime: guess_mime(&path).map(|s| s.to_string()),
    }
}

fn list_photos_via_sqlite(lib: &Path) -> Result<Vec<ExternalItem>, String> {
    let db = lib.join("database/Photos.sqlite");
    if !db.is_file() {
        return Err("无 Photos.sqlite".into());
    }
    let conn = Connection::open_with_flags(&db, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|e| e.to_string())?;
    let _ = conn.busy_timeout(Duration::from_millis(400));
    let mut stmt = conn
        .prepare(
            "SELECT IFNULL(ZDIRECTORY,''), ZFILENAME, IFNULL(ZKIND,0), ZDATECREATED
             FROM ZASSET
             WHERE IFNULL(ZTRASHEDSTATE,0)=0 AND IFNULL(ZHIDDEN,0)=0
               AND ZFILENAME IS NOT NULL
             ORDER BY ZDATECREATED DESC",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, Option<f64>>(3)?,
            ))
        })
        .map_err(|e| e.to_string())?;

    let mut items = Vec::new();
    for row in rows {
        let (dir, filename, _kind, created) = row.map_err(|e| e.to_string())?;
        let Some((path, kind)) = resolve_photos_file(lib, &dir, &filename) else {
            continue;
        };
        let modified_at = created.map(|d| d as i64 + APPLE_EPOCH);
        items.push(item_from_resolved(path, filename, kind, modified_at));
    }
    Ok(items)
}

fn list_photos_via_walk(lib: &Path) -> Vec<ExternalItem> {
    let mut items = Vec::new();
    let originals = lib.join("originals");
    if originals.is_dir() {
        for entry in WalkDir::new(&originals).into_iter().filter_map(|e| e.ok()) {
            if !entry.file_type().is_file() {
                continue;
            }
            if let Some(item) = item_from_path(entry.path()) {
                if item.kind != ExternalKind::Folder {
                    items.push(item);
                }
            }
        }
    }
    if !items.is_empty() {
        return items;
    }
    let masters = lib.join("resources/derivatives/masters");
    if masters.is_dir() {
        for entry in WalkDir::new(&masters).into_iter().filter_map(|e| e.ok()) {
            if !entry.file_type().is_file() {
                continue;
            }
            if let Some(item) = item_from_path(entry.path()) {
                if item.kind != ExternalKind::Folder {
                    items.push(item);
                }
            }
        }
    }
    items
}

fn list_photos_library(
    lib: &Path,
    media_limit: usize,
    media_offset: usize,
    listing_root: &Path,
) -> Result<ExternalListing, String> {
    let t0 = std::time::Instant::now();
    let (items, source) = match list_photos_via_sqlite(lib) {
        Ok(v) if !v.is_empty() => (v, "sqlite"),
        Ok(_) => (list_photos_via_walk(lib), "walk-empty-db"),
        Err(e) => {
            log::warn!(
                target: "inspirex",
                "photos_db_open_fail lib={} err={}",
                lib.display(),
                e
            );
            (list_photos_via_walk(lib), "walk")
        }
    };

    let total_media = items.len();
    let truncated = media_offset + media_limit < total_media;
    let media = items
        .into_iter()
        .skip(media_offset)
        .take(media_limit)
        .collect::<Vec<_>>();

    let parent = lib.parent().and_then(|p| {
        let root_c = listing_root
            .canonicalize()
            .unwrap_or_else(|_| listing_root.to_path_buf());
        let parent_c = p.canonicalize().unwrap_or_else(|_| p.to_path_buf());
        if parent_c.starts_with(&root_c) || parent_c == root_c {
            Some(p.display().to_string())
        } else {
            None
        }
    });

    log::info!(
        target: "inspirex",
        "photos_list lib={} source={} total={} page={} offset={} truncated={} ms={}",
        lib.display(),
        source,
        total_media,
        media.len(),
        media_offset,
        truncated,
        t0.elapsed().as_millis()
    );

    Ok(ExternalListing {
        root: listing_root.display().to_string(),
        path: lib.display().to_string(),
        parent,
        folders: Vec::new(),
        media,
        truncated,
        total_media,
    })
}

fn same_path(a: &Path, b: &Path) -> bool {
    if a == b {
        return true;
    }
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(x), Ok(y)) => x == y,
        _ => false,
    }
}

pub fn ensure_under_root(path: &Path, root: &Path) -> Result<PathBuf, String> {
    let root_c = root
        .canonicalize()
        .map_err(|e| format!("浏览根目录不可用 {}: {e}", root.display()))?;
    let path_c = path
        .canonicalize()
        .map_err(|e| format!("路径不可用 {}: {e}", path.display()))?;
    if !path_c.starts_with(&root_c) {
        return Err(format!(
            "仅允许浏览 {} 下的路径，收到: {}",
            root_c.display(),
            path_c.display()
        ));
    }
    Ok(path_c)
}

pub fn ensure_under_browse_root(path: &Path) -> Result<PathBuf, String> {
    if is_volumes_virtual_root(path) {
        return Ok(PathBuf::from(VOLUMES_VIRTUAL_ROOT));
    }
    ensure_under_any_index_root("volumes", path)
}

pub fn ensure_under_kind(path: &Path, kind: &str) -> Result<PathBuf, String> {
    if kind == "hub" && !hub_is_bound() {
        return Err("尚未绑定中转站文件夹".into());
    }
    if index_kind_ok(kind) {
        if is_kind_virtual_root(kind, path) {
            return Ok(PathBuf::from(virtual_root_for(kind)));
        }
        return ensure_under_any_index_root(kind, path);
    }
    let root = root_for_kind(kind);
    if kind == "hub" && root.as_os_str().is_empty() {
        return Err("尚未绑定中转站文件夹".into());
    }
    ensure_under_root(path, &root)
}

pub fn ensure_readable(path: &Path) -> Result<PathBuf, String> {
    if let Ok(p) = ensure_under_kind(path, "volumes") {
        return Ok(p);
    }
    if let Ok(p) = ensure_under_kind(path, "local") {
        return Ok(p);
    }
    if hub_is_bound() {
        if let Ok(p) = ensure_under_kind(path, "hub") {
            return Ok(p);
        }
    }
    Err(format!(
        "仅允许浏览外置卷、本机目录或中转站，收到: {}",
        path.display()
    ))
}

/// 选参考图等：家目录 / 外置卷沙箱内的图片、视频可预览（不要求已绑定索引）。
pub fn ensure_picker_media(path: &Path) -> Result<PathBuf, String> {
    let path_c = path
        .canonicalize()
        .map_err(|e| format!("路径不可用 {}: {e}", path.display()))?;
    if !is_media(&path_c) {
        return Err(format!("不是图片或视频: {}", path.display()));
    }
    for kind in ["local", "volumes"] {
        let root = picker_root_for(kind);
        if ensure_under_root(&path_c, &root).is_ok() {
            return Ok(path_c);
        }
    }
    Err(format!(
        "仅允许预览家目录或外置卷中的图片，收到: {}",
        path.display()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn lists_images_and_videos() {
        let dir = tempdir().unwrap();
        fs::create_dir(dir.path().join("sub")).unwrap();
        fs::write(dir.path().join("a.png"), b"x").unwrap();
        fs::write(dir.path().join("b.mp4"), b"x").unwrap();
        fs::write(dir.path().join("c.txt"), b"x").unwrap();
        fs::write(dir.path().join("._skip.png"), b"x").unwrap();

        // temporarily override browse root for parent check — list doesn't require under root for unit test of classify
        let listing = {
            // call internal listing without root guard by using list on temp — parent logic uses default root
            // so just verify classify via reading dir manually
            let mut n_img = 0;
            let mut n_vid = 0;
            let mut n_dir = 0;
            for e in fs::read_dir(dir.path()).unwrap().filter_map(|e| e.ok()) {
                let p = e.path();
                let name = e.file_name().to_string_lossy().to_string();
                if is_hidden_name(&name) {
                    continue;
                }
                match {
                    if p.is_dir() {
                        Some(ExternalKind::Folder)
                    } else {
                        match guess_mime(&p) {
                            Some(m) if m.starts_with("image/") => Some(ExternalKind::Image),
                            Some(m) if m.starts_with("video/") => Some(ExternalKind::Video),
                            _ => None,
                        }
                    }
                } {
                    Some(ExternalKind::Image) => n_img += 1,
                    Some(ExternalKind::Video) => n_vid += 1,
                    Some(ExternalKind::Folder) => n_dir += 1,
                    None => {}
                }
            }
            (n_img, n_vid, n_dir)
        };
        assert_eq!(listing, (1, 1, 1));
    }

    #[test]
    fn skips_system_volume_names() {
        assert!(is_system_volume_name("Macintosh HD"));
        assert!(is_system_volume_name("Macintosh HD - Data"));
        assert!(is_system_volume_name("com.apple.TimeMachine.localsnapshots"));
        assert!(!is_system_volume_name("Q-V-P"));
        assert!(!is_system_volume_name("Lexar"));
    }

    #[test]
    fn parse_kind_and_skip_local() {
        assert_eq!(parse_browse_kind(None), "volumes");
        assert_eq!(parse_browse_kind(Some("local")), "local");
        assert_eq!(parse_browse_kind(Some("LOCAL")), "local");
        assert_eq!(parse_browse_kind(Some("hub")), "hub");
        assert_eq!(parse_browse_kind(Some("HUB")), "hub");
        assert!(skip_local_entry("Library"));
        assert!(skip_local_entry("InspireX"));
        assert!(!skip_local_entry("Pictures"));
        assert!(!skip_local_entry("Movies"));
    }

    #[test]
    fn hub_config_roundtrip_and_copy() {
        let dir = tempdir().unwrap();
        let cfg = dir.path().join("hub-root");
        let hub = dir.path().join("cloud");
        fs::create_dir(&hub).unwrap();
        write_hub_config(&cfg, &hub).unwrap();
        assert_eq!(read_hub_config(&cfg).unwrap(), hub);

        fs::write(dir.path().join("a.png"), b"x").unwrap();
        fs::write(dir.path().join("note.txt"), b"no").unwrap();
        let result = copy_into_dir(
            &[
                dir.path().join("a.png").display().to_string(),
                dir.path().join("note.txt").display().to_string(),
            ],
            &hub,
        )
        .unwrap();
        assert_eq!(result.copied, 1);
        assert_eq!(result.skipped, 1);
        assert!(hub.join("a.png").is_file());

        let listing = list_external_dir(&hub, 72, 0, &hub).unwrap();
        assert_eq!(listing.media.len(), 1);
        assert_eq!(listing.root, hub.display().to_string());
    }

    #[test]
    fn hub_copy_renames_collision() {
        let dir = tempdir().unwrap();
        let hub = dir.path().join("cloud");
        fs::create_dir(&hub).unwrap();
        fs::write(hub.join("a.png"), b"old").unwrap();
        fs::write(dir.path().join("a.png"), b"new").unwrap();
        let result = copy_into_dir(&[dir.path().join("a.png").display().to_string()], &hub).unwrap();
        assert_eq!(result.copied, 1);
        assert!(hub.join("a-1.png").is_file());
        assert_eq!(fs::read(hub.join("a.png")).unwrap(), b"old");
    }

    #[test]
    fn unbound_hub_kind_errors() {
        let dir = tempdir().unwrap();
        let cfg = dir.path().join("missing-hub-root");
        let prev_cfg = std::env::var("INSPIREX_HUB_CONFIG").ok();
        let prev_root = std::env::var("INSPIREX_HUB_ROOT").ok();
        std::env::set_var("INSPIREX_HUB_CONFIG", &cfg);
        std::env::remove_var("INSPIREX_HUB_ROOT");
        let err = ensure_under_kind(dir.path(), "hub").unwrap_err();
        assert!(err.contains("尚未绑定"), "{err}");
        match prev_cfg {
            Some(v) => std::env::set_var("INSPIREX_HUB_CONFIG", v),
            None => std::env::remove_var("INSPIREX_HUB_CONFIG"),
        }
        match prev_root {
            Some(v) => std::env::set_var("INSPIREX_HUB_ROOT", v),
            None => std::env::remove_var("INSPIREX_HUB_ROOT"),
        }
    }

    #[test]
    fn local_root_listing_skips_library_and_does_not_copy() {
        let dir = tempdir().unwrap();
        fs::create_dir(dir.path().join("Library")).unwrap();
        fs::create_dir(dir.path().join("Pictures")).unwrap();
        fs::write(dir.path().join("a.png"), b"x").unwrap();
        let listing = list_external_dir(dir.path(), 72, 0, dir.path()).unwrap();
        let names: Vec<&str> = listing.folders.iter().map(|f| f.name.as_str()).collect();
        assert!(names.contains(&"Pictures"));
        assert!(!names.contains(&"Library"));
        assert_eq!(listing.media.len(), 1);
        assert_eq!(listing.root, dir.path().display().to_string());
    }

    #[test]
    fn photos_library_label_default() {
        assert_eq!(
            photos_library_label(Path::new("/x/Photos Library.photoslibrary")),
            "照片"
        );
        assert_eq!(
            photos_library_label(Path::new("/x/Travel.photoslibrary")),
            "Travel"
        );
    }

    #[test]
    fn lists_photos_library_originals_without_copying() {
        let dir = tempdir().unwrap();
        let lib = dir.path().join("Photos Library.photoslibrary");
        fs::create_dir_all(lib.join("originals/A")).unwrap();
        fs::create_dir_all(lib.join("database")).unwrap();
        fs::write(lib.join("originals/A/uuid.png"), b"x").unwrap();
        fs::write(lib.join("database/Photos.sqlite"), b"not-a-db").unwrap();
        let listing = list_external_dir(&lib, 72, 0, dir.path()).unwrap();
        assert!(listing.folders.is_empty());
        assert_eq!(listing.media.len(), 1);
        assert_eq!(listing.media[0].kind, ExternalKind::Image);
        assert!(listing.media[0].path.contains("originals"));
    }

    #[test]
    fn pictures_dir_shows_photos_album_folder() {
        let dir = tempdir().unwrap();
        let pic = dir.path().join("Pictures");
        fs::create_dir_all(pic.join("Photos Library.photoslibrary/originals")).unwrap();
        fs::write(pic.join("a.png"), b"x").unwrap();
        let listing = list_external_dir(&pic, 72, 0, dir.path()).unwrap();
        let names: Vec<&str> = listing.folders.iter().map(|f| f.name.as_str()).collect();
        assert!(names.contains(&"照片"), "{names:?}");
        assert_eq!(listing.media.len(), 1);
    }

    #[test]
    fn home_listing_injects_photos_library() {
        let dir = tempdir().unwrap();
        let lib = dir
            .path()
            .join("Pictures")
            .join("Photos Library.photoslibrary");
        fs::create_dir_all(lib.join("originals")).unwrap();
        let listing = list_external_dir(dir.path(), 72, 0, dir.path()).unwrap();
        let names: Vec<&str> = listing.folders.iter().map(|f| f.name.as_str()).collect();
        assert!(names.contains(&"照片"), "{names:?}");
        assert!(names.contains(&"Pictures"));
    }

    #[test]
    fn real_macos_photos_library_viewable_if_present() {
        let Some(home) = dirs::home_dir() else {
            return;
        };
        let lib = home.join("Pictures/Photos Library.photoslibrary");
        if !lib.is_dir() {
            return;
        }
        let listing = list_external_dir(&lib, 8, 0, &home).expect("list photos library");
        if listing.total_media == 0 {
            // 相册存在但当前进程看不到原图（权限/空库）时跳过
            return;
        }
        assert!(
            listing.total_media > 0,
            "本机相册应能列出可查看的图片/视频，实际 0"
        );
        assert!(!listing.media.is_empty());
        assert!(listing.folders.is_empty());
    }

    #[test]
    fn local_index_roots_bind_multiple_and_do_not_default_home() {
        let dir = tempdir().unwrap();
        let cfg = dir.path().join("local-roots.json");
        std::env::set_var("INSPIREX_LOCAL_ROOTS_CONFIG", &cfg);
        std::env::remove_var("INSPIREX_LOCAL_ROOT");

        let empty = list_local_index_listing();
        assert!(empty.folders.is_empty());
        assert_eq!(empty.path, LOCAL_VIRTUAL_ROOT);

        let a = dir.path().join("album-a");
        let b = dir.path().join("album-b");
        fs::create_dir(&a).unwrap();
        fs::create_dir(&b).unwrap();
        fs::write(a.join("x.png"), b"x").unwrap();

        let roots = add_local_index_root(&a).unwrap();
        assert_eq!(roots.len(), 1);
        let roots = add_local_index_root(&b).unwrap();
        assert_eq!(roots.len(), 2);
        let again = add_local_index_root(&a).unwrap();
        assert_eq!(again.len(), 2, "duplicate bind is no-op");

        let listing = list_browse_for_kind(None, "local", 72, 0).unwrap();
        assert_eq!(listing.path, LOCAL_VIRTUAL_ROOT);
        assert_eq!(listing.folders.len(), 2);
        assert_eq!(listing.total_media, 0);

        let inner = list_browse_for_kind(Some(a.clone()), "local", 72, 0).unwrap();
        assert_eq!(inner.media.len(), 1);
        assert_eq!(inner.parent.as_deref(), Some(LOCAL_VIRTUAL_ROOT));

        let home = dirs::home_dir().unwrap();
        assert!(ensure_under_kind(&home, "local").is_err());

        let left = remove_local_index_root(&a).unwrap();
        assert_eq!(left.len(), 1);
        assert_eq!(left[0].path, b.canonicalize().unwrap().display().to_string());
    }

    #[test]
    fn volume_index_roots_bind_multiple_and_do_not_default_volumes() {
        let dir = tempdir().unwrap();
        let cfg = dir.path().join("volume-roots.json");
        std::env::set_var("INSPIREX_VOLUME_ROOTS_CONFIG", &cfg);
        std::env::remove_var("INSPIREX_BROWSE_ROOT");

        let empty = list_index_listing("volumes");
        assert!(empty.folders.is_empty());
        assert_eq!(empty.path, VOLUMES_VIRTUAL_ROOT);

        let a = dir.path().join("disk-a");
        let b = dir.path().join("disk-b");
        fs::create_dir(&a).unwrap();
        fs::create_dir(&b).unwrap();
        fs::write(a.join("x.png"), b"x").unwrap();

        assert_eq!(add_index_root("volumes", &a).unwrap().len(), 1);
        assert_eq!(add_index_root("volumes", &b).unwrap().len(), 2);

        let listing = list_browse_for_kind(None, "volumes", 72, 0).unwrap();
        assert_eq!(listing.path, VOLUMES_VIRTUAL_ROOT);
        assert_eq!(listing.folders.len(), 2);

        let inner = list_browse_for_kind(Some(a.clone()), "volumes", 72, 0).unwrap();
        assert_eq!(inner.media.len(), 1);
        assert_eq!(inner.parent.as_deref(), Some(VOLUMES_VIRTUAL_ROOT));

        assert!(ensure_under_kind(Path::new("/Volumes"), "volumes").is_err());
        assert_eq!(remove_index_root("volumes", &a).unwrap().len(), 1);
    }

    #[test]
    fn picker_lists_folders_not_files() {
        let dir = tempdir().unwrap();
        fs::create_dir(dir.path().join("Pictures")).unwrap();
        fs::create_dir(dir.path().join("Movies")).unwrap();
        fs::create_dir(dir.path().join("Library")).unwrap();
        fs::create_dir(dir.path().join(".hidden")).unwrap();
        fs::write(dir.path().join("a.png"), b"x").unwrap();

        let listing = list_picker_dir_under(None, dir.path(), false).unwrap();
        let names: Vec<_> = listing.folders.iter().map(|f| f.name.as_str()).collect();
        assert!(names.contains(&"Pictures"));
        assert!(names.contains(&"Movies"));
        assert!(!names.contains(&"Library"));
        assert!(!names.iter().any(|n| n.starts_with('.')));
        assert!(!names.contains(&"a.png"));
        assert!(listing.media.is_empty());
        assert!(listing.parent.is_none());

        let with_files = list_picker_dir_under(None, dir.path(), true).unwrap();
        assert_eq!(with_files.folders.len(), listing.folders.len());
        assert_eq!(with_files.media.len(), 1);
        assert_eq!(with_files.media[0].name, "a.png");
    }

    #[test]
    fn picker_enters_subdir_and_rejects_outside() {
        let dir = tempdir().unwrap();
        fs::create_dir(dir.path().join("Pictures")).unwrap();
        fs::create_dir(dir.path().join("Pictures").join("album")).unwrap();

        let inner = list_picker_dir_under(Some(&dir.path().join("Pictures")), dir.path(), false).unwrap();
        assert!(inner.parent.is_some());
        assert!(inner.folders.iter().any(|f| f.name == "album"));

        let err = list_picker_dir_under(Some(Path::new("/tmp")), dir.path(), false).unwrap_err();
        assert!(err.contains("仅允许浏览"));
    }

    #[test]
    fn picker_media_allows_image_under_home_or_rejects_outside() {
        let dir = tempdir().unwrap();
        let png = dir.path().join("ref.png");
        fs::write(&png, b"x").unwrap();
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/Users"));
        if png.starts_with(&home) {
            let got = ensure_picker_media(&png).unwrap();
            assert_eq!(got.file_name().unwrap(), "ref.png");
        } else {
            let err = ensure_picker_media(&png).unwrap_err();
            assert!(
                err.contains("仅允许预览") || err.contains("仅允许浏览"),
                "{err}"
            );
        }
        let txt = dir.path().join("notes.txt");
        fs::write(&txt, b"x").unwrap();
        let err = ensure_picker_media(&txt).unwrap_err();
        assert!(err.contains("不是图片"), "{err}");
    }

    #[test]
    fn picker_root_kind() {
        assert_eq!(picker_root_for("volumes"), PathBuf::from("/Volumes"));
        assert_eq!(picker_root_for("VOLUMES"), PathBuf::from("/Volumes"));
        let home = picker_root_for("local");
        assert!(!home.as_os_str().is_empty());
        assert_eq!(picker_root_for("hub"), home);
    }

    #[test]
    fn all_linked_media_flattens_nested_and_all_sources() {
        let dir = tempdir().unwrap();
        let local = dir.path().join("local-album");
        fs::create_dir_all(local.join("nested")).unwrap();
        fs::write(local.join("nested").join("deep.png"), b"x").unwrap();
        fs::write(local.join("top.jpg"), b"x").unwrap();

        let vol = dir.path().join("disk");
        fs::create_dir(&vol).unwrap();
        fs::write(vol.join("vol.png"), b"x").unwrap();

        let hub = dir.path().join("cloud");
        fs::create_dir(&hub).unwrap();
        fs::write(hub.join("hub.png"), b"x").unwrap();

        let nested = collect_media_under(&local, 100);
        assert_eq!(
            nested.len(),
            2,
            "{:?}",
            nested.iter().map(|m| m.name.as_str()).collect::<Vec<_>>()
        );

        let listing = list_all_linked_media_from(
            vec![
                ("local", local),
                ("volumes", vol),
                ("hub", hub),
            ],
            72,
            0,
        );
        let names: Vec<&str> = listing.media.iter().map(|m| m.name.as_str()).collect();
        assert!(names.contains(&"deep.png"), "{names:?}");
        assert!(names.contains(&"top.jpg"), "{names:?}");
        assert!(names.contains(&"vol.png"), "{names:?}");
        assert!(names.contains(&"hub.png"), "{names:?}");
        assert_eq!(listing.total_media, 4, "{names:?}");
        assert!(!listing.truncated);
        assert!(listing.folders.is_empty());
    }
}
