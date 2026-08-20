//! 视频关键帧：依赖本机 ffmpeg。图片缩略图也可走同一条缩放命令。

use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::OnceLock;

fn looks_like_video(path: &Path) -> bool {
    matches!(
        path.extension()
            .and_then(|e| e.to_str())
            .map(|s| s.to_ascii_lowercase())
            .as_deref(),
        Some("mp4" | "mov" | "m4v" | "webm" | "mkv" | "avi")
    )
}

pub fn has_ffmpeg() -> bool {
    static OK: OnceLock<bool> = OnceLock::new();
    *OK.get_or_init(|| {
        Command::new("ffmpeg")
            .arg("-version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    })
}

/// 从视频抽取一帧为 JPEG（优先 1s，失败则 0s）。
pub fn extract_keyframe(video: &Path, out_jpg: &Path) -> Result<(), String> {
    extract_scaled_frame(video, out_jpg, None)
}

/// 抽帧或转码为 JPEG，可选最长边缩放（一次完成，避免先解全帧再缩）。
pub fn extract_scaled_frame(
    input: &Path,
    out_jpg: &Path,
    max_side: Option<u32>,
) -> Result<(), String> {
    if !has_ffmpeg() {
        return Err("ffmpeg 不可用".into());
    }
    if let Some(parent) = out_jpg.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let video = looks_like_video(input);
    let seeks: &[&str] = if video {
        &["00:00:01.000", "00:00:00.000"]
    } else {
        &[""]
    };
    let scale = max_side.map(|s| format!("scale='min({s},iw)':-2"));
    for seek in seeks {
        let mut cmd = Command::new("ffmpeg");
        cmd.arg("-y").arg("-hide_banner").arg("-loglevel").arg("error");
        if video && !seek.is_empty() {
            cmd.arg("-ss").arg(seek);
        }
        cmd.arg("-i").arg(input);
        cmd.args(["-frames:v", "1", "-an", "-q:v", "5"]);
        if let Some(vf) = scale.as_deref() {
            cmd.arg("-vf").arg(vf);
        }
        cmd.arg(out_jpg);
        cmd.stdout(Stdio::null()).stderr(Stdio::null());
        let status = cmd
            .status()
            .map_err(|e| format!("ffmpeg 不可用: {e}"))?;
        if status.success() && out_jpg.exists() {
            log::debug!(
                target: "inspirex",
                "media_frame seek={} out={} scaled={}",
                seek,
                out_jpg.display(),
                max_side.unwrap_or(0)
            );
            return Ok(());
        }
    }
    Err(format!("缩略图失败: {}", input.display()))
}
