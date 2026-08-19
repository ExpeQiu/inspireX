//! 外置媒体缩略图：生成到本地库缓存，返回文件路径供前端 asset 协议加载。

use crate::pipeline::{guess_mime, write_thumbnail};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

pub fn cache_dir(library_path: &Path) -> PathBuf {
    library_path.join("browse-thumbs")
}

fn cache_key(path: &Path, max_side: u32) -> String {
    let meta = std::fs::metadata(path).ok();
    let mtime = meta
        .as_ref()
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let size = meta.as_ref().map(|m| m.len()).unwrap_or(0);
    let raw = format!("{}|{}|{}|{}", path.display(), mtime, size, max_side);
    hex::encode(Sha256::digest(raw.as_bytes()))
}

/// 确保缩略图落盘，返回本地绝对路径（勿再 base64，避免 IPC 卡顿）。
pub fn ensure_image_thumb(path: &Path, library_path: &Path, max_side: u32) -> Result<PathBuf, String> {
    let mime = guess_mime(path).unwrap_or("");
    if !mime.starts_with("image/") {
        return Err("不是图片".into());
    }

    let dir = cache_dir(library_path);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let thumb = dir.join(format!("{}.jpg", cache_key(path, max_side)));

    if !thumb.exists() {
        let t0 = std::time::Instant::now();
        write_thumbnail(path, &thumb, max_side)?;
        log::debug!(
            target: "inspirex",
            "thumb_built src={} out={} ms={}",
            path.display(),
            thumb.display(),
            t0.elapsed().as_millis()
        );
    }

    Ok(thumb)
}

#[cfg(test)]
mod thumb_tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn builds_cached_thumb_file() {
        let dir = tempdir().unwrap();
        let img_path = dir.path().join("a.png");
        let mut img = image::RgbImage::new(16, 16);
        for p in img.pixels_mut() {
            *p = image::Rgb([10, 20, 30]);
        }
        img.save(&img_path).unwrap();
        let lib = dir.path().join("lib");
        std::fs::create_dir_all(&lib).unwrap();
        let thumb = ensure_image_thumb(&img_path, &lib, 64).unwrap();
        assert!(thumb.exists());
        assert!(thumb.extension().and_then(|e| e.to_str()) == Some("jpg"));
    }
}
