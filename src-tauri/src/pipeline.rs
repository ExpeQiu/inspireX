//! 本地 CLIP 管道：Mock 确定性伪向量；真实 CLIP 后续替换 encode_* 实现。

use image::GenericImageView;
use sha2::{Digest, Sha256};
use std::path::Path;

pub const EMBED_DIM: usize = 512;
pub const MOCK_MODEL: &str = "mock-clip-v1";

/// 从任意字节种子生成单位化伪向量（可复现）。
pub fn mock_embed_from_bytes(seed: &[u8], dim: usize) -> Vec<f32> {
    let mut out = Vec::with_capacity(dim);
    let mut state = Sha256::digest(seed);
    while out.len() < dim {
        let next = Sha256::digest(&state);
        for chunk in next.chunks(4) {
            if out.len() >= dim {
                break;
            }
            let mut buf = [0u8; 4];
            buf.copy_from_slice(chunk);
            let u = u32::from_le_bytes(buf);
            // map to [-1, 1]
            let v = (u as f32 / u32::MAX as f32) * 2.0 - 1.0;
            out.push(v);
        }
        state = next;
    }
    l2_normalize(&mut out);
    out
}

pub fn mock_embed_text(text: &str) -> Vec<f32> {
    mock_embed_from_bytes(text.as_bytes(), EMBED_DIM)
}

pub fn mock_embed_file(path: &Path) -> Result<Vec<f32>, String> {
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    let hash = Sha256::digest(&bytes);
    Ok(mock_embed_from_bytes(&hash, EMBED_DIM))
}

pub fn l2_normalize(v: &mut [f32]) {
    let norm = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm > 1e-12 {
        for x in v.iter_mut() {
            *x /= norm;
        }
    }
}

pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    a.iter().zip(b.iter()).map(|(x, y)| x * y).sum()
}

pub fn content_hash_file(path: &Path) -> Result<String, String> {
    use std::io::Read;
    let mut file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 1024 * 64];
    loop {
        let n = file.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hex::encode(hasher.finalize()))
}

#[derive(Debug, Clone)]
pub struct ImageMeta {
    pub width: u32,
    pub height: u32,
    pub dominant_color: String,
    pub colors: Vec<String>,
}

/// 提取尺寸与主色（采样）。
pub fn analyze_image(path: &Path) -> Result<ImageMeta, String> {
    let img = image::open(path).map_err(|e| e.to_string())?;
    let (w, h) = img.dimensions();
    let rgb = img.to_rgb8();
    let mut r_sum: u64 = 0;
    let mut g_sum: u64 = 0;
    let mut b_sum: u64 = 0;
    let mut n: u64 = 0;
    let step = ((w * h) / 2000).max(1);
    for (i, pixel) in rgb.pixels().enumerate() {
        if i as u32 % step != 0 {
            continue;
        }
        r_sum += pixel[0] as u64;
        g_sum += pixel[1] as u64;
        b_sum += pixel[2] as u64;
        n += 1;
    }
    let (r, g, b) = if n == 0 {
        (128, 128, 128)
    } else {
        ((r_sum / n) as u8, (g_sum / n) as u8, (b_sum / n) as u8)
    };
    let dominant = format!("#{:02X}{:02X}{:02X}", r, g, b);
    Ok(ImageMeta {
        width: w,
        height: h,
        dominant_color: dominant.clone(),
        colors: vec![dominant],
    })
}

pub fn write_thumbnail(src: &Path, dest: &Path, max_side: u32) -> Result<(), String> {
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    if try_sips_thumb(src, dest, max_side).is_ok() {
        return Ok(());
    }
    if crate::video::extract_scaled_frame(src, dest, Some(max_side)).is_ok() {
        return Ok(());
    }
    let img = image::open(src).map_err(|e| e.to_string())?;
    let thumb = img.thumbnail(max_side, max_side);
    thumb
        .save_with_format(dest, image::ImageFormat::Jpeg)
        .map_err(|e| e.to_string())?;
    Ok(())
}

fn try_sips_thumb(src: &Path, dest: &Path, max_side: u32) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        use std::process::{Command, Stdio};
        let status = Command::new("sips")
            .args([
                "-Z",
                &max_side.to_string(),
                "-s",
                "format",
                "jpeg",
                &src.display().to_string(),
                "--out",
                &dest.display().to_string(),
            ])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map_err(|e| e.to_string())?;
        if status.success() && dest.exists() {
            return Ok(());
        }
        Err("sips 缩略图失败".into())
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (src, dest, max_side);
        Err("no sips".into())
    }
}

pub fn guess_mime(path: &Path) -> Option<&'static str> {
    match path
        .extension()
        .and_then(|e| e.to_str())
        .map(|s| s.to_ascii_lowercase())
        .as_deref()
    {
        Some("jpg") | Some("jpeg") => Some("image/jpeg"),
        Some("png") => Some("image/png"),
        Some("webp") => Some("image/webp"),
        Some("gif") => Some("image/gif"),
        Some("bmp") => Some("image/bmp"),
        Some("tif") | Some("tiff") => Some("image/tiff"),
        Some("heic") | Some("heif") => Some("image/heic"),
        Some("dng") => Some("image/x-adobe-dng"),
        Some("mp4") => Some("video/mp4"),
        Some("mov") | Some("m4v") => Some("video/quicktime"),
        Some("webm") => Some("video/webm"),
        Some("mkv") => Some("video/x-matroska"),
        Some("avi") => Some("video/x-msvideo"),
        _ => None,
    }
}

pub fn is_image(path: &Path) -> bool {
    matches!(
        guess_mime(path).map(|m| m.starts_with("image/")),
        Some(true)
    )
}

pub fn is_video(path: &Path) -> bool {
    matches!(
        guess_mime(path).map(|m| m.starts_with("video/")),
        Some(true)
    )
}

pub fn is_media(path: &Path) -> bool {
    is_image(path) || is_video(path)
}

/// 解析 #RGB / #RRGGBB。
pub fn parse_hex_color(hex: &str) -> Option<(u8, u8, u8)> {
    let s = hex.trim().trim_start_matches('#');
    let (r, g, b) = match s.len() {
        3 => {
            let r = u8::from_str_radix(&s[0..1].repeat(2), 16).ok()?;
            let g = u8::from_str_radix(&s[1..2].repeat(2), 16).ok()?;
            let b = u8::from_str_radix(&s[2..3].repeat(2), 16).ok()?;
            (r, g, b)
        }
        6 => {
            let r = u8::from_str_radix(&s[0..2], 16).ok()?;
            let g = u8::from_str_radix(&s[2..4], 16).ok()?;
            let b = u8::from_str_radix(&s[4..6], 16).ok()?;
            (r, g, b)
        }
        _ => return None,
    };
    Some((r, g, b))
}

/// RGB 欧氏距离（0–~441）。
pub fn color_distance(a: (u8, u8, u8), b: (u8, u8, u8)) -> f32 {
    let dr = a.0 as f32 - b.0 as f32;
    let dg = a.1 as f32 - b.1 as f32;
    let db = a.2 as f32 - b.2 as f32;
    (dr * dr + dg * dg + db * db).sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mock_embed_stable() {
        let a = mock_embed_text("red chair");
        let b = mock_embed_text("red chair");
        assert_eq!(a.len(), EMBED_DIM);
        assert!((cosine_similarity(&a, &b) - 1.0).abs() < 1e-5);
    }

    #[test]
    fn different_text_differs() {
        let a = mock_embed_text("cat");
        let b = mock_embed_text("airplane");
        assert!(cosine_similarity(&a, &b) < 0.99);
    }

    #[test]
    fn hex_and_distance() {
        let a = parse_hex_color("#FF0000").unwrap();
        let b = parse_hex_color("#f00").unwrap();
        assert_eq!(a, b);
        assert!(color_distance(a, (0, 255, 0)) > 200.0);
    }
}
