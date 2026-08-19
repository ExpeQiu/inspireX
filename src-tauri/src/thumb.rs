//! 外置媒体缩略图：生成到本地库缓存，返回文件路径供前端 asset 协议加载。

use crate::pipeline::{guess_mime, is_video, write_thumbnail};
use crate::video::{extract_keyframe, has_ffmpeg};
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

fn cached_jpg(library_path: &Path, path: &Path, max_side: u32) -> Result<PathBuf, String> {
    let dir = cache_dir(library_path);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir.join(format!("{}.jpg", cache_key(path, max_side))))
}

fn ensure_video_thumb(path: &Path, library_path: &Path, max_side: u32) -> Result<PathBuf, String> {
    if !has_ffmpeg() {
        return Err("需要 ffmpeg 才能生成视频预览".into());
    }
    let thumb = cached_jpg(library_path, path, max_side)?;
    if thumb.exists() {
        return Ok(thumb);
    }
    let t0 = std::time::Instant::now();
    let tmp = thumb.with_extension("full.jpg");
    extract_keyframe(path, &tmp)?;
    let resized = write_thumbnail(&tmp, &thumb, max_side);
    if let Err(e) = resized {
        if tmp.exists() {
            std::fs::rename(&tmp, &thumb).map_err(|_| e.clone())?;
        } else {
            let _ = std::fs::remove_file(&tmp);
            return Err(e);
        }
    } else {
        let _ = std::fs::remove_file(&tmp);
    }
    log::info!(
        target: "inspirex",
        "video_thumb_built src={} out={} ms={}",
        path.display(),
        thumb.display(),
        t0.elapsed().as_millis()
    );
    Ok(thumb)
}

/// 确保缩略图落盘，返回本地绝对路径（勿再 base64，避免 IPC 卡顿）。
/// 图片走缩放 JPEG；视频抽关键帧后再缩放。
pub fn ensure_image_thumb(path: &Path, library_path: &Path, max_side: u32) -> Result<PathBuf, String> {
    let mime = guess_mime(path).unwrap_or("");
    if mime.starts_with("video/") || is_video(path) {
        return ensure_video_thumb(path, library_path, max_side);
    }
    if !mime.starts_with("image/") {
        return Err("不是图片或视频".into());
    }

    let thumb = cached_jpg(library_path, path, max_side)?;
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
