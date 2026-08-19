//! 本地采集 HTTP：供浏览器插件 POST 入库。
//! 同时提供外置卷浏览回退（浏览器预览 / IPC 未注入时）。
//! 默认 127.0.0.1:18765，Bearer token 鉴权（浏览 GET 仅本机、不鉴权）。

use crate::browse::{
    add_index_root, add_local_index_root, ensure_readable, ensure_under_kind, list_browse_for_kind,
    list_index_roots, list_local_index_roots, parse_browse_kind, remove_index_root,
    remove_local_index_root, root_for_kind,
};
use crate::db;
use crate::library::{create_folder, import_from_url, import_paths, library_info, AppState};
use crate::models::{
    IngestCreateFolderRequest, IngestImportBatchRequest, IngestImportPathsRequest, IngestImportRequest,
};
use crate::pipeline::guess_mime;
use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tiny_http::{Header, Method, Response, Server, StatusCode};

pub fn spawn_ingest_server(state: Arc<AppState>) {
    let port = state.ingest_port;
    let addr = format!("127.0.0.1:{port}");
    std::thread::Builder::new()
        .name("inspirex-ingest".into())
        .spawn(move || {
            let server = match Server::http(&addr) {
                Ok(s) => s,
                Err(e) => {
                    log::error!(target: "inspirex", "ingest_bind_fail addr={} err={}", addr, e);
                    return;
                }
            };
            log::info!(
                target: "inspirex",
                "ingest_listen addr={} token_set={}",
                addr,
                !state.ingest_token.is_empty()
            );
            for mut request in server.incoming_requests() {
                let method = request.method().clone();
                let url = request.url().to_string();
                let path = url.split('?').next().unwrap_or(&url);

                // CORS preflight
                if method == Method::Options {
                    let _ = request.respond(cors_empty(StatusCode(204)));
                    continue;
                }

                if path == "/health" && method == Method::Get {
                    let body = r#"{"ok":true,"service":"inspirex-ingest"}"#;
                    let _ = request.respond(cors_json(StatusCode(200), body));
                    continue;
                }

                if path == "/v1/browse/root" && method == Method::Get {
                    let kind = parse_browse_kind(query_param(&url, "kind").as_deref());
                    let root = root_for_kind(kind);
                    log::info!(
                        target: "inspirex",
                        "http_browse_root kind={} root={}",
                        kind,
                        root.display()
                    );
                    let body = serde_json::to_string(&root.display().to_string())
                        .unwrap_or_else(|_| "\"/Volumes\"".into());
                    let _ = request.respond(cors_json(StatusCode(200), &body));
                    continue;
                }

                if path == "/v1/browse/list" && method == Method::Get {
                    let kind = parse_browse_kind(query_param(&url, "kind").as_deref()).to_string();
                    let target = query_param(&url, "path")
                        .filter(|s| !s.trim().is_empty())
                        .map(PathBuf::from);
                    let path_log = target
                        .as_ref()
                        .map(|p| p.display().to_string())
                        .unwrap_or_default();
                    let limit = query_param(&url, "limit")
                        .and_then(|s| s.parse().ok())
                        .unwrap_or(72usize);
                    let offset = query_param(&url, "offset")
                        .and_then(|s| s.parse().ok())
                        .unwrap_or(0usize);
                    match list_browse_for_kind(target, &kind, limit, offset) {
                        Ok(listing) => match serde_json::to_string(&listing) {
                            Ok(body) => {
                                let _ = request.respond(cors_json(StatusCode(200), &body));
                            }
                            Err(e) => {
                                let _ = request.respond(cors_json(
                                    StatusCode(500),
                                    &json_err(&e.to_string()),
                                ));
                            }
                        },
                        Err(e) => {
                            log::warn!(
                                target: "inspirex",
                                "http_browse_list_fail path={} err={}",
                                path_log,
                                e
                            );
                            let _ = request.respond(cors_json(StatusCode(400), &json_err(&e)));
                        }
                    }
                    continue;
                }

                if path == "/v1/browse/index-roots" && method == Method::Get {
                    let kind = parse_browse_kind(query_param(&url, "kind").as_deref());
                    let list = list_index_roots(kind);
                    let body = serde_json::to_string(&list).unwrap_or_else(|_| "[]".into());
                    let _ = request.respond(cors_json(StatusCode(200), &body));
                    continue;
                }

                if path == "/v1/browse/index-roots" && method == Method::Post {
                    match read_json::<serde_json::Value>(&mut request) {
                        Ok(v) => {
                            let kind = parse_browse_kind(v.get("kind").and_then(|x| x.as_str()));
                            let raw = v
                                .get("path")
                                .and_then(|x| x.as_str())
                                .unwrap_or("")
                                .trim()
                                .to_string();
                            match add_index_root(kind, &PathBuf::from(raw)) {
                                Ok(list) => {
                                    let body =
                                        serde_json::to_string(&list).unwrap_or_else(|_| "[]".into());
                                    let _ = request.respond(cors_json(StatusCode(200), &body));
                                }
                                Err(e) => {
                                    let _ = request
                                        .respond(cors_json(StatusCode(400), &json_err(&e)));
                                }
                            }
                        }
                        Err(e) => {
                            let _ = request.respond(cors_json(StatusCode(400), &json_err(&e)));
                        }
                    }
                    continue;
                }

                if path == "/v1/browse/index-roots/remove" && method == Method::Post {
                    match read_json::<serde_json::Value>(&mut request) {
                        Ok(v) => {
                            let kind = parse_browse_kind(v.get("kind").and_then(|x| x.as_str()));
                            let raw = v
                                .get("path")
                                .and_then(|x| x.as_str())
                                .unwrap_or("")
                                .trim()
                                .to_string();
                            match remove_index_root(kind, &PathBuf::from(raw)) {
                                Ok(list) => {
                                    let body =
                                        serde_json::to_string(&list).unwrap_or_else(|_| "[]".into());
                                    let _ = request.respond(cors_json(StatusCode(200), &body));
                                }
                                Err(e) => {
                                    let _ = request
                                        .respond(cors_json(StatusCode(400), &json_err(&e)));
                                }
                            }
                        }
                        Err(e) => {
                            let _ = request.respond(cors_json(StatusCode(400), &json_err(&e)));
                        }
                    }
                    continue;
                }

                if path == "/v1/browse/local-roots" && method == Method::Get {
                    let list = list_local_index_roots();
                    let body = serde_json::to_string(&list).unwrap_or_else(|_| "[]".into());
                    let _ = request.respond(cors_json(StatusCode(200), &body));
                    continue;
                }

                if path == "/v1/browse/local-roots" && method == Method::Post {
                    match read_json::<serde_json::Value>(&mut request) {
                        Ok(v) => {
                            let raw = v
                                .get("path")
                                .and_then(|x| x.as_str())
                                .unwrap_or("")
                                .trim()
                                .to_string();
                            match add_local_index_root(&PathBuf::from(raw)) {
                                Ok(list) => {
                                    let body =
                                        serde_json::to_string(&list).unwrap_or_else(|_| "[]".into());
                                    let _ = request.respond(cors_json(StatusCode(200), &body));
                                }
                                Err(e) => {
                                    let _ = request
                                        .respond(cors_json(StatusCode(400), &json_err(&e)));
                                }
                            }
                        }
                        Err(e) => {
                            let _ = request.respond(cors_json(StatusCode(400), &json_err(&e)));
                        }
                    }
                    continue;
                }

                if path == "/v1/browse/local-roots/remove" && method == Method::Post {
                    match read_json::<serde_json::Value>(&mut request) {
                        Ok(v) => {
                            let raw = v
                                .get("path")
                                .and_then(|x| x.as_str())
                                .unwrap_or("")
                                .trim()
                                .to_string();
                            match remove_local_index_root(&PathBuf::from(raw)) {
                                Ok(list) => {
                                    let body =
                                        serde_json::to_string(&list).unwrap_or_else(|_| "[]".into());
                                    let _ = request.respond(cors_json(StatusCode(200), &body));
                                }
                                Err(e) => {
                                    let _ = request
                                        .respond(cors_json(StatusCode(400), &json_err(&e)));
                                }
                            }
                        }
                        Err(e) => {
                            let _ = request.respond(cors_json(StatusCode(400), &json_err(&e)));
                        }
                    }
                    continue;
                }

                if path == "/v1/browse/by-tag" && method == Method::Get {
                    let Some(tag) = query_param(&url, "tag").filter(|s| !s.trim().is_empty()) else {
                        let _ = request.respond(cors_json(
                            StatusCode(400),
                            &json_err("tag required"),
                        ));
                        continue;
                    };
                    let kind = parse_browse_kind(query_param(&url, "kind").as_deref()).to_string();
                    let root = root_for_kind(&kind);
                    let target = query_param(&url, "path")
                        .filter(|s| !s.trim().is_empty())
                        .map(PathBuf::from)
                        .unwrap_or_else(|| root.clone());
                    let limit = query_param(&url, "limit")
                        .and_then(|s| s.parse().ok())
                        .unwrap_or(72usize);
                    let offset = query_param(&url, "offset")
                        .and_then(|s| s.parse().ok())
                        .unwrap_or(0usize);
                    match ensure_under_kind(&target, &kind).and_then(|safe| {
                        let scope = crate::finder_tags::tag_scope_for(&safe);
                        let scope = ensure_under_kind(&scope, &kind)?;
                        crate::finder_tags::list_by_finder_tag(&scope, &tag, limit, offset, &root)
                    }) {
                        Ok(listing) => match serde_json::to_string(&listing) {
                            Ok(body) => {
                                let _ = request.respond(cors_json(StatusCode(200), &body));
                            }
                            Err(e) => {
                                let _ = request.respond(cors_json(
                                    StatusCode(500),
                                    &json_err(&e.to_string()),
                                ));
                            }
                        },
                        Err(e) => {
                            log::warn!(
                                target: "inspirex",
                                "http_browse_by_tag_fail tag={} path={} err={}",
                                tag,
                                target.display(),
                                e
                            );
                            let _ = request.respond(cors_json(StatusCode(400), &json_err(&e)));
                        }
                    }
                    continue;
                }

                if path == "/v1/browse/thumb" && method == Method::Get {
                    let Some(raw) = query_param(&url, "path") else {
                        let _ = request.respond(cors_json(
                            StatusCode(400),
                            &json_err("path required"),
                        ));
                        continue;
                    };
                    let side = query_param(&url, "maxSide")
                        .and_then(|s| s.parse().ok())
                        .unwrap_or(240u32);
                    let p = PathBuf::from(&raw);
                    match ensure_readable(&p).and_then(|safe| {
                        crate::thumb::ensure_image_thumb(&safe, &state.library_path, side)
                            .map(|t| t.display().to_string())
                    }) {
                        Ok(thumb) => {
                            let body = serde_json::to_string(&thumb).unwrap_or_default();
                            let _ = request.respond(cors_json(StatusCode(200), &body));
                        }
                        Err(e) => {
                            let _ = request.respond(cors_json(StatusCode(400), &json_err(&e)));
                        }
                    }
                    continue;
                }

                if path == "/v1/browse/file" && method == Method::Get {
                    let Some(raw) = query_param(&url, "path") else {
                        let _ = request.respond(cors_json(
                            StatusCode(400),
                            &json_err("path required"),
                        ));
                        continue;
                    };
                    match allow_read_path(&state, Path::new(&raw)) {
                        Ok(safe) => match std::fs::read(&safe) {
                            Ok(data) => {
                                let mime = guess_mime(&safe).unwrap_or("application/octet-stream");
                                let _ = request.respond(cors_bytes(StatusCode(200), mime, data));
                            }
                            Err(e) => {
                                let _ = request.respond(cors_json(
                                    StatusCode(404),
                                    &json_err(&e.to_string()),
                                ));
                            }
                        },
                        Err(e) => {
                            let _ = request.respond(cors_json(StatusCode(403), &json_err(&e)));
                        }
                    }
                    continue;
                }

                if path == "/v1/invoke" && method == Method::Post {
                    match read_json::<crate::models::InvokeRequest>(&mut request) {
                        Ok(req) => {
                            let cmd = req.cmd.trim().to_string();
                            log::info!(target: "inspirex", "http_invoke cmd={}", cmd);
                            match crate::rpc::dispatch(&state, &cmd, req.args) {
                                Ok(v) => {
                                    let body =
                                        serde_json::to_string(&v).unwrap_or_else(|_| "null".into());
                                    let _ = request.respond(cors_json(StatusCode(200), &body));
                                }
                                Err(e) => {
                                    log::warn!(
                                        target: "inspirex",
                                        "http_invoke_fail cmd={} err={}",
                                        cmd,
                                        e
                                    );
                                    let _ = request.respond(cors_json(StatusCode(400), &json_err(&e)));
                                }
                            }
                        }
                        Err(e) => {
                            let _ = request.respond(cors_json(StatusCode(400), &json_err(&e)));
                        }
                    }
                    continue;
                }

                if path == "/v1/library/info" && method == Method::Get {
                    match library_info(&state) {
                        Ok(info) => {
                            let body = serde_json::to_string(&info).unwrap_or_else(|_| "{}".into());
                            let _ = request.respond(cors_json(StatusCode(200), &body));
                        }
                        Err(e) => {
                            let _ = request.respond(cors_json(StatusCode(500), &json_err(&e)));
                        }
                    }
                    continue;
                }

                if path == "/v1/folders" && method == Method::Get {
                    match state.open_db().and_then(|c| db::list_folders(&c)) {
                        Ok(list) => {
                            let body = serde_json::to_string(&list).unwrap_or_else(|_| "[]".into());
                            let _ = request.respond(cors_json(StatusCode(200), &body));
                        }
                        Err(e) => {
                            let _ = request.respond(cors_json(StatusCode(500), &json_err(&e.to_string())));
                        }
                    }
                    continue;
                }

                if path == "/v1/folders" && method == Method::Post {
                    match read_json::<IngestCreateFolderRequest>(&mut request) {
                        Ok(req) => match create_folder(&state, req.name, req.parent_id) {
                            Ok(folder) => {
                                let body = serde_json::to_string(&folder).unwrap_or_else(|_| "{}".into());
                                let _ = request.respond(cors_json(StatusCode(200), &body));
                            }
                            Err(e) => {
                                let _ = request.respond(cors_json(StatusCode(400), &json_err(&e)));
                            }
                        },
                        Err(e) => {
                            let _ = request.respond(cors_json(StatusCode(400), &json_err(&e)));
                        }
                    }
                    continue;
                }

                if path == "/v1/tags" && method == Method::Get {
                    match state.open_db().and_then(|c| db::list_tags(&c)) {
                        Ok(list) => {
                            let body = serde_json::to_string(&list).unwrap_or_else(|_| "[]".into());
                            let _ = request.respond(cors_json(StatusCode(200), &body));
                        }
                        Err(e) => {
                            let _ = request.respond(cors_json(StatusCode(500), &json_err(&e.to_string())));
                        }
                    }
                    continue;
                }

                if path == "/v1/import/paths" && method == Method::Post {
                    match read_json::<IngestImportPathsRequest>(&mut request) {
                        Ok(req) => {
                            let paths: Vec<String> = req
                                .paths
                                .into_iter()
                                .map(|s| s.trim().to_string())
                                .filter(|s| !s.is_empty())
                                .take(50)
                                .collect();
                            if paths.is_empty() {
                                let _ = request.respond(cors_json(
                                    StatusCode(400),
                                    &json_err("paths required"),
                                ));
                            } else {
                                log::info!(
                                    target: "inspirex",
                                    "http_import_paths count={} folder={:?}",
                                    paths.len(),
                                    req.folder_id
                                );
                                match import_paths(&state, paths, req.folder_id) {
                                    Ok(result) => {
                                        let body = serde_json::to_string(&result)
                                            .unwrap_or_else(|_| "{}".into());
                                        let _ = request.respond(cors_json(StatusCode(200), &body));
                                    }
                                    Err(e) => {
                                        let _ = request.respond(cors_json(StatusCode(500), &json_err(&e)));
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            let _ = request.respond(cors_json(StatusCode(400), &json_err(&e)));
                        }
                    }
                    continue;
                }

                if path == "/v1/import" && method == Method::Post {
                    if !authorize(&request, &state.ingest_token) {
                        let _ = request.respond(cors_json(
                            StatusCode(401),
                            r#"{"ok":false,"error":"unauthorized"}"#,
                        ));
                        continue;
                    }
                    let mut body = String::new();
                    if let Err(e) = request.as_reader().read_to_string(&mut body) {
                        let _ = request.respond(cors_json(
                            StatusCode(400),
                            &format!(r#"{{"ok":false,"error":"read body: {e}"}}"#),
                        ));
                        continue;
                    }
                    let parsed: Result<IngestImportRequest, _> = serde_json::from_str(&body);
                    let req = match parsed {
                        Ok(r) => r,
                        Err(e) => {
                            let _ = request.respond(cors_json(
                                StatusCode(400),
                                &format!(r#"{{"ok":false,"error":"json: {e}"}}"#),
                            ));
                            continue;
                        }
                    };
                    if req.image_url.trim().is_empty() {
                        let _ = request.respond(cors_json(
                            StatusCode(400),
                            r#"{"ok":false,"error":"imageUrl required"}"#,
                        ));
                        continue;
                    }
                    match import_from_url(
                        &state,
                        &req.image_url,
                        req.page_url.as_deref(),
                        req.title.as_deref(),
                        req.notes.as_deref(),
                        req.folder_id,
                    ) {
                        Ok(result) => {
                            let payload = serde_json::json!({
                                "ok": true,
                                "imported": result.imported,
                                "skipped": result.skipped,
                                "errors": result.errors,
                                "runId": result.run_id,
                            });
                            let _ = request.respond(cors_json(
                                StatusCode(200),
                                &payload.to_string(),
                            ));
                        }
                        Err(e) => {
                            log::warn!(target: "inspirex", "ingest_import_fail err={}", e);
                            let _ = request.respond(cors_json(
                                StatusCode(500),
                                &format!(r#"{{"ok":false,"error":{}}}"#, serde_json::to_string(&e).unwrap_or_default()),
                            ));
                        }
                    }
                    continue;
                }

                if path == "/v1/import/batch" && method == Method::Post {
                    if !authorize(&request, &state.ingest_token) {
                        let _ = request.respond(cors_json(
                            StatusCode(401),
                            r#"{"ok":false,"error":"unauthorized"}"#,
                        ));
                        continue;
                    }
                    let mut body = String::new();
                    if let Err(e) = request.as_reader().read_to_string(&mut body) {
                        let _ = request.respond(cors_json(
                            StatusCode(400),
                            &format!(r#"{{"ok":false,"error":"read body: {e}"}}"#),
                        ));
                        continue;
                    }
                    let parsed: Result<IngestImportBatchRequest, _> = serde_json::from_str(&body);
                    let req = match parsed {
                        Ok(r) => r,
                        Err(e) => {
                            let _ = request.respond(cors_json(
                                StatusCode(400),
                                &format!(r#"{{"ok":false,"error":"json: {e}"}}"#),
                            ));
                            continue;
                        }
                    };
                    let urls: Vec<String> = req
                        .image_urls
                        .into_iter()
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                        .take(30)
                        .collect();
                    if urls.is_empty() {
                        let _ = request.respond(cors_json(
                            StatusCode(400),
                            r#"{"ok":false,"error":"imageUrls required"}"#,
                        ));
                        continue;
                    }
                    let batch_id = uuid::Uuid::new_v4().to_string();
                    log::info!(
                        target: "inspirex",
                        "ingest_batch_start run_id={} count={} page={:?}",
                        batch_id,
                        urls.len(),
                        req.page_url
                    );
                    let mut imported = 0usize;
                    let mut skipped = 0usize;
                    let mut errors = Vec::new();
                    for image_url in &urls {
                        match import_from_url(
                            &state,
                            image_url,
                            req.page_url.as_deref(),
                            req.title.as_deref(),
                            req.notes.as_deref(),
                            req.folder_id.clone(),
                        ) {
                            Ok(result) => {
                                imported += result.imported;
                                skipped += result.skipped;
                                errors.extend(result.errors);
                            }
                            Err(e) => errors.push(format!("{image_url}: {e}")),
                        }
                    }
                    log::info!(
                        target: "inspirex",
                        "ingest_batch_done run_id={} imported={} skipped={} errors={}",
                        batch_id,
                        imported,
                        skipped,
                        errors.len()
                    );
                    let payload = serde_json::json!({
                        "ok": true,
                        "imported": imported,
                        "skipped": skipped,
                        "errors": errors,
                        "runId": batch_id,
                    });
                    let _ = request.respond(cors_json(StatusCode(200), &payload.to_string()));
                    continue;
                }

                let _ = request.respond(cors_json(
                    StatusCode(404),
                    r#"{"ok":false,"error":"not found"}"#,
                ));
            }
        })
        .ok();
}

fn authorize(request: &tiny_http::Request, token: &str) -> bool {
    if token.is_empty() {
        return true;
    }
    for h in request.headers() {
        let name = h.field.as_str().as_str();
        if name.eq_ignore_ascii_case("Authorization") {
            let v = h.value.as_str();
            if let Some(rest) = v.strip_prefix("Bearer ") {
                return rest.trim() == token;
            }
        }
    }
    false
}

fn cors_headers() -> Vec<Header> {
    vec![
        Header::from_bytes(&b"Access-Control-Allow-Origin"[..], &b"*"[..]).unwrap(),
        Header::from_bytes(
            &b"Access-Control-Allow-Headers"[..],
            &b"Content-Type, Authorization"[..],
        )
        .unwrap(),
        Header::from_bytes(&b"Access-Control-Allow-Methods"[..], &b"GET, POST, OPTIONS"[..])
            .unwrap(),
        Header::from_bytes(&b"Access-Control-Allow-Private-Network"[..], &b"true"[..]).unwrap(),
        Header::from_bytes(&b"Content-Type"[..], &b"application/json; charset=utf-8"[..]).unwrap(),
    ]
}

fn cors_json(status: StatusCode, body: &str) -> Response<Cursor<Vec<u8>>> {
    let data = body.as_bytes().to_vec();
    let len = data.len();
    Response::new(
        status,
        cors_headers(),
        Cursor::new(data),
        Some(len),
        None,
    )
}

fn cors_empty(status: StatusCode) -> Response<Cursor<Vec<u8>>> {
    let mut headers = cors_headers();
    headers.retain(|h| !h.field.as_str().as_str().eq_ignore_ascii_case("Content-Type"));
    Response::new(status, headers, Cursor::new(Vec::new()), Some(0), None)
}

fn cors_bytes(status: StatusCode, content_type: &str, data: Vec<u8>) -> Response<Cursor<Vec<u8>>> {
    let headers = vec![
        Header::from_bytes(&b"Access-Control-Allow-Origin"[..], &b"*"[..]).unwrap(),
        Header::from_bytes(&b"Access-Control-Allow-Methods"[..], &b"GET, POST, OPTIONS"[..])
            .unwrap(),
        Header::from_bytes(&b"Access-Control-Allow-Private-Network"[..], &b"true"[..]).unwrap(),
        Header::from_bytes(&b"Content-Type"[..], content_type.as_bytes()).unwrap(),
        Header::from_bytes(&b"Cache-Control"[..], &b"private, max-age=60"[..]).unwrap(),
    ];
    let len = data.len();
    Response::new(status, headers, Cursor::new(data), Some(len), None)
}

fn json_err(msg: &str) -> String {
    serde_json::json!({ "ok": false, "error": msg }).to_string()
}

fn read_json<T: serde::de::DeserializeOwned>(request: &mut tiny_http::Request) -> Result<T, String> {
    let mut body = String::new();
    Read::read_to_string(&mut request.as_reader(), &mut body)
        .map_err(|e| format!("read body: {e}"))?;
    serde_json::from_str(&body).map_err(|e| format!("json: {e}"))
}

fn allow_read_path(state: &AppState, path: &Path) -> Result<PathBuf, String> {
    if let Ok(p) = ensure_readable(path) {
        return Ok(p);
    }
    let lib = state
        .library_path
        .canonicalize()
        .unwrap_or_else(|_| state.library_path.clone());
    let path_c = path
        .canonicalize()
        .map_err(|e| format!("路径不可用 {}: {e}", path.display()))?;
    if path_c.starts_with(&lib) {
        return Ok(path_c);
    }
    Err(format!("不允许读取 {}", path.display()))
}

fn query_param(url: &str, key: &str) -> Option<String> {
    let q = url.split_once('?')?.1;
    for pair in q.split('&') {
        if let Some((k, v)) = pair.split_once('=') {
            if k == key {
                return Some(percent_decode(v));
            }
        }
    }
    None
}

fn percent_decode(s: &str) -> String {
    let s = s.replace('+', " ");
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_param_decodes_path() {
        let url = "/v1/browse/list?path=%2FVolumes%2FQ-V-P&limit=72";
        assert_eq!(query_param(url, "path").as_deref(), Some("/Volumes/Q-V-P"));
        assert_eq!(query_param(url, "limit").as_deref(), Some("72"));
    }

    #[test]
    fn percent_decode_utf8_folder() {
        assert_eq!(percent_decode("%E5%A5%BD%E5%86%85%E5%AE%B9%E5%9B%BE"), "好内容图");
    }

    #[test]
    fn batch_request_parses_camel_case() {
        let raw = r#"{"imageUrls":["https://a.example/1.png","https://a.example/2.jpg"],"pageUrl":"https://a.example/","title":"t"}"#;
        let req: crate::models::IngestImportBatchRequest = serde_json::from_str(raw).unwrap();
        assert_eq!(req.image_urls.len(), 2);
        assert_eq!(req.page_url.as_deref(), Some("https://a.example/"));
    }

    #[test]
    fn cors_headers_allow_private_network() {
        let names: Vec<String> = cors_headers()
            .iter()
            .map(|h| h.field.as_str().as_str().to_string())
            .collect();
        assert!(
            names.iter().any(|n| n.eq_ignore_ascii_case("Access-Control-Allow-Private-Network")),
            "missing PNA header, got {names:?}"
        );
    }

    #[test]
    fn create_folder_and_import_paths_parse() {
        let folder: crate::models::IngestCreateFolderRequest =
            serde_json::from_str(r#"{"name":"素材","parentId":null}"#).unwrap();
        assert_eq!(folder.name, "素材");
        let paths: crate::models::IngestImportPathsRequest =
            serde_json::from_str(r#"{"paths":["/Volumes/Q-V-P/a"],"folderId":"f1"}"#).unwrap();
        assert_eq!(paths.paths.len(), 1);
        assert_eq!(paths.folder_id.as_deref(), Some("f1"));
    }

    #[test]
    fn invoke_request_parses_cmd_and_args() {
        let req: crate::models::InvokeRequest = serde_json::from_str(
            r##"{"cmd":"create_tag","args":{"name":"红","color":"#FF5F57"}}"##,
        )
        .unwrap();
        assert_eq!(req.cmd, "create_tag");
        assert_eq!(req.args["name"], "红");
    }
}
