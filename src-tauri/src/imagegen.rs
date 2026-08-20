//! 来源三：生图客户端（Mock First → MiniMax / OpenAI 兼容）。
//! CLIP 的 `AI_MOCK_MODE` 与生图 Provider 独立：配好 Key 即走真实接口。

use crate::db;
use crate::library::{import_paths_with, AppState, ImportOpts};
use crate::models::{GenJob, GenerateRequest};
use image::{Rgb, RgbImage};
use sha2::{Digest, Sha256};
use std::path::PathBuf;
use std::time::Duration;
use uuid::Uuid;

const HTTP_TIMEOUT_SECS: u64 = 120;
const MINIMAX_PROMPT_MAX: usize = 1500;
const MINIMAX_DEFAULT_BASE: &str = "https://api.minimaxi.com";
const MINIMAX_DEFAULT_MODEL: &str = "image-01";

pub fn effective_provider(state: &AppState) -> String {
    let ep = state.ai_image();
    crate::aisettings::effective_of(&ep.provider, &ep.api_key)
}

fn http_agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(HTTP_TIMEOUT_SECS))
        .build()
}

pub fn generate(state: &AppState, req: GenerateRequest) -> Result<GenJob, String> {
    let prompt = req.prompt.trim().to_string();
    if prompt.is_empty() {
        return Err("prompt 不能为空".into());
    }
    let n = req.n.unwrap_or(1).clamp(1, 8);
    let size = req.size.unwrap_or_else(|| "1024x1024".into());
    let (w, h) = parse_size(&size);
    let ep = state.ai_image();
    let provider = crate::aisettings::effective_of(&ep.provider, &ep.api_key);
    let model = default_model(&ep.model, &provider);

    let now = chrono::Utc::now().to_rfc3339();
    let mut job = GenJob {
        id: Uuid::new_v4().to_string(),
        prompt: prompt.clone(),
        n,
        size: format!("{w}x{h}"),
        provider: provider.clone(),
        model: model.clone(),
        status: "running".into(),
        folder_id: req.folder_id.clone(),
        error: None,
        run_id: None,
        imported: 0,
        created_at: now.clone(),
        finished_at: None,
        output_paths: Vec::new(),
    };

    let conn = state.open_db().map_err(|e| e.to_string())?;
    db::insert_gen_job(&conn, &job).map_err(|e| e.to_string())?;
    log::info!(
        target: "inspirex",
        "image_gen_start job_id={} provider={} n={} prompt_len={}",
        job.id,
        provider,
        n,
        prompt.len()
    );

    let result = match provider.as_str() {
        "mock" => mock_files(state, &job.id, &prompt, n as u32, w, h),
        "minimax" => minimax_files(state, &job.id, &prompt, n as u32, w, h, &model),
        _ => openai_compat_files(state, &prompt, n as u32, &job.size, &model),
    };

    match result {
        Ok(paths) => {
            if paths.is_empty() {
                job.status = "error".into();
                job.error = Some("未生成任何文件".into());
                job.finished_at = Some(chrono::Utc::now().to_rfc3339());
                db::update_gen_job(&conn, &job).map_err(|e| e.to_string())?;
                return Err("未生成任何文件".into());
            }
            job.status = "generated".into();
            job.output_paths = paths;
            job.finished_at = Some(chrono::Utc::now().to_rfc3339());
            db::update_gen_job(&conn, &job).map_err(|e| e.to_string())?;
            log::info!(
                target: "inspirex",
                "image_gen_ready job_id={} files={} imported=0",
                job.id,
                job.output_paths.len()
            );
            Ok(job)
        }
        Err(e) => {
            job.status = "error".into();
            job.error = Some(e.clone());
            job.finished_at = Some(chrono::Utc::now().to_rfc3339());
            db::update_gen_job(&conn, &job).map_err(|err| err.to_string())?;
            log::warn!(
                target: "inspirex",
                "image_gen_fail job_id={} err={}",
                job.id,
                e
            );
            Err(e)
        }
    }
}

pub fn import_job(
    state: &AppState,
    job_id: &str,
    folder_id: Option<String>,
) -> Result<GenJob, String> {
    let conn = state.open_db().map_err(|e| e.to_string())?;
    let mut job = db::get_gen_job(&conn, job_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("生图任务不存在: {job_id}"))?;
    if job.status == "done" && job.imported > 0 {
        return Err("该任务已入库".into());
    }
    if job.status != "generated" {
        return Err(format!("当前状态不能入库: {}", job.status));
    }
    let paths: Vec<String> = job
        .output_paths
        .iter()
        .filter(|p| std::path::Path::new(p).is_file())
        .cloned()
        .collect();
    if paths.is_empty() {
        return Err("生成文件已丢失，请重新生成".into());
    }
    let dest_folder = folder_id.or_else(|| job.folder_id.clone());
    log::info!(
        target: "inspirex",
        "image_gen_import_start job_id={} files={} folder={:?}",
        job.id,
        paths.len(),
        dest_folder
    );
    let import = import_paths_with(
        state,
        paths,
        ImportOpts {
            folder_id: dest_folder.clone(),
            source_url: None,
            notes: Some(format!(
                "prompt: {}\nmodel: {}\nprovider: {}",
                job.prompt, job.model, job.provider
            )),
            source_kind: "ai".to_string(),
            source_ref: Some(job.id.clone()),
        },
    )?;
    job.folder_id = dest_folder;
    job.status = "done".into();
    job.imported = import.imported as i64;
    job.run_id = Some(import.run_id);
    job.finished_at = Some(chrono::Utc::now().to_rfc3339());
    if !import.errors.is_empty() {
        job.error = Some(import.errors.join(" | "));
    }
    db::update_gen_job(&conn, &job).map_err(|e| e.to_string())?;
    log::info!(
        target: "inspirex",
        "image_gen_import_done job_id={} run_id={:?} imported={}",
        job.id,
        job.run_id,
        job.imported
    );
    Ok(job)
}

fn default_model(configured: &str, provider: &str) -> String {
    let configured = configured.trim();
    if !configured.is_empty() {
        return configured.to_string();
    }
    match provider {
        "mock" => "mock-image-v1".into(),
        "minimax" => MINIMAX_DEFAULT_MODEL.into(),
        _ => "dall-e-3".into(),
    }
}

fn parse_size(size: &str) -> (u32, u32) {
    let mut parts = size.split(['x', '×', '*']);
    let w = parts.next().and_then(|s| s.parse().ok()).unwrap_or(1024);
    let h = parts.next().and_then(|s| s.parse().ok()).unwrap_or(w);
    (w.clamp(64, 2048), h.clamp(64, 2048))
}

fn align_minimax_px(v: u32) -> u32 {
    let v = v.clamp(512, 2048);
    (v / 8) * 8
}

fn minimax_status_msg(code: i64, msg: &str) -> String {
    let hint = match code {
        1002 => "限流，请稍后重试",
        1004 | 2049 => "鉴权失败，检查 IMAGE_GEN_API_KEY / MINIMAX_API_KEY（生图需按量计费 Key）",
        1008 => "余额不足",
        1026 => "prompt 含敏感内容",
        2013 => "参数无效（尺寸需 512–2048 且为 8 的倍数）",
        _ => "",
    };
    if hint.is_empty() {
        format!("MiniMax 业务错误 {code}: {msg}")
    } else {
        format!("MiniMax 业务错误 {code}: {msg}（{hint}）")
    }
}

fn mock_files(
    state: &AppState,
    job_id: &str,
    prompt: &str,
    n: u32,
    w: u32,
    h: u32,
) -> Result<Vec<String>, String> {
    state.ensure_dirs().map_err(|e| e.to_string())?;
    let mut paths = Vec::new();
    for i in 0..n {
        let rgb = color_from_seed(prompt, i);
        let mut img = RgbImage::new(w, h);
        for (x, y, p) in img.enumerate_pixels_mut() {
            let mix = ((x + y + i * 17) % 32) as u8;
            *p = Rgb([
                rgb[0].saturating_add(mix / 3),
                rgb[1].saturating_add(mix / 4),
                rgb[2].saturating_add(mix / 5),
            ]);
        }
        let path = state
            .library_path
            .join("inbox")
            .join(format!("gen-{job_id}-{i}.png"));
        img.save(&path).map_err(|e| format!("写 mock 图失败: {e}"))?;
        paths.push(path.display().to_string());
    }
    Ok(paths)
}

fn color_from_seed(prompt: &str, i: u32) -> [u8; 3] {
    let mut hasher = Sha256::new();
    hasher.update(prompt.as_bytes());
    hasher.update(i.to_le_bytes());
    let d = hasher.finalize();
    [d[0], d[1], d[2]]
}

fn minimax_endpoint(base: &str) -> String {
    let base = base.trim().trim_end_matches('/');
    let base = if base.is_empty() {
        MINIMAX_DEFAULT_BASE
    } else {
        base
    };
    if base.ends_with("/image_generation") {
        base.to_string()
    } else if base.ends_with("/v1") {
        format!("{base}/image_generation")
    } else {
        format!("{base}/v1/image_generation")
    }
}

#[derive(Debug)]
enum MiniMaxImage {
    Url(String),
    B64(String),
}

fn parse_minimax_images(json: &serde_json::Value) -> Result<Vec<MiniMaxImage>, String> {
    if let Some(code) = json
        .pointer("/base_resp/status_code")
        .and_then(|v| v.as_i64())
    {
        if code != 0 {
            let msg = json
                .pointer("/base_resp/status_msg")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            return Err(minimax_status_msg(code, msg));
        }
    }
    let data = json.get("data").ok_or_else(|| format!("MiniMax 响应缺 data: {json}"))?;
    let mut out = Vec::new();
    if let Some(urls) = data.get("image_urls").and_then(|v| v.as_array()) {
        for u in urls {
            if let Some(s) = u.as_str().filter(|s| !s.is_empty()) {
                out.push(MiniMaxImage::Url(s.to_string()));
            }
        }
    }
    if let Some(b64s) = data.get("image_base64").and_then(|v| v.as_array()) {
        for b in b64s {
            if let Some(s) = b.as_str().filter(|s| !s.is_empty()) {
                out.push(MiniMaxImage::B64(s.to_string()));
            }
        }
    }
    if out.is_empty() {
        return Err(format!("MiniMax 未返回 image_urls/image_base64: {json}"));
    }
    Ok(out)
}

fn ext_from_bytes(bytes: &[u8]) -> &'static str {
    if bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
        "png"
    } else if bytes.len() >= 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP" {
        "webp"
    } else if bytes.starts_with(&[0xFF, 0xD8]) {
        "jpg"
    } else {
        "jpg"
    }
}

fn write_image_bytes(state: &AppState, job_id: &str, i: usize, bytes: &[u8]) -> Result<String, String> {
    let ext = ext_from_bytes(bytes);
    let dest: PathBuf = state
        .library_path
        .join("inbox")
        .join(format!("gen-minimax-{job_id}-{i}.{ext}"));
    std::fs::write(&dest, bytes).map_err(|e| e.to_string())?;
    Ok(dest.display().to_string())
}

fn ureq_err(context: &str, e: ureq::Error) -> String {
    match e {
        ureq::Error::Status(code, resp) => {
            let body = resp.into_string().unwrap_or_default();
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(&body) {
                if let Some(status) = json.pointer("/base_resp/status_code").and_then(|v| v.as_i64()) {
                    let msg = json
                        .pointer("/base_resp/status_msg")
                        .and_then(|v| v.as_str())
                        .unwrap_or(&body);
                    return minimax_status_msg(status, msg);
                }
            }
            format!("{context} HTTP {code}: {body}")
        }
        other => format!("{context}: {other}"),
    }
}

fn minimax_files(
    state: &AppState,
    job_id: &str,
    prompt: &str,
    n: u32,
    w: u32,
    h: u32,
    model: &str,
) -> Result<Vec<String>, String> {
    let ep = state.ai_image();
    let url = minimax_endpoint(&ep.base_url);
    let prompt: String = prompt.chars().take(MINIMAX_PROMPT_MAX).collect();
    let width = align_minimax_px(w);
    let height = align_minimax_px(h);
    let body = serde_json::json!({
        "model": model,
        "prompt": prompt,
        "n": n,
        "width": width,
        "height": height,
        "response_format": "url",
        "prompt_optimizer": false,
    });
    log::info!(
        target: "inspirex",
        "minimax_image_req job_id={} model={} n={} size={}x{} url={}",
        job_id,
        model,
        n,
        width,
        height,
        url
    );
    let resp = http_agent()
        .post(&url)
        .set(
            "Authorization",
            &format!("Bearer {}", ep.api_key.trim()),
        )
        .set("Content-Type", "application/json")
        .send_json(body)
        .map_err(|e| ureq_err("MiniMax 生图请求失败", e))?;
    let json: serde_json::Value = resp
        .into_json()
        .map_err(|e| format!("MiniMax 响应不是 JSON: {e}"))?;
    let images = parse_minimax_images(&json)?;
    let trace = json.get("id").and_then(|v| v.as_str()).unwrap_or("-");
    log::info!(
        target: "inspirex",
        "minimax_image_ok job_id={} trace_id={} count={}",
        job_id,
        trace,
        images.len()
    );

    state.ensure_dirs().map_err(|e| e.to_string())?;
    let mut paths = Vec::new();
    for (i, item) in images.into_iter().enumerate() {
        match item {
            MiniMaxImage::Url(img_url) => {
                let tmp: PathBuf = state
                    .library_path
                    .join("inbox")
                    .join(format!("gen-minimax-{job_id}-{i}.bin"));
                crate::library::download_file(&img_url, &tmp)?;
                let bytes = std::fs::read(&tmp).map_err(|e| e.to_string())?;
                let _ = std::fs::remove_file(&tmp);
                paths.push(write_image_bytes(state, job_id, i, &bytes)?);
            }
            MiniMaxImage::B64(b64) => {
                let bytes = decode_b64(&b64)?;
                paths.push(write_image_bytes(state, job_id, i, &bytes)?);
            }
        }
    }
    if paths.is_empty() {
        return Err("MiniMax 未返回图片".into());
    }
    Ok(paths)
}

fn openai_compat_files(
    state: &AppState,
    prompt: &str,
    n: u32,
    size: &str,
    model: &str,
) -> Result<Vec<String>, String> {
    let ep = state.ai_image();
    let base = ep.base_url.trim().trim_end_matches('/');
    if base.is_empty() {
        return Err("图片服务 Base URL 未配置，请在设置中填写".into());
    }
    let url = if base.ends_with("/images/generations") {
        base.to_string()
    } else {
        format!("{base}/images/generations")
    };
    let body = serde_json::json!({
        "model": model,
        "prompt": prompt,
        "n": n,
        "size": size,
        "response_format": "url",
    });
    let resp = http_agent()
        .post(&url)
        .set("Authorization", &format!("Bearer {}", ep.api_key.trim()))
        .set("Content-Type", "application/json")
        .send_json(body)
        .map_err(|e| format!("生图 API 失败: {e}"))?;
    let json: serde_json::Value = resp
        .into_json()
        .map_err(|e| format!("生图响应不是 JSON: {e}"))?;
    let data = json
        .get("data")
        .and_then(|d| d.as_array())
        .ok_or_else(|| format!("生图响应缺 data: {json}"))?;

    state.ensure_dirs().map_err(|e| e.to_string())?;
    let mut paths = Vec::new();
    for (i, item) in data.iter().enumerate() {
        let dest: PathBuf = state
            .library_path
            .join("inbox")
            .join(format!("gen-api-{i}-{}.png", Uuid::new_v4()));
        if let Some(b64) = item.get("b64_json").and_then(|v| v.as_str()) {
            let bytes = decode_b64(b64)?;
            std::fs::write(&dest, bytes).map_err(|e| e.to_string())?;
        } else if let Some(img_url) = item.get("url").and_then(|v| v.as_str()) {
            crate::library::download_file(img_url, &dest)?;
        } else {
            return Err("条目既无 url 也无 b64_json".into());
        }
        paths.push(dest.display().to_string());
    }
    if paths.is_empty() {
        return Err("生图 API 未返回图片".into());
    }
    Ok(paths)
}

fn decode_b64(s: &str) -> Result<Vec<u8>, String> {
    fn val(c: u8) -> Option<u8> {
        match c {
            b'A'..=b'Z' => Some(c - b'A'),
            b'a'..=b'z' => Some(c - b'a' + 26),
            b'0'..=b'9' => Some(c - b'0' + 52),
            b'+' => Some(62),
            b'/' => Some(63),
            _ => None,
        }
    }
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len() * 3 / 4);
    let mut buf = [0u8; 4];
    let mut n = 0;
    for &c in bytes {
        if c == b'=' || c.is_ascii_whitespace() {
            continue;
        }
        buf[n] = val(c).ok_or_else(|| "非法 base64".to_string())?;
        n += 1;
        if n == 4 {
            out.push((buf[0] << 2) | (buf[1] >> 4));
            out.push((buf[1] << 4) | (buf[2] >> 2));
            out.push((buf[2] << 6) | buf[3]);
            n = 0;
        }
    }
    if n == 2 {
        out.push((buf[0] << 2) | (buf[1] >> 4));
    } else if n == 3 {
        out.push((buf[0] << 2) | (buf[1] >> 4));
        out.push((buf[1] << 4) | (buf[2] >> 2));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::AppState;
    use tempfile::tempdir;

    #[test]
    fn mock_single_and_batch() {
        let dir = tempdir().unwrap();
        let lib = dir.path().join("Library");
        std::env::set_var("INSPIREX_LIBRARY_PATH", &lib);
        std::env::set_var("AI_MOCK_MODE", "true");
        std::env::set_var("IMAGE_GEN_PROVIDER", "mock");
        let state = AppState::from_env();
        state.ensure_dirs().unwrap();

        let one = generate(
            &state,
            GenerateRequest {
                prompt: "red ceramic bowl".into(),
                n: Some(1),
                size: Some("128x128".into()),
                folder_id: None,
            },
        )
        .unwrap();
        assert_eq!(one.status, "generated");
        assert_eq!(one.imported, 0);
        assert_eq!(one.output_paths.len(), 1);
        assert_eq!(one.provider, "mock");

        let conn = state.open_db().unwrap();
        let before = db::list_assets(&conn, None, 20).unwrap();
        assert!(!before.iter().any(|a| a.source_ref.as_deref() == Some(one.id.as_str())));
        drop(conn);

        let imported = import_job(&state, &one.id, None).unwrap();
        assert_eq!(imported.status, "done");
        assert_eq!(imported.imported, 1);

        let batch = generate(
            &state,
            GenerateRequest {
                prompt: "blue tiles pattern".into(),
                n: Some(4),
                size: Some("64x64".into()),
                folder_id: None,
            },
        )
        .unwrap();
        assert_eq!(batch.status, "generated");
        assert_eq!(batch.output_paths.len(), 4);
        let batch_in = import_job(&state, &batch.id, None).unwrap();
        assert_eq!(batch_in.imported, 4);

        let conn = state.open_db().unwrap();
        let assets = db::list_assets(&conn, None, 20).unwrap();
        assert!(assets.iter().any(|a| a.source_kind == "ai"));
        let ai = assets.iter().find(|a| a.source_ref.as_deref() == Some(batch.id.as_str()));
        assert!(ai.unwrap().notes.as_deref().unwrap().contains("blue tiles"));
        let jobs = db::list_gen_jobs(&conn, 10).unwrap();
        assert!(jobs.len() >= 2);
        assert!(import_job(&state, &one.id, None).is_err());
    }

    #[test]
    fn image_gen_provider_independent_of_clip_mock() {
        let dir = tempdir().unwrap();
        std::env::set_var("INSPIREX_LIBRARY_PATH", dir.path().join("Library"));
        std::env::set_var("AI_MOCK_MODE", "true");
        let cfg = dir.path().join("ai-services.json");
        std::env::set_var("INSPIREX_AI_CONFIG", &cfg);
        std::env::remove_var("MINIMAX_API_KEY");
        std::env::remove_var("MINIMAX_IMAGE_MODEL");
        std::env::remove_var("TEXT_GEN_PROVIDER");
        std::env::remove_var("TEXT_GEN_API_KEY");

        std::env::set_var("IMAGE_GEN_PROVIDER", "minimax");
        std::env::set_var("IMAGE_GEN_API_KEY", "sk-test");
        std::env::set_var("IMAGE_GEN_BASE_URL", "https://api.minimaxi.com");
        std::env::set_var("IMAGE_GEN_MODEL", "");
        let with_key = AppState::from_env();
        assert_eq!(effective_provider(&with_key), "minimax");
        assert_eq!(default_model(&with_key.ai_image().model, "minimax"), "image-01");

        std::env::remove_var("IMAGE_GEN_API_KEY");
        let no_key = AppState::from_env();
        assert_eq!(effective_provider(&no_key), "mock");

        std::env::set_var("IMAGE_GEN_PROVIDER", "mock");
        std::env::remove_var("INSPIREX_AI_CONFIG");
    }

    #[test]
    fn parse_minimax_ok_and_auth_error() {
        let ok = serde_json::json!({
            "id": "trace-1",
            "data": { "image_urls": ["https://cdn.example/a.jpg"] },
            "base_resp": { "status_code": 0, "status_msg": "success" }
        });
        let images = parse_minimax_images(&ok).unwrap();
        assert_eq!(images.len(), 1);

        let err = serde_json::json!({
            "base_resp": { "status_code": 2049, "status_msg": "invalid api key" }
        });
        let msg = parse_minimax_images(&err).unwrap_err();
        assert!(msg.contains("2049"));
        assert!(msg.contains("鉴权"));
    }

    #[test]
    fn minimax_endpoint_and_align() {
        assert_eq!(
            minimax_endpoint("https://api.minimaxi.com"),
            "https://api.minimaxi.com/v1/image_generation"
        );
        assert_eq!(
            minimax_endpoint("https://api.minimaxi.com/v1"),
            "https://api.minimaxi.com/v1/image_generation"
        );
        assert_eq!(align_minimax_px(500), 512);
        assert_eq!(align_minimax_px(1024), 1024);
        assert_eq!(align_minimax_px(770), 768);
    }
}
