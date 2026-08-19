//! macOS Finder 标签（kMDItemUserTags）：侧栏色点、筛选、贴到文件。

use crate::browse::{
    default_local_root, ensure_readable, item_from_path, ExternalKind,
    ExternalListing,
};
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

/// Finder 色板 index：0 无 / 1 灰 / 2 绿 / 3 紫 / 4 蓝 / 5 黄 / 6 红 / 7 橙
pub const FINDER_PALETTE: &[(&str, u8, &str)] = &[
    ("红色", 6, "#FF5F57"),
    ("橙色", 7, "#FEBC2E"),
    ("黄色", 5, "#F5C518"),
    ("绿色", 2, "#28C840"),
    ("蓝色", 4, "#2A9DF4"),
    ("紫色", 3, "#AF52DE"),
    ("灰色", 1, "#A3A3A3"),
];

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FinderTag {
    pub name: String,
    pub color: u8,
    pub hex: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FinderTagValue {
    pub name: String,
    pub color: u8,
}

pub fn default_finder_tags() -> Vec<FinderTag> {
    FINDER_PALETTE
        .iter()
        .map(|(name, color, hex)| FinderTag {
            name: (*name).to_string(),
            color: *color,
            hex: (*hex).to_string(),
        })
        .collect()
}

pub fn color_hex(color: u8) -> &'static str {
    FINDER_PALETTE
        .iter()
        .find(|(_, c, _)| *c == color)
        .map(|(_, _, hex)| *hex)
        .unwrap_or("#A3A3A3")
}

pub fn parse_tag_entry(raw: &str) -> FinderTagValue {
    let raw = raw.trim();
    if let Some((name, rest)) = raw.split_once('\n') {
        let color = rest.trim().parse::<u8>().unwrap_or(0).min(7);
        FinderTagValue {
            name: name.to_string(),
            color,
        }
    } else {
        let known = FINDER_PALETTE.iter().find(|(n, _, _)| *n == raw);
        FinderTagValue {
            name: raw.to_string(),
            color: known.map(|(_, c, _)| *c).unwrap_or(0),
        }
    }
}

fn encode_tag_entry(tag: &FinderTagValue) -> String {
    format!("{}\n{}", tag.name, tag.color.min(7))
}

fn xattr_hex_bytes(s: &str) -> Vec<u8> {
    s.split_whitespace()
        .filter_map(|b| u8::from_str_radix(b, 16).ok())
        .collect()
}

fn bytes_to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn bplist_to_strings(bytes: &[u8]) -> Result<Vec<String>, String> {
    let tmp = std::env::temp_dir().join(format!("inspirex-tags-{}.plist", uuid::Uuid::new_v4()));
    std::fs::write(&tmp, bytes).map_err(|e| e.to_string())?;
    let out = Command::new("plutil")
        .args(["-convert", "json", "-o", "-", "--"])
        .arg(&tmp)
        .output()
        .map_err(|e| format!("plutil: {e}"))?;
    let _ = std::fs::remove_file(&tmp);
    if !out.status.success() {
        return Err(format!(
            "plutil 失败: {}",
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    serde_json::from_slice(&out.stdout).map_err(|e| format!("解析标签 plist: {e}"))
}

fn strings_to_bplist(items: &[String]) -> Result<Vec<u8>, String> {
    let tmp = std::env::temp_dir().join(format!("inspirex-tags-{}.json", uuid::Uuid::new_v4()));
    {
        let mut f = std::fs::File::create(&tmp).map_err(|e| e.to_string())?;
        let json = serde_json::to_vec(items).map_err(|e| e.to_string())?;
        f.write_all(&json).map_err(|e| e.to_string())?;
    }
    let st = Command::new("plutil")
        .args(["-convert", "binary1"])
        .arg(&tmp)
        .status()
        .map_err(|e| format!("plutil: {e}"))?;
    if !st.success() {
        let _ = std::fs::remove_file(&tmp);
        return Err("plutil 转 binary 失败".into());
    }
    // plutil -convert binary1 会把 .json 就地改成 binary，扩展名仍可能是 .json
    let bytes = std::fs::read(&tmp).map_err(|e| e.to_string())?;
    let _ = std::fs::remove_file(&tmp);
    Ok(bytes)
}

pub fn get_path_tags(path: &Path) -> Result<Vec<FinderTagValue>, String> {
    let out = Command::new("xattr")
        .args(["-px", "com.apple.metadata:_kMDItemUserTags"])
        .arg(path)
        .output()
        .map_err(|e| format!("xattr: {e}"))?;
    if !out.status.success() {
        return Ok(Vec::new());
    }
    let bytes = xattr_hex_bytes(&String::from_utf8_lossy(&out.stdout));
    if bytes.is_empty() {
        return Ok(Vec::new());
    }
    let raws = bplist_to_strings(&bytes).unwrap_or_default();
    Ok(raws.iter().map(|s| parse_tag_entry(s)).collect())
}

pub fn set_path_tags(path: &Path, tags: &[FinderTagValue]) -> Result<(), String> {
    if tags.is_empty() {
        let _ = Command::new("xattr")
            .args(["-d", "com.apple.metadata:_kMDItemUserTags"])
            .arg(path)
            .status();
        return Ok(());
    }
    let encoded: Vec<String> = tags.iter().map(encode_tag_entry).collect();
    let bytes = strings_to_bplist(&encoded)?;
    let st = Command::new("xattr")
        .args(["-wx", "com.apple.metadata:_kMDItemUserTags", &bytes_to_hex(&bytes)])
        .arg(path)
        .status()
        .map_err(|e| format!("xattr: {e}"))?;
    if !st.success() {
        return Err("写入 Finder 标签失败".into());
    }
    let _ = Command::new("mdimport").args(["-i"]).arg(path).status();
    Ok(())
}

pub fn apply_tag(path: &Path, name: &str, color: u8, remove: bool) -> Result<Vec<FinderTagValue>, String> {
    let mut tags = get_path_tags(path)?;
    if remove {
        tags.retain(|t| t.name != name);
    } else if !tags.iter().any(|t| t.name == name) {
        tags.push(FinderTagValue {
            name: name.to_string(),
            color,
        });
    } else {
        for t in &mut tags {
            if t.name == name {
                t.color = color;
            }
        }
    }
    set_path_tags(path, &tags)?;
    Ok(tags)
}

fn mdfind_tag(scope: &Path, tag: &str) -> Result<Vec<PathBuf>, String> {
    let escaped = tag.replace('\\', "\\\\").replace('"', "\\\"");
    let query = format!("kMDItemUserTags ==[c] \"{escaped}\"");
    let out = Command::new("mdfind")
        .args(["-0", "-onlyin"])
        .arg(scope)
        .arg(&query)
        .output()
        .map_err(|e| format!("mdfind: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "mdfind 失败: {}",
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    Ok(out
        .stdout
        .split(|b| *b == 0)
        .filter(|p| !p.is_empty())
        .map(|p| PathBuf::from(String::from_utf8_lossy(p).into_owned()))
        .collect())
}

pub fn list_by_finder_tag(
    scope: &Path,
    tag: &str,
    media_limit: usize,
    media_offset: usize,
    listing_root: &Path,
) -> Result<ExternalListing, String> {
    let t0 = Instant::now();
    let paths = mdfind_tag(scope, tag)?;
    let mut folders = Vec::new();
    let mut media = Vec::new();
    let mut total_media = 0usize;
    let mut truncated = false;
    let mut media_index = 0usize;

    for p in paths {
        let Some(item) = item_from_path(&p) else {
            continue;
        };
        match item.kind {
            ExternalKind::Folder => folders.push(item),
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
                media.push(item);
                media_index += 1;
            }
        }
    }

    log::info!(
        target: "inspirex",
        "browse_tag tag={} scope={} folders={} media_total={} page={} offset={} truncated={} ms={}",
        tag,
        scope.display(),
        folders.len(),
        total_media,
        media.len(),
        media_offset,
        truncated,
        t0.elapsed().as_millis()
    );

    Ok(ExternalListing {
        root: listing_root.display().to_string(),
        path: scope.display().to_string(),
        parent: None,
        folders,
        media,
        truncated,
        total_media,
    })
}

fn first_level_under(path: &Path, root: &Path) -> PathBuf {
    let root_s = root.display().to_string().trim_end_matches('/').to_string();
    let p = path.display().to_string();
    if p == root_s || !p.starts_with(&format!("{root_s}/")) {
        return root.to_path_buf();
    }
    let rest = &p[root_s.len() + 1..];
    let vol = rest.split('/').next().unwrap_or("");
    if vol.is_empty() {
        root.to_path_buf()
    } else {
        PathBuf::from(format!("{root_s}/{vol}"))
    }
}

fn path_under(path: &Path, root: &Path) -> bool {
    let root_s = root.display().to_string().trim_end_matches('/').to_string();
    let p = path.display().to_string();
    p == root_s || p.starts_with(&format!("{root_s}/"))
}

pub fn tag_scope_for(path: &Path) -> PathBuf {
    if let Some(root) = crate::browse::matching_index_root("volumes", path) {
        return root;
    }
    let hub = crate::browse::default_hub_root();
    if !hub.as_os_str().is_empty() && path_under(path, &hub) {
        return hub;
    }
    if let Some(root) = crate::browse::matching_local_root(path) {
        return root;
    }
    let local = default_local_root();
    if path_under(path, &local) {
        return first_level_under(path, &local);
    }
    path.to_path_buf()
}

pub fn ensure_and_apply(
    paths: &[String],
    tag: &str,
    color: u8,
    remove: bool,
) -> Result<usize, String> {
    let mut ok = 0usize;
    for raw in paths {
        let p = PathBuf::from(raw);
        let safe = ensure_readable(&p)?;
        apply_tag(&safe, tag, color, remove)?;
        ok += 1;
    }
    log::info!(
        target: "inspirex",
        "finder_tag_apply tag={} remove={} files={} ok={}",
        tag,
        remove,
        paths.len(),
        ok
    );
    Ok(ok)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_finder_tag_entries() {
        let t = parse_tag_entry("红色\n6");
        assert_eq!(t.name, "红色");
        assert_eq!(t.color, 6);
        let t = parse_tag_entry("工作");
        assert_eq!(t.name, "工作");
        assert_eq!(t.color, 0);
        let t = parse_tag_entry("橙色");
        assert_eq!(t.color, 7);
    }

    #[test]
    fn palette_has_seven() {
        assert_eq!(default_finder_tags().len(), 7);
        assert_eq!(color_hex(6), "#FF5F57");
    }
}
