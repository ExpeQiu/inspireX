//! 本地 CLIP：Mock 确定性向量 / 真实 sidecar（transformers）。

use crate::pipeline::{mock_embed_file, mock_embed_text, MOCK_MODEL};
use parking_lot::Mutex;
use serde::Deserialize;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::Arc;

pub const REAL_MODEL_DEFAULT: &str = "openai/clip-vit-base-patch32";

#[derive(Debug, Clone)]
pub struct EmbedResult {
    pub vector: Vec<f32>,
    pub model: String,
}

struct SidecarProc {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<std::process::ChildStdout>,
}

pub struct ClipService {
    mock: bool,
    python: PathBuf,
    script: PathBuf,
    model_id: String,
    inner: Mutex<Option<SidecarProc>>,
}

#[derive(Debug, Deserialize)]
struct SidecarResp {
    ok: bool,
    #[serde(default)]
    vector: Option<Vec<f32>>,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    error: Option<String>,
}

impl ClipService {
    pub fn from_env(project_root_hint: &Path) -> Arc<Self> {
        let mock = std::env::var("AI_MOCK_MODE")
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(true);
        let python = std::env::var("CLIP_PYTHON")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("python3"));
        let script = std::env::var("CLIP_SIDECAR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| {
                let a = project_root_hint.join("scripts/clip_embed.py");
                if a.exists() {
                    a
                } else {
                    PathBuf::from("scripts/clip_embed.py")
                }
            });
        let model_id =
            std::env::var("CLIP_MODEL").unwrap_or_else(|_| REAL_MODEL_DEFAULT.to_string());
        Arc::new(Self {
            mock,
            python,
            script,
            model_id,
            inner: Mutex::new(None),
        })
    }

    pub fn model_name(&self) -> &str {
        if self.mock {
            MOCK_MODEL
        } else {
            &self.model_id
        }
    }

    pub fn embed_image(&self, path: &Path) -> Result<EmbedResult, String> {
        if self.mock {
            return Ok(EmbedResult {
                vector: mock_embed_file(path)?,
                model: MOCK_MODEL.to_string(),
            });
        }
        self.call_sidecar(serde_json::json!({
            "op": "embed_image",
            "path": path.display().to_string(),
        }))
    }

    pub fn embed_text(&self, text: &str) -> Result<EmbedResult, String> {
        if self.mock {
            return Ok(EmbedResult {
                vector: mock_embed_text(text),
                model: MOCK_MODEL.to_string(),
            });
        }
        self.call_sidecar(serde_json::json!({
            "op": "embed_text",
            "text": text,
        }))
    }

    fn call_sidecar(&self, req: serde_json::Value) -> Result<EmbedResult, String> {
        let line = serde_json::to_string(&req).map_err(|e| e.to_string())?;
        let mut guard = self.inner.lock();
        if guard.is_none() {
            *guard = Some(self.spawn()?);
        }

        let write_ok = {
            let proc = guard.as_mut().unwrap();
            writeln!(proc.stdin, "{line}").is_ok() && proc.stdin.flush().is_ok()
        };
        if !write_ok {
            log::warn!(target: "inspirex", "clip_sidecar_write_fail; respawn");
            if let Some(mut old) = guard.take() {
                let _ = old.child.kill();
            }
            *guard = Some(self.spawn()?);
            let proc = guard.as_mut().unwrap();
            writeln!(proc.stdin, "{line}").map_err(|e| e.to_string())?;
            proc.stdin.flush().map_err(|e| e.to_string())?;
        }

        let proc = guard.as_mut().unwrap();
        let mut resp_line = String::new();
        proc.stdout
            .read_line(&mut resp_line)
            .map_err(|e| format!("clip_sidecar_read: {e}"))?;
        if resp_line.trim().is_empty() {
            return Err("clip_sidecar 无响应（进程可能已退出）".into());
        }
        let resp: SidecarResp =
            serde_json::from_str(resp_line.trim()).map_err(|e| format!("clip_sidecar_json: {e}"))?;
        if !resp.ok {
            return Err(resp.error.unwrap_or_else(|| "clip_sidecar failed".into()));
        }
        let vector = resp.vector.ok_or_else(|| "clip_sidecar 缺 vector".to_string())?;
        Ok(EmbedResult {
            vector,
            model: resp.model.unwrap_or_else(|| self.model_id.clone()),
        })
    }

    fn spawn(&self) -> Result<SidecarProc, String> {
        if !self.script.exists() {
            return Err(format!(
                "找不到 CLIP sidecar: {}（设 CLIP_SIDECAR）",
                self.script.display()
            ));
        }
        log::info!(
            target: "inspirex",
            "clip_sidecar_spawn python={} script={} model={}",
            self.python.display(),
            self.script.display(),
            self.model_id
        );
        let mut child = Command::new(&self.python)
            .arg(&self.script)
            .env("CLIP_MODEL", &self.model_id)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|e| format!("启动 clip sidecar 失败: {e}"))?;
        let stdin = child.stdin.take().ok_or("sidecar stdin")?;
        let stdout = child.stdout.take().ok_or("sidecar stdout")?;
        let mut proc = SidecarProc {
            child,
            stdin,
            stdout: BufReader::new(stdout),
        };
        // 健康检查
        writeln!(proc.stdin, r#"{{"op":"ping"}}"#).map_err(|e| e.to_string())?;
        proc.stdin.flush().map_err(|e| e.to_string())?;
        let mut line = String::new();
        proc.stdout
            .read_line(&mut line)
            .map_err(|e| format!("clip ping: {e}"))?;
        let resp: SidecarResp =
            serde_json::from_str(line.trim()).map_err(|e| format!("clip ping json: {e}"))?;
        if !resp.ok {
            let _ = proc.child.kill();
            return Err(resp
                .error
                .unwrap_or_else(|| "clip ping failed".into()));
        }
        log::info!(target: "inspirex", "clip_sidecar_ready model={}", self.model_id);
        Ok(proc)
    }
}

impl Drop for ClipService {
    fn drop(&mut self) {
        if let Some(mut p) = self.inner.lock().take() {
            let _ = p.child.kill();
        }
    }
}
