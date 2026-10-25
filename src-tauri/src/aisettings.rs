//! AI 服务配置：文本 / 图片 / 视频。界面可改，落盘 JSON；无文件时回退环境变量。

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

const MINIMAX_DEFAULT_BASE: &str = "https://api.minimaxi.com";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AiEndpoint {
    pub provider: String,
    pub base_url: String,
    pub api_key: String,
    pub model: String,
}

impl AiEndpoint {
    #[allow(dead_code)]
    pub fn mock() -> Self {
        Self {
            provider: "mock".into(),
            base_url: String::new(),
            api_key: String::new(),
            model: String::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AiServices {
    pub text: AiEndpoint,
    pub image: AiEndpoint,
    pub video: AiEndpoint,
}

impl AiServices {
    #[allow(dead_code)]
    pub fn mock_all() -> Self {
        Self {
            text: AiEndpoint::mock(),
            image: AiEndpoint::mock(),
            video: AiEndpoint::mock(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiEndpointView {
    pub provider: String,
    pub base_url: String,
    pub model: String,
    pub api_key_set: bool,
    pub effective: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiServicesView {
    pub text: AiEndpointView,
    pub image: AiEndpointView,
    pub video: AiEndpointView,
    pub config_path: String,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct AiEndpointPatch {
    pub provider: Option<String>,
    pub base_url: Option<String>,
    pub api_key: Option<String>,
    pub model: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct AiServicesPatch {
    pub text: Option<AiEndpointPatch>,
    pub image: Option<AiEndpointPatch>,
    pub video: Option<AiEndpointPatch>,
}

pub fn config_file() -> PathBuf {
    std::env::var("INSPIREX_AI_CONFIG")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            dirs::home_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join("InspireX")
                .join("ai-services.json")
        })
}

pub fn load(path: &Path) -> AiServices {
    let from_env = from_env_services();
    let from_file = read_file(path);
    match from_file {
        Some(file) => overlay(from_env, file),
        None => from_env,
    }
}

pub fn view_of(services: &AiServices, config_path: &Path) -> AiServicesView {
    AiServicesView {
        text: view_endpoint(&services.text),
        image: view_endpoint(&services.image),
        video: view_endpoint(&services.video),
        config_path: config_path.display().to_string(),
    }
}

pub fn apply_patch(cur: &mut AiServices, patch: AiServicesPatch) {
    if let Some(p) = patch.text {
        apply_endpoint(&mut cur.text, p, "text");
    }
    if let Some(p) = patch.image {
        apply_endpoint(&mut cur.image, p, "image");
    }
    if let Some(p) = patch.video {
        apply_endpoint(&mut cur.video, p, "video");
    }
}

pub fn save(path: &Path, services: &AiServices) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let stored = AiServices {
        text: redact_for_store(&services.text),
        image: redact_for_store(&services.image),
        video: redact_for_store(&services.video),
    };
    let json = serde_json::to_string_pretty(&stored).map_err(|e| e.to_string())?;
    std::fs::write(path, json).map_err(|e| e.to_string())
}

pub fn effective_of(provider: &str, api_key: &str) -> String {
    let p = normalize_provider(provider);
    if p == "mock" {
        return "mock".into();
    }
    if api_key.trim().is_empty() {
        log::warn!(
            target: "inspirex",
            "ai_service_no_key provider={} → mock",
            p
        );
        return "mock".into();
    }
    p
}

fn view_endpoint(ep: &AiEndpoint) -> AiEndpointView {
    AiEndpointView {
        provider: normalize_provider(&ep.provider),
        base_url: ep.base_url.clone(),
        model: ep.model.clone(),
        api_key_set: !ep.api_key.trim().is_empty(),
        effective: effective_of(&ep.provider, &ep.api_key),
    }
}

fn redact_for_store(ep: &AiEndpoint) -> AiEndpoint {
    AiEndpoint {
        provider: normalize_provider(&ep.provider),
        base_url: ep.base_url.trim().to_string(),
        api_key: ep.api_key.trim().to_string(),
        model: ep.model.trim().to_string(),
    }
}

fn apply_endpoint(cur: &mut AiEndpoint, patch: AiEndpointPatch, kind: &str) {
    if let Some(p) = patch.provider {
        cur.provider = normalize_provider(&p);
    }
    if let Some(u) = patch.base_url {
        cur.base_url = u.trim().to_string();
    }
    if let Some(m) = patch.model {
        cur.model = m.trim().to_string();
    }
    if let Some(k) = patch.api_key {
        let t = k.trim();
        if !t.is_empty() && t != "********" {
            cur.api_key = t.to_string();
        }
    }
    fill_kind_defaults(cur, kind);
}

fn fill_kind_defaults(ep: &mut AiEndpoint, kind: &str) {
    if ep.provider != "minimax" {
        return;
    }
    if ep.base_url.trim().is_empty() {
        ep.base_url = MINIMAX_DEFAULT_BASE.into();
    }
    if kind == "image" {
        let m = ep.model.trim().to_ascii_lowercase().replace('_', "-");
        if m == "m3" || m.starts_with("minimax-m") {
            log::warn!(
                target: "inspirex",
                "ai_image_model_coerced from={} to=image-01",
                ep.model
            );
            ep.model = "image-01".into();
        }
    }
    if ep.model.trim().is_empty() {
        ep.model = match kind {
            "image" => "image-01".into(),
            "text" => "MiniMax-M3".into(),
            "video" => "MiniMax-Hailuo-02".into(),
            _ => String::new(),
        };
    }
}

fn overlay(base: AiServices, file: AiServices) -> AiServices {
    AiServices {
        text: overlay_endpoint(base.text, file.text, "text"),
        image: overlay_endpoint(base.image, file.image, "image"),
        video: overlay_endpoint(base.video, file.video, "video"),
    }
}

fn overlay_endpoint(env: AiEndpoint, file: AiEndpoint, kind: &str) -> AiEndpoint {
    let mut out = env;
    if !file.provider.trim().is_empty() {
        out.provider = normalize_provider(&file.provider);
    }
    if !file.base_url.trim().is_empty() {
        out.base_url = file.base_url.trim().to_string();
    }
    if !file.api_key.trim().is_empty() {
        out.api_key = file.api_key.trim().to_string();
    }
    if !file.model.trim().is_empty() {
        out.model = file.model.trim().to_string();
    }
    fill_kind_defaults(&mut out, kind);
    out
}

fn read_file(path: &Path) -> Option<AiServices> {
    let raw = std::fs::read_to_string(path).ok()?;
    let t = raw.trim();
    if t.is_empty() {
        return None;
    }
    serde_json::from_str::<AiServices>(t).ok()
}

fn from_env_services() -> AiServices {
    let shared_key = env_first(&["MINIMAX_API_KEY"]);
    let mut text = AiEndpoint {
        provider: env_first(&["TEXT_GEN_PROVIDER", "IMAGE_GEN_PROVIDER"]),
        base_url: env_first(&["TEXT_GEN_BASE_URL", "IMAGE_GEN_BASE_URL", "MINIMAX_BASE_URL"]),
        api_key: env_first(&["TEXT_GEN_API_KEY", "IMAGE_GEN_API_KEY"]),
        model: env_first(&["TEXT_GEN_MODEL", "MINIMAX_TEXT_MODEL", "IMAGE_CAPTION_MODEL"]),
    };
    if text.api_key.is_empty() {
        text.api_key = shared_key.clone();
    }
    if text.provider.is_empty() {
        // 封装 App 无 .env 时仍带上 MiniMax 文本默认（无 Key 则 effective=mock）
        text.provider = "minimax".into();
    }
    text.provider = normalize_provider(&text.provider);
    fill_kind_defaults(&mut text, "text");

    let mut image = AiEndpoint {
        provider: env_first(&["IMAGE_GEN_PROVIDER"]),
        base_url: env_first(&["IMAGE_GEN_BASE_URL", "MINIMAX_BASE_URL"]),
        api_key: env_first(&["IMAGE_GEN_API_KEY", "MINIMAX_API_KEY"]),
        model: env_first(&["IMAGE_GEN_MODEL", "MINIMAX_IMAGE_MODEL"]),
    };
    if image.provider.is_empty() {
        image.provider = "minimax".into();
    }
    image.provider = normalize_provider(&image.provider);
    fill_kind_defaults(&mut image, "image");

    let mut video = AiEndpoint {
        provider: env_first(&["VIDEO_GEN_PROVIDER"]),
        base_url: env_first(&["VIDEO_GEN_BASE_URL", "MINIMAX_BASE_URL"]),
        api_key: env_first(&["VIDEO_GEN_API_KEY", "MINIMAX_API_KEY"]),
        model: env_first(&["VIDEO_GEN_MODEL"]),
    };
    if video.provider.is_empty() {
        video.provider = "mock".into();
    }
    video.provider = normalize_provider(&video.provider);
    fill_kind_defaults(&mut video, "video");

    AiServices { text, image, video }
}

fn normalize_provider(p: &str) -> String {
    let t = p.trim().to_ascii_lowercase();
    match t.as_str() {
        "" | "mock" => "mock".into(),
        "minimax" => "minimax".into(),
        "openai" | "openai_compat" | "openai-compat" => "openai_compat".into(),
        other => other.to_string(),
    }
}

fn env_first(keys: &[&str]) -> String {
    for k in keys {
        if let Ok(v) = std::env::var(k) {
            let t = v.trim().to_string();
            if !t.is_empty() {
                return t;
            }
        }
    }
    String::new()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn save_roundtrip_and_keep_key_when_blank() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("ai-services.json");
        let mut svc = AiServices::mock_all();
        svc.image.provider = "minimax".into();
        svc.image.api_key = "sk-secret".into();
        save(&path, &svc).unwrap();

        let loaded = load(&path);
        assert_eq!(loaded.image.provider, "minimax");
        assert_eq!(loaded.image.api_key, "sk-secret");
        assert_eq!(loaded.image.base_url, MINIMAX_DEFAULT_BASE);
        assert_eq!(loaded.image.model, "image-01");

        apply_patch(
            &mut svc,
            AiServicesPatch {
                image: Some(AiEndpointPatch {
                    provider: Some("minimax".into()),
                    base_url: Some("https://api.minimaxi.com".into()),
                    api_key: Some("".into()),
                    model: Some("image-01".into()),
                }),
                ..Default::default()
            },
        );
        assert_eq!(svc.image.api_key, "sk-secret");

        let v = view_of(&svc, &path);
        assert!(v.image.api_key_set);
        assert_eq!(v.image.effective, "minimax");
        assert_eq!(effective_of("minimax", ""), "mock");
    }

    #[test]
    fn packaged_defaults_minimax_models_without_key() {
        for k in [
            "TEXT_GEN_PROVIDER",
            "TEXT_GEN_API_KEY",
            "TEXT_GEN_MODEL",
            "IMAGE_GEN_PROVIDER",
            "IMAGE_GEN_API_KEY",
            "IMAGE_GEN_MODEL",
            "MINIMAX_API_KEY",
            "MINIMAX_TEXT_MODEL",
            "IMAGE_CAPTION_MODEL",
        ] {
            std::env::remove_var(k);
        }
        let dir = tempdir().unwrap();
        let path = dir.path().join("missing-ai-services.json");
        let loaded = load(&path);
        assert_eq!(loaded.text.provider, "minimax");
        assert_eq!(loaded.text.model, "MiniMax-M3");
        assert_eq!(loaded.image.provider, "minimax");
        assert_eq!(loaded.image.model, "image-01");
        assert_eq!(effective_of(&loaded.text.provider, &loaded.text.api_key), "mock");
        assert_eq!(effective_of(&loaded.image.provider, &loaded.image.api_key), "mock");
    }
}
