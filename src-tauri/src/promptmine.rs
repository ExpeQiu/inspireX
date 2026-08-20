//! 参考图 → 文生图提示词（Mock First；有 Key 时走 MiniMax / OpenAI 兼容视觉）。

use crate::aisettings::effective_of;
use crate::library::AppState;
use crate::models::MinedPrompt;
use base64::Engine;
use image::GenericImageView;
use sha2::{Digest, Sha256};
use std::io::Cursor;
use std::path::Path;
use std::time::Duration;

const HTTP_TIMEOUT_SECS: u64 = 90;
const MINE_PROMPT: &str = "你是文生图提示词挖掘器。根据图片写出可直接用于文生图的中文结果。\
只输出 JSON，不要解释：{\"name\":\"不超过12字的短名称\",\"body\":\"含主体、构图、光线、风格、色彩的提示词\"}";

pub fn mine_from_image(state: &AppState, path: String) -> Result<MinedPrompt, String> {
    let path = path.trim();
    if path.is_empty() {
        return Err("请先选择参考图".into());
    }
    let p = Path::new(path);
    if !p.is_file() {
        return Err(format!("找不到图片: {path}"));
    }
    let ep = state.ai_text();
    let provider = effective_of(&ep.provider, &ep.api_key);
    log::info!(
        target: "inspirex",
        "prompt_mine_start provider={} path={}",
        provider,
        p.display()
    );
    let mined = match provider.as_str() {
        "mock" => mock_mine(p)?,
        "minimax" => vision_chat(state, p, true)?,
        _ => vision_chat(state, p, false)?,
    };
    log::info!(
        target: "inspirex",
        "prompt_mine_ok provider={} name_len={} body_len={}",
        mined.provider,
        mined.name.len(),
        mined.body.len()
    );
    Ok(mined)
}

fn mock_mine(path: &Path) -> Result<MinedPrompt, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("读图失败: {e}"))?;
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    let digest = hasher.finalize();
    let (tone, light, style) = mock_vocab(&digest, path);
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "参考图".into());
    let name: String = if stem.chars().count() <= 12 {
        stem
    } else {
        stem.chars().take(12).collect()
    };
    let body = format!(
        "{tone}，{light}，{style}，主体清晰，适合文生图（mock 挖掘）"
    );
    Ok(MinedPrompt {
        name,
        body,
        provider: "mock".into(),
    })
}

fn mock_vocab(digest: &[u8], path: &Path) -> (&'static str, &'static str, &'static str) {
    let tones = ["暖色静物", "冷调空间", "高饱和产品", "低饱和纪实", "金色黄昏"];
    let lights = ["侧光", "逆光轮廓", "柔和漫射", "硬光阴影", "窗边自然光"];
    let styles = ["胶片质感", "电影构图", "极简商业", "手作质地", "建筑几何"];
    if let Ok(img) = image::open(path) {
        let (w, h) = img.dimensions();
        let step_x = (w / 8).max(1);
        let step_y = (h / 8).max(1);
        let mut r = 0u64;
        let mut g = 0u64;
        let mut b = 0u64;
        let mut n = 0u64;
        for y in (0..h).step_by(step_y as usize) {
            for x in (0..w).step_by(step_x as usize) {
                let px = img.get_pixel(x, y).0;
                r += px[0] as u64;
                g += px[1] as u64;
                b += px[2] as u64;
                n += 1;
            }
        }
        if n > 0 {
            let avg_r = (r / n) as usize;
            let avg_g = (g / n) as usize;
            let avg_b = (b / n) as usize;
            let tone = if avg_r > avg_b + 20 {
                "暖色静物"
            } else if avg_b > avg_r + 20 {
                "冷调空间"
            } else {
                tones[avg_g % tones.len()]
            };
            let light = lights[(avg_r / 32) % lights.len()];
            let style = styles[(avg_b / 32) % styles.len()];
            return (tone, light, style);
        }
    }
    (
        tones[digest[0] as usize % tones.len()],
        lights[digest[1] as usize % lights.len()],
        styles[digest[2] as usize % styles.len()],
    )
}

fn vision_chat(state: &AppState, path: &Path, minimax: bool) -> Result<MinedPrompt, String> {
    let ep = state.ai_text();
    let data_url = image_data_url(path)?;
    let model = caption_model(&ep.model, minimax);
    let url = chat_endpoint(&ep.base_url, minimax);
    let body = serde_json::json!({
        "model": model,
        "temperature": 0.3,
        "messages": [{
            "role": "user",
            "content": [
                {"type": "image_url", "image_url": {"url": data_url}},
                {"type": "text", "text": MINE_PROMPT}
            ]
        }]
    });
    log::info!(
        target: "inspirex",
        "prompt_mine_req model={} url={}",
        model,
        url
    );
    let resp = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(HTTP_TIMEOUT_SECS))
        .build()
        .post(&url)
        .set(
            "Authorization",
            &format!("Bearer {}", ep.api_key.trim()),
        )
        .set("Content-Type", "application/json")
        .send_json(body)
        .map_err(|e| format!("挖掘请求失败: {e}"))?;
    let json: serde_json::Value = resp
        .into_json()
        .map_err(|e| format!("挖掘响应不是 JSON: {e}"))?;
    if let Some(code) = json
        .pointer("/base_resp/status_code")
        .and_then(|v| v.as_i64())
    {
        if code != 0 {
            let msg = json
                .pointer("/base_resp/status_msg")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            return Err(format!("挖掘失败 {code}: {msg}"));
        }
    }
    let text = extract_chat_text(&json).ok_or_else(|| format!("挖掘未返回文本: {json}"))?;
    let mut mined = parse_mined(&text);
    mined.provider = if minimax { "minimax".into() } else { "openai_compat".into() };
    Ok(mined)
}

fn caption_model(configured: &str, minimax: bool) -> String {
    let configured = configured.trim();
    if !configured.is_empty() {
        return configured.to_string();
    }
    let from_env = std::env::var("IMAGE_CAPTION_MODEL")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    from_env.unwrap_or_else(|| {
        if minimax {
            "MiniMax-M3".into()
        } else {
            "gpt-4o-mini".into()
        }
    })
}

fn chat_endpoint(base: &str, minimax: bool) -> String {
    let base = base.trim().trim_end_matches('/');
    let base = if base.is_empty() {
        if minimax {
            "https://api.minimaxi.com"
        } else {
            "https://api.openai.com"
        }
    } else {
        base
    };
    if base.ends_with("/chat/completions") {
        base.to_string()
    } else if base.ends_with("/v1") {
        format!("{base}/chat/completions")
    } else {
        format!("{base}/v1/chat/completions")
    }
}

fn image_data_url(path: &Path) -> Result<String, String> {
    let img = image::open(path).map_err(|e| format!("解码图片失败: {e}"))?;
    let img = img.thumbnail(768, 768);
    let mut buf = Cursor::new(Vec::new());
    img.write_to(&mut buf, image::ImageFormat::Jpeg)
        .map_err(|e| format!("压缩参考图失败: {e}"))?;
    let b64 = base64::engine::general_purpose::STANDARD.encode(buf.into_inner());
    Ok(format!("data:image/jpeg;base64,{b64}"))
}

fn extract_chat_text(json: &serde_json::Value) -> Option<String> {
    if let Some(s) = json
        .pointer("/choices/0/message/content")
        .and_then(|v| v.as_str())
    {
        return Some(s.trim().to_string());
    }
    if let Some(arr) = json
        .pointer("/choices/0/message/content")
        .and_then(|v| v.as_array())
    {
        let text = arr
            .iter()
            .filter_map(|p| {
                p.get("text")
                    .and_then(|t| t.as_str())
                    .or_else(|| p.get("content").and_then(|t| t.as_str()))
            })
            .collect::<Vec<_>>()
            .join("");
        if !text.trim().is_empty() {
            return Some(text.trim().to_string());
        }
    }
    json.get("reply")
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
}

fn parse_mined(text: &str) -> MinedPrompt {
    let json_slice = text.find('{').and_then(|i| {
        text.rfind('}').map(|j| {
            if j >= i {
                &text[i..=j]
            } else {
                text
            }
        })
    });
    if let Some(raw) = json_slice {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(raw) {
            let name = v
                .get("name")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .trim()
                .to_string();
            let body = v
                .get("body")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .trim()
                .to_string();
            if !body.is_empty() {
                return MinedPrompt {
                    name: if name.is_empty() {
                        body.chars().take(12).collect()
                    } else {
                        name.chars().take(12).collect()
                    },
                    body,
                    provider: String::new(),
                };
            }
        }
    }
    let body = text.trim().to_string();
    MinedPrompt {
        name: body.chars().take(12).collect(),
        body,
        provider: String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::AppState;
    use image::{Rgb, RgbImage};
    use tempfile::tempdir;

    fn write_png(dir: &Path, name: &str, color: [u8; 3]) -> std::path::PathBuf {
        let mut img = RgbImage::new(32, 32);
        for p in img.pixels_mut() {
            *p = Rgb(color);
        }
        let path = dir.join(name);
        img.save(&path).unwrap();
        path
    }

    #[test]
    fn mock_mine_fills_name_and_body() {
        let dir = tempdir().unwrap();
        let png = write_png(dir.path(), "暖色陶罐.png", [200, 80, 40]);
        let state = AppState::for_test(dir.path().join("lib"));
        let mined = mine_from_image(&state, png.display().to_string()).unwrap();
        assert_eq!(mined.provider, "mock");
        assert!(!mined.name.is_empty());
        assert!(mined.body.contains("mock"));
        assert!(mined.body.contains("暖色") || mined.body.contains("冷调") || mined.body.contains("色"));
    }

    #[test]
    fn mine_rejects_missing_file() {
        let dir = tempdir().unwrap();
        let state = AppState::for_test(dir.path().join("lib"));
        let err = mine_from_image(&state, dir.path().join("nope.jpg").display().to_string())
            .unwrap_err();
        assert!(err.contains("找不到图片"), "{err}");
    }

    #[test]
    fn mine_rejects_empty_path() {
        let dir = tempdir().unwrap();
        let state = AppState::for_test(dir.path().join("lib"));
        let err = mine_from_image(&state, "  ".into()).unwrap_err();
        assert!(err.contains("请先选择参考图"), "{err}");
    }
}
