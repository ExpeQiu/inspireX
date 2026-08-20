//! 外置媒体缩略图：生成到本地库缓存，返回文件路径供前端 asset 协议加载。

use crate::pipeline::{guess_mime, is_video, write_thumbnail};
use crate::video::{extract_scaled_frame, has_ffmpeg};
use parking_lot::Mutex;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

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

fn cached_jpg(library_path: &Path, path: &Path, max_side: u32) -> Result<PathBuf, String> {
    let dir = cache_dir(library_path);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir.join(format!("{}.jpg", cache_key(path, max_side))))
}

fn inflight() -> &'static Mutex<HashMap<String, Arc<Mutex<()>>>> {
    static MAP: OnceLock<Mutex<HashMap<String, Arc<Mutex<()>>>>> = OnceLock::new();
    MAP.get_or_init(|| Mutex::new(HashMap::new()))
}

fn with_inflight<T>(key: &str, f: impl FnOnce() -> Result<T, String>) -> Result<T, String> {
    let lock = {
        let mut map = inflight().lock();
        map.entry(key.to_string())
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone()
    };
    let _guard = lock.lock();
    f()
}

fn ensure_video_thumb(path: &Path, library_path: &Path, max_side: u32) -> Result<PathBuf, String> {
    if !has_ffmpeg() {
        return Err("需要 ffmpeg 才能生成视频预览".into());
    }
    let thumb = cached_jpg(library_path, path, max_side)?;
    if thumb.exists() {
        return Ok(thumb);
    }
    with_inflight(&thumb.display().to_string(), || {
        if thumb.exists() {
            return Ok(thumb.clone());
        }
        let t0 = Instant::now();
        extract_scaled_frame(path, &thumb, Some(max_side))?;
        log::info!(
            target: "inspirex",
            "video_thumb_built src={} out={} ms={}",
            path.display(),
            thumb.display(),
            t0.elapsed().as_millis()
        );
        Ok(thumb.clone())
    })
}

/// 确保缩略图落盘，返回本地绝对路径（勿再 base64，避免 IPC 卡顿）。
/// 图片走缩放 JPEG；视频抽关键帧时直接缩到 max_side。
pub fn ensure_image_thumb(path: &Path, library_path: &Path, max_side: u32) -> Result<PathBuf, String> {
    let mime = guess_mime(path).unwrap_or("");
    if mime.starts_with("video/") || is_video(path) {
        return ensure_video_thumb(path, library_path, max_side);
    }
    if !mime.starts_with("image/") {
        return Err("不是图片或视频".into());
    }

    let thumb = cached_jpg(library_path, path, max_side)?;
    if thumb.exists() {
        return Ok(thumb);
    }
    with_inflight(&thumb.display().to_string(), || {
        if thumb.exists() {
            return Ok(thumb.clone());
        }
        let t0 = Instant::now();
        write_thumbnail(path, &thumb, max_side)?;
        let ms = t0.elapsed().as_millis();
        if ms >= 80 {
            log::info!(
                target: "inspirex",
                "thumb_built src={} out={} ms={}",
                path.display(),
                thumb.display(),
                ms
            );
        } else {
            log::debug!(
                target: "inspirex",
                "thumb_built src={} out={} ms={}",
                path.display(),
                thumb.display(),
                ms
            );
        }
        Ok(thumb.clone())
    })
}

/// 后台预热当前页缩略图，不阻塞列表响应。新的一次预热会取消上一轮。
pub fn spawn_warmup(library_path: PathBuf, paths: Vec<String>, max_side: u32) {
    if paths.is_empty() {
        return;
    }
    static GEN: AtomicU64 = AtomicU64::new(0);
    let gen = GEN.fetch_add(1, Ordering::SeqCst) + 1;
    let n = paths.len();
    let _ = std::thread::Builder::new()
        .name("inspirex-thumb-warm".into())
        .spawn(move || {
            let t0 = Instant::now();
            let mut ok = 0usize;
            for p in &paths {
                if GEN.load(Ordering::SeqCst) != gen {
                    log::debug!(target: "inspirex", "thumb_warm_cancel gen={}", gen);
                    return;
                }
                if ensure_image_thumb(Path::new(p), &library_path, max_side).is_ok() {
                    ok += 1;
                }
            }
            log::info!(
                target: "inspirex",
                "thumb_warm n={} ok={} ms={}",
                n,
                ok,
                t0.elapsed().as_millis()
            );
        });
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
        let again = ensure_image_thumb(&img_path, &lib, 64).unwrap();
        assert_eq!(thumb, again);
    }

    #[test]
    fn builds_cached_video_keyframe_if_ffmpeg() {
        if !has_ffmpeg() {
            eprintln!("skip: no ffmpeg");
            return;
        }
        let dir = tempdir().unwrap();
        let video = dir.path().join("sample.mp4");
        let status = std::process::Command::new("ffmpeg")
            .args([
                "-y",
                "-f",
                "lavfi",
                "-i",
                "color=c=blue:s=64x48:d=1",
                "-pix_fmt",
                "yuv420p",
                &video.display().to_string(),
            ])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .expect("ffmpeg");
        assert!(status.success());
        let lib = dir.path().join("lib");
        std::fs::create_dir_all(&lib).unwrap();
        let thumb = ensure_image_thumb(&video, &lib, 64).unwrap();
        assert!(thumb.exists());
        assert!(thumb.extension().and_then(|e| e.to_str()) == Some("jpg"));
        let again = ensure_image_thumb(&video, &lib, 64).unwrap();
        assert_eq!(thumb, again);
    }
}
