//! 从 VPromptStore 读取图片提示词，同步进 prompt_presets（真源仍是 Markdown，不复制仓库）。

use crate::db;
use crate::library::AppState;
use crate::models::{PromptCategory, PromptPreset};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

pub const SOURCE_KIND: &str = "vprompt";
const IMAGE_DIR: &str = "图片提示词";
const SAMPLE_FILE: &str = "样本集合.md";
const DEFAULT_ROOT: &str = "/Volumes/Lexar/git/12 highvalue/VPromptStore";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VpromptStatus {
    pub root: String,
    pub ready: bool,
    pub last_sync_at: Option<String>,
    pub last_run_id: Option<String>,
    pub last_inserted: i64,
    pub last_updated: i64,
    pub last_pruned: i64,
    pub last_total: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VpromptSyncResult {
    pub run_id: String,
    pub root: String,
    pub categories: i64,
    pub inserted: i64,
    pub updated: i64,
    pub pruned: i64,
    pub total: i64,
    pub skipped: i64,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct VpromptFileConfig {
    #[serde(default)]
    root: String,
    last_sync_at: Option<String>,
    last_run_id: Option<String>,
    #[serde(default)]
    last_inserted: i64,
    #[serde(default)]
    last_updated: i64,
    #[serde(default)]
    last_pruned: i64,
    #[serde(default)]
    last_total: i64,
}

#[derive(Debug, Clone)]
pub struct ParsedImagePrompt {
    pub uid: String,
    pub category_key: String,
    pub category_name: String,
    pub title: String,
    pub body: String,
}

pub fn is_store_root(path: &Path) -> bool {
    path.join(IMAGE_DIR).is_dir()
}

pub fn config_file() -> PathBuf {
    std::env::var("INSPIREX_VPROMPT_CONFIG")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            dirs::home_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join("InspireX")
                .join("vprompt.json")
        })
}

fn load_file_config() -> VpromptFileConfig {
    let path = config_file();
    let Ok(raw) = fs::read_to_string(&path) else {
        return VpromptFileConfig::default();
    };
    serde_json::from_str(&raw).unwrap_or_default()
}

fn save_file_config(cfg: &VpromptFileConfig) -> Result<(), String> {
    let path = config_file();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let json = serde_json::to_string_pretty(cfg).map_err(|e| e.to_string())?;
    fs::write(&path, json).map_err(|e| e.to_string())
}

fn env_root() -> Option<PathBuf> {
    ["VPROMPT_STORE_ROOT", "INSPIREX_VPROMPT_ROOT"]
        .iter()
        .find_map(|key| {
            std::env::var(key)
                .ok()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .map(PathBuf::from)
        })
}

pub fn resolve_root(explicit: Option<&str>) -> PathBuf {
    if let Some(raw) = explicit.map(str::trim).filter(|s| !s.is_empty()) {
        return PathBuf::from(raw);
    }
    if let Some(env) = env_root() {
        return env;
    }
    let file = load_file_config();
    if !file.root.trim().is_empty() {
        return PathBuf::from(file.root.trim());
    }
    PathBuf::from(DEFAULT_ROOT)
}

pub fn status() -> VpromptStatus {
    let file = load_file_config();
    let root = resolve_root(None);
    VpromptStatus {
        root: root.display().to_string(),
        ready: is_store_root(&root),
        last_sync_at: file.last_sync_at,
        last_run_id: file.last_run_id,
        last_inserted: file.last_inserted,
        last_updated: file.last_updated,
        last_pruned: file.last_pruned,
        last_total: file.last_total,
    }
}

pub fn set_root(path: String) -> Result<VpromptStatus, String> {
    let root = PathBuf::from(path.trim());
    if !is_store_root(&root) {
        return Err(format!(
            "不是 VPromptStore 根目录（需要「{IMAGE_DIR}/」）: {}",
            root.display()
        ));
    }
    let mut cfg = load_file_config();
    cfg.root = root.display().to_string();
    save_file_config(&cfg)?;
    log::info!(
        target: "inspirex",
        "vprompt_root_bind path={}",
        root.display()
    );
    Ok(status())
}

pub fn scan_image_prompts(root: &Path) -> Result<Vec<ParsedImagePrompt>, String> {
    let image_dir = root.join(IMAGE_DIR);
    if !image_dir.is_dir() {
        return Err(format!("找不到 {IMAGE_DIR}: {}", image_dir.display()));
    }
    let mut items = Vec::new();
    let mut dirs: Vec<_> = fs::read_dir(&image_dir)
        .map_err(|e| e.to_string())?
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_dir())
        .collect();
    dirs.sort_by_key(|e| e.file_name());
    for entry in dirs {
        let folder = entry.file_name().to_string_lossy().to_string();
        let sample = entry.path().join(SAMPLE_FILE);
        if !sample.is_file() {
            continue;
        }
        let markdown = fs::read_to_string(&sample).map_err(|e| {
            format!("读取 {} 失败: {e}", sample.display())
        })?;
        items.extend(parse_image_collection(&markdown, &folder));
    }
    Ok(items)
}

pub fn parse_image_collection(markdown: &str, category_key: &str) -> Vec<ParsedImagePrompt> {
    let mut items = Vec::new();
    for (index, (title, body)) in split_numbered_h2(markdown).into_iter().enumerate() {
        let parsed = parse_image_body(&body);
        let text = parsed.body.trim().to_string();
        if text.is_empty() {
            continue;
        }
        let source_id = parsed
            .source_id
            .unwrap_or_else(|| format!("idx-{}", index + 1));
        let name = if title.trim().is_empty() {
            text.chars().take(32).collect()
        } else {
            title.trim().to_string()
        };
        items.push(ParsedImagePrompt {
            uid: format!("image:{category_key}:{source_id}"),
            category_key: category_key.to_string(),
            category_name: category_key.to_string(),
            title: name,
            body: text,
        });
    }
    items
}

struct ParsedBody {
    body: String,
    source_id: Option<String>,
}

fn split_numbered_h2(markdown: &str) -> Vec<(String, String)> {
    let mut blocks = Vec::new();
    let mut current_title: Option<String> = None;
    let mut current_body = String::new();
    for line in markdown.lines() {
        if let Some(title) = numbered_h2_title(line) {
            if let Some(prev) = current_title.take() {
                blocks.push((prev, current_body.trim_end().to_string()));
            }
            current_title = Some(title);
            current_body.clear();
            current_body.push_str(line);
            current_body.push('\n');
        } else if current_title.is_some() {
            current_body.push_str(line);
            current_body.push('\n');
        }
    }
    if let Some(prev) = current_title {
        blocks.push((prev, current_body.trim_end().to_string()));
    }
    blocks
}

fn numbered_h2_title(line: &str) -> Option<String> {
    let rest = line.strip_prefix("## ")?;
    let rest = rest.trim();
    let dot = rest.find(". ")?;
    let idx = &rest[..dot];
    if idx.is_empty() || !idx.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    Some(rest[dot + 2..].trim().to_string())
}

fn parse_image_body(body: &str) -> ParsedBody {
    let fields = extract_fields(body);
    if let Some(text) = fields
        .get("提示词")
        .or_else(|| fields.get("提示词"))
        .or_else(|| fields.get("prompt"))
        .cloned()
        .filter(|s| !s.trim().is_empty())
    {
        let id_raw = fields
            .get("id")
            .or_else(|| fields.get("ID"))
            .cloned()
            .unwrap_or_default();
        return ParsedBody {
            body: text,
            source_id: digits_or_none(&id_raw),
        };
    }

    let dict_line = strip_heading_line(body);
    let source_id = extract_dict_id(&dict_line);
    if let Some(content) = extract_dict_content(&dict_line) {
        return ParsedBody {
            body: unescape_py(&content),
            source_id,
        };
    }
    ParsedBody {
        body: dict_line
            .trim()
            .trim_end_matches("---")
            .trim()
            .to_string(),
        source_id,
    }
}

fn strip_heading_line(body: &str) -> String {
    if let Some(rest) = body.strip_prefix("## ") {
        if let Some((_, tail)) = rest.split_once('\n') {
            return tail.trim_start().to_string();
        }
        return String::new();
    }
    body.to_string()
}

fn extract_fields(block: &str) -> HashMap<String, String> {
    let padded = format!("\n{block}");
    let mut starts: Vec<(usize, usize, String)> = Vec::new();
    let mut search_from = 0;
    while let Some(rel) = padded[search_from..].find("\n**") {
        let start = search_from + rel;
        let after = start + 3;
        if let Some(name_end) = padded[after..].find("：**") {
            let name = padded[after..after + name_end].trim();
            if !name.is_empty() && !name.contains('\n') && !name.contains('*') {
                let value_start = after + name_end + "：**".len();
                starts.push((start, value_start, name.to_string()));
                search_from = value_start;
                continue;
            }
        }
        search_from = start + 3;
    }
    let mut fields = HashMap::new();
    for (i, (_, value_start, name)) in starts.iter().enumerate() {
        let value_end = starts
            .get(i + 1)
            .map(|(s, _, _)| *s)
            .unwrap_or(padded.len());
        let mut value = padded[*value_start..value_end].trim().to_string();
        if i + 1 == starts.len() {
            if let Some(stripped) = value.strip_suffix("---") {
                value = stripped.trim_end().to_string();
            }
        }
        fields.insert(name.clone(), value);
    }
    fields
}

fn extract_dict_id(raw: &str) -> Option<String> {
    for key in ["'id'", "\"id\""] {
        if let Some(pos) = raw.find(key) {
            let after = raw[pos + key.len()..].trim_start();
            let after = after.strip_prefix(':')?.trim_start();
            let digits: String = after.chars().take_while(|c| c.is_ascii_digit()).collect();
            if !digits.is_empty() {
                return Some(digits);
            }
        }
    }
    None
}

fn extract_dict_content(raw: &str) -> Option<String> {
    const MARKERS: [&str; 2] = ["'content': '", "\"content\": \""];
    for (i, marker) in MARKERS.iter().enumerate() {
        if let Some(pos) = raw.find(marker) {
            let rest = &raw[pos + marker.len()..];
            let quote = if i == 0 { '\'' } else { '"' };
            return Some(trim_dict_tail(rest, quote));
        }
    }
    None
}

fn trim_dict_tail(rest: &str, quote: char) -> String {
    let mut end = rest.len();
    if let Some(idx) = rest.rfind(quote) {
        let after = rest[idx + 1..].trim_start();
        if after.starts_with(',') || after.starts_with('}') || after.is_empty() {
            end = idx;
        }
    }
    let mut value = rest[..end].to_string();
    if let Some(stripped) = value.strip_suffix("---") {
        value = stripped.trim_end().to_string();
    }
    value
}

fn unescape_py(raw: &str) -> String {
    raw.replace("\\n", "\n")
        .replace("\\t", "\t")
        .replace("\\'", "'")
        .replace("\\\"", "\"")
        .replace("\\\\", "\\")
        .trim()
        .to_string()
}

fn digits_or_none(raw: &str) -> Option<String> {
    let t = raw.trim();
    if t.is_empty() {
        return None;
    }
    if t.chars().all(|c| c.is_ascii_digit()) {
        Some(t.to_string())
    } else {
        None
    }
}

fn category_source_key(folder: &str) -> String {
    format!("vprompt:image:{folder}")
}

pub fn sync_prompts(state: &AppState, root_override: Option<String>) -> Result<VpromptSyncResult, String> {
    let run_id = uuid::Uuid::new_v4().to_string().chars().take(8).collect::<String>();
    let root = resolve_root(root_override.as_deref());
    if !is_store_root(&root) {
        return Err(format!(
            "VPromptStore 不可用: {}。设置 VPROMPT_STORE_ROOT 或点「绑定片库」。",
            root.display()
        ));
    }
    let parsed = scan_image_prompts(&root)?;
    let conn = state.open_db().map_err(|e| e.to_string())?;
    let now = chrono::Utc::now().to_rfc3339();
    let mut inserted = 0i64;
    let mut updated = 0i64;
    let mut skipped = 0i64;
    let mut category_ids: HashMap<String, String> = HashMap::new();
    let mut keep_uids: HashSet<String> = HashSet::new();

    for item in &parsed {
        if item.body.trim().is_empty() {
            skipped += 1;
            continue;
        }
        keep_uids.insert(item.uid.clone());
        let cat_key = category_source_key(&item.category_key);
        let category_id = if let Some(id) = category_ids.get(&cat_key) {
            id.clone()
        } else if let Some(existing) = db::get_prompt_category_by_source_key(&conn, &cat_key)
            .map_err(|e| e.to_string())?
        {
            if existing.name != item.category_name {
                db::update_prompt_category_name(&conn, &existing.id, &item.category_name)
                    .map_err(|e| e.to_string())?;
            }
            category_ids.insert(cat_key, existing.id.clone());
            existing.id
        } else {
            let cat = PromptCategory {
                id: uuid::Uuid::new_v4().to_string(),
                name: item.category_name.clone(),
                created_at: now.clone(),
                item_count: 0,
                source_key: Some(cat_key.clone()),
            };
            db::insert_prompt_category(&conn, &cat).map_err(|e| e.to_string())?;
            category_ids.insert(cat_key, cat.id.clone());
            cat.id
        };

        if let Some(mut preset) = db::get_prompt_preset_by_source_uid(&conn, &item.uid)
            .map_err(|e| e.to_string())?
        {
            let changed = preset.name != item.title
                || preset.body != item.body
                || preset.category_id.as_deref() != Some(category_id.as_str());
            if changed {
                preset.name = item.title.clone();
                preset.body = item.body.clone();
                preset.category_id = Some(category_id);
                preset.updated_at = now.clone();
                db::update_prompt_preset(&conn, &preset).map_err(|e| e.to_string())?;
                updated += 1;
            }
        } else {
            let preset = PromptPreset {
                id: uuid::Uuid::new_v4().to_string(),
                name: item.title.clone(),
                body: item.body.clone(),
                image_path: None,
                category_id: Some(category_id),
                created_at: now.clone(),
                updated_at: now.clone(),
                source_kind: SOURCE_KIND.to_string(),
                source_uid: Some(item.uid.clone()),
            };
            db::insert_prompt_preset(&conn, &preset).map_err(|e| e.to_string())?;
            inserted += 1;
        }
    }

    let existing = db::list_prompt_source_uids(&conn, SOURCE_KIND).map_err(|e| e.to_string())?;
    let mut pruned = 0i64;
    for uid in existing {
        if !keep_uids.contains(&uid) {
            if db::delete_prompt_preset_by_source_uid(&conn, &uid).map_err(|e| e.to_string())? {
                pruned += 1;
            }
        }
    }

    let total = i64::try_from(keep_uids.len()).unwrap_or(0);
    let categories = i64::try_from(category_ids.len()).unwrap_or(0);
    let mut cfg = load_file_config();
    cfg.root = root.display().to_string();
    cfg.last_sync_at = Some(now);
    cfg.last_run_id = Some(run_id.clone());
    cfg.last_inserted = inserted;
    cfg.last_updated = updated;
    cfg.last_pruned = pruned;
    cfg.last_total = total;
    save_file_config(&cfg)?;

    log::info!(
        target: "inspirex",
        "vprompt_sync run_id={} root={} categories={} inserted={} updated={} pruned={} total={} skipped={}",
        run_id,
        root.display(),
        categories,
        inserted,
        updated,
        pruned,
        total,
        skipped
    );

    Ok(VpromptSyncResult {
        run_id,
        root: root.display().to_string(),
        categories,
        inserted,
        updated,
        pruned,
        total,
        skipped,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn sample_markdown() -> String {
        r#"# Demo分类

## 1. Mirror selfie

{'id': 553, 'content': '### Scene\nMirror selfie in an otaku room.\n'}

---

## 2. Poster still life

**id：** 9001

**提示词：**
暖色陶罐静物，侧光，胶片质感

---
"#
        .to_string()
    }

    #[test]
    fn parse_python_dict_and_field_blocks() {
        let items = parse_image_collection(&sample_markdown(), "Demo分类");
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].uid, "image:Demo分类:553");
        assert!(items[0].body.contains("Mirror selfie"));
        assert_eq!(items[1].uid, "image:Demo分类:9001");
        assert_eq!(items[1].body, "暖色陶罐静物，侧光，胶片质感");
    }

    fn write_store(dir: &Path) {
        let cat = dir.join(IMAGE_DIR).join("Demo分类");
        fs::create_dir_all(&cat).unwrap();
        fs::write(cat.join(SAMPLE_FILE), sample_markdown()).unwrap();
    }

    #[test]
    fn sync_insert_update_prune_keeps_local() {
        let store = tempdir().unwrap();
        write_store(store.path());
        let lib = tempdir().unwrap();
        let cfg = tempdir().unwrap().path().join("vprompt.json");
        let prev_cfg = std::env::var("INSPIREX_VPROMPT_CONFIG").ok();
        std::env::set_var("INSPIREX_VPROMPT_CONFIG", &cfg);
        let state = AppState::for_test(lib.path().to_path_buf());
        state.ensure_dirs().unwrap();

        let first = sync_prompts(&state, Some(store.path().display().to_string())).unwrap();
        assert_eq!(first.inserted, 2);
        assert_eq!(first.updated, 0);
        assert_eq!(first.total, 2);

        let conn = state.open_db().unwrap();
        let local = PromptPreset {
            id: "local-1".into(),
            name: "我的提示词".into(),
            body: "本地自建".into(),
            image_path: None,
            category_id: None,
            created_at: chrono::Utc::now().to_rfc3339(),
            updated_at: chrono::Utc::now().to_rfc3339(),
            source_kind: "local".into(),
            source_uid: None,
        };
        db::insert_prompt_preset(&conn, &local).unwrap();
        drop(conn);

        let second = sync_prompts(&state, Some(store.path().display().to_string())).unwrap();
        assert_eq!(second.inserted, 0);
        assert_eq!(second.updated, 0);
        assert_eq!(second.pruned, 0);

        let md = store
            .path()
            .join(IMAGE_DIR)
            .join("Demo分类")
            .join(SAMPLE_FILE);
        fs::write(
            &md,
            r#"# Demo分类

## 1. Mirror selfie

{'id': 553, 'content': '### Scene\nUpdated selfie.\n'}

---
"#,
        )
        .unwrap();

        let third = sync_prompts(&state, Some(store.path().display().to_string())).unwrap();
        assert_eq!(third.inserted, 0);
        assert_eq!(third.updated, 1);
        assert_eq!(third.pruned, 1);
        assert_eq!(third.total, 1);

        let conn = state.open_db().unwrap();
        let presets = db::list_prompt_presets(&conn).unwrap();
        assert_eq!(presets.len(), 2);
        assert!(presets.iter().any(|p| p.name == "我的提示词"));
        let synced = presets
            .iter()
            .find(|p| p.source_uid.as_deref() == Some("image:Demo分类:553"))
            .unwrap();
        assert!(synced.body.contains("Updated selfie"));

        match prev_cfg {
            Some(v) => std::env::set_var("INSPIREX_VPROMPT_CONFIG", v),
            None => std::env::remove_var("INSPIREX_VPROMPT_CONFIG"),
        }
    }

    #[test]
    fn parse_real_store_if_present() {
        let root = PathBuf::from(DEFAULT_ROOT);
        if !is_store_root(&root) {
            return;
        }
        let items = scan_image_prompts(&root).unwrap();
        assert!(
            items.len() >= 50,
            "expected sampled image prompts, got {}",
            items.len()
        );
        assert!(items.iter().any(|p| p.body.chars().count() > 20));
    }
}
