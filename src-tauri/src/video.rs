//! 视频关键帧：依赖本机 ffmpeg。

use std::path::Path;
use std::process::Command;

pub fn has_ffmpeg() -> bool {
    Command::new("ffmpeg")
        .arg("-version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// 从视频抽取一帧为 JPEG（优先 1s，失败则 0s）。
pub fn extract_keyframe(video: &Path, out_jpg: &Path) -> Result<(), String> {
    if let Some(parent) = out_jpg.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    for seek in ["00:00:01.000", "00:00:00.000"] {
        let status = Command::new("ffmpeg")
            .args([
                "-y",
                "-ss",
                seek,
                "-i",
                &video.display().to_string(),
                "-frames:v",
                "1",
                "-q:v",
                "3",
                &out_jpg.display().to_string(),
            ])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map_err(|e| format!("ffmpeg 不可用: {e}"))?;
        if status.success() && out_jpg.exists() {
            log::info!(
                target: "inspirex",
                "video_keyframe seek={} out={}",
                seek,
                out_jpg.display()
            );
            return Ok(());
        }
    }
    Err(format!("关键帧失败: {}", video.display()))
}
