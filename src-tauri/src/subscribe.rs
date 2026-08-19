//! 来源二：RSS/Atom、URL 列表、公开 HTML 页订阅，拉取后走 import_remote_image。

use crate::db;
use crate::library::{import_remote_image, AppState, ImportOpts};
use crate::models::{CreateSource, ImportResult, Source};
use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;
use uuid::Uuid;

const MAX_IMAGES_PER_PULL: usize = 30;

#[derive(Debug, Clone)]
pub struct RemoteImage {
    pub image_url: String,
    pub page_url: Option<String>,
    pub title: Option<String>,
}

pub fn spawn_subscribe_poller(state: Arc<AppState>) {
    std::thread::Builder::new()
        .name("inspirex-subscribe".into())
        .spawn(move || {
            log::info!(
                target: "inspirex",
                "subscribe_poller_start scan_sec=60 default_interval_min={}",
                state.subscribe_poll_min
            );
            loop {
                std::thread::sleep(Duration::from_secs(60));
                match pull_due_sources(&state) {
                    Ok(n) if n > 0 => {
                        log::info!(target: "inspirex", "subscribe_poll pulled_sources={}", n);
                    }
                    Ok(_) => {}
                    Err(e) => {
                        log::warn!(target: "inspirex", "subscribe_poll_fail err={e}");
                    }
                }
            }
        })
        .ok();
}

pub fn create_source(state: &AppState, req: CreateSource) -> Result<Source, String> {
    let kind = req.kind.trim().to_ascii_lowercase();
    if kind != "rss" && kind != "url_list" && kind != "html_page" {
        return Err(format!("不支持的源类型: {kind}（rss | url_list | html_page）"));
    }
    let url = req.url.trim().to_string();
    if url.is_empty() {
        return Err("url 不能为空".into());
    }
    let name = if req.name.trim().is_empty() {
        url.chars().take(48).collect()
    } else {
        req.name.trim().to_string()
    };
    let now = chrono::Utc::now().to_rfc3339();
    let source = Source {
        id: Uuid::new_v4().to_string(),
        name,
        kind,
        url,
        interval_min: req.interval_min.unwrap_or(state.subscribe_poll_min).max(5),
        folder_id: req.folder_id.filter(|s| !s.is_empty()),
        enabled: true,
        last_run_at: None,
        last_error: None,
        created_at: now,
    };
    let conn = state.open_db().map_err(|e| e.to_string())?;
    db::insert_source(&conn, &source).map_err(|e| e.to_string())?;
    log::info!(
        target: "inspirex",
        "source_create id={} kind={} url={}",
        source.id,
        source.kind,
        source.url
    );
    Ok(source)
}

pub fn pull_source(state: &AppState, source_id: &str) -> Result<ImportResult, String> {
    let conn = state.open_db().map_err(|e| e.to_string())?;
    let source = db::get_source(&conn, source_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("订阅不存在: {source_id}"))?;
    pull_source_inner(state, &source)
}

fn pull_due_sources(state: &AppState) -> Result<usize, String> {
    let conn = state.open_db().map_err(|e| e.to_string())?;
    let sources = db::list_sources(&conn).map_err(|e| e.to_string())?;
    let now = chrono::Utc::now();
    let mut n = 0usize;
    for src in sources {
        if !src.enabled {
            continue;
        }
        let due = match &src.last_run_at {
            None => true,
            Some(ts) => chrono::DateTime::parse_from_rfc3339(ts)
                .map(|t| now.signed_duration_since(t.with_timezone(&chrono::Utc)).num_minutes() >= src.interval_min)
                .unwrap_or(true),
        };
        if due {
            let _ = pull_source_inner(state, &src);
            n += 1;
        }
    }
    Ok(n)
}

fn pull_source_inner(state: &AppState, source: &Source) -> Result<ImportResult, String> {
    let run_id = Uuid::new_v4().to_string();
    let now = chrono::Utc::now().to_rfc3339();
    log::info!(
        target: "inspirex",
        "subscribe_pull_start run_id={} source_id={} kind={}",
        run_id,
        source.id,
        source.kind
    );

    let conn = state.open_db().map_err(|e| e.to_string())?;
    db::insert_pipeline_run(&conn, &run_id, "subscribe", "running", &source.id, &now)
        .map_err(|e| e.to_string())?;

    let items = match collect_source_images(source) {
        Ok(v) => v,
        Err(e) => {
            db::update_source_run(&conn, &source.id, &now, Some(&e))
                .map_err(|err| err.to_string())?;
            db::insert_pipeline_run(
                &conn,
                &format!("{run_id}-done"),
                "subscribe",
                "error",
                &format!("source_id={} err={e}", source.id),
                &now,
            )
            .map_err(|err| err.to_string())?;
            log::warn!(
                target: "inspirex",
                "subscribe_pull_fail run_id={} source_id={} err={}",
                run_id,
                source.id,
                e
            );
            return Ok(ImportResult {
                imported: 0,
                skipped: 0,
                errors: vec![e],
                run_id,
            });
        }
    };

    let mut imported = 0usize;
    let mut skipped = 0usize;
    let mut errors = Vec::new();

    for item in items.into_iter().take(MAX_IMAGES_PER_PULL) {
        let notes = item.title.clone();
        match import_remote_image(
            state,
            &item.image_url,
            ImportOpts {
                folder_id: source.folder_id.clone(),
                source_url: item.page_url.or_else(|| Some(item.image_url.clone())),
                notes,
                source_kind: "web".to_string(),
                source_ref: Some(source.id.clone()),
            },
        ) {
            Ok(r) => {
                imported += r.imported;
                skipped += r.skipped;
                errors.extend(r.errors);
            }
            Err(e) => errors.push(format!("{}: {e}", item.image_url)),
        }
    }

    let err_summary = if errors.is_empty() {
        None
    } else {
        Some(errors.join(" | ").chars().take(500).collect::<String>())
    };
    db::update_source_run(&conn, &source.id, &now, err_summary.as_deref())
        .map_err(|e| e.to_string())?;

    let detail = format!(
        "source_id={} imported={imported} skipped={skipped} errors={}",
        source.id,
        errors.len()
    );
    db::insert_pipeline_run(
        &conn,
        &format!("{run_id}-done"),
        "subscribe",
        "ok",
        &detail,
        &now,
    )
    .map_err(|e| e.to_string())?;
    log::info!(target: "inspirex", "subscribe_pull_done run_id={} {}", run_id, detail);

    Ok(ImportResult {
        imported,
        skipped,
        errors,
        run_id,
    })
}

fn collect_source_images(source: &Source) -> Result<Vec<RemoteImage>, String> {
    match source.kind.as_str() {
        "url_list" => Ok(parse_url_list(&source.url)),
        "html_page" => fetch_html_images(&source.url),
        _ => {
            let xml = fetch_text(&source.url)?;
            Ok(parse_feed(&xml))
        }
    }
}

fn fetch_text(url: &str) -> Result<String, String> {
    if let Some(path) = url.strip_prefix("file://") {
        return std::fs::read_to_string(path).map_err(|e| format!("读本地 feed: {e}"));
    }
    if url.starts_with('/') || (url.len() > 2 && url.as_bytes()[1] == b':') {
        return std::fs::read_to_string(url).map_err(|e| format!("读本地 feed: {e}"));
    }
    let resp = ureq::get(url)
        .set("User-Agent", "InspireX/0.1")
        .call()
        .map_err(|e| format!("拉取 feed 失败: {e}"))?;
    resp.into_string()
        .map_err(|e| format!("读取 feed 失败: {e}"))
}

pub fn parse_url_list(text: &str) -> Vec<RemoteImage> {
    text.split(|c: char| c.is_whitespace() || c == ',')
        .map(str::trim)
        .filter(|s| s.starts_with("http://") || s.starts_with("https://"))
        .map(|u| RemoteImage {
            image_url: u.to_string(),
            page_url: Some(u.to_string()),
            title: None,
        })
        .collect()
}

pub fn parse_feed(xml: &str) -> Vec<RemoteImage> {
    let mut out = Vec::new();
    let mut blocks = find_blocks(xml, "item");
    if blocks.is_empty() {
        blocks = find_blocks(xml, "entry");
    }
    for block in blocks {
        if let Some(img) = extract_image(&block) {
            out.push(img);
        }
    }
    out
}

fn find_blocks(xml: &str, tag: &str) -> Vec<String> {
    let lower = xml.to_ascii_lowercase();
    let start_pat = format!("<{tag}");
    let end_pat = format!("</{tag}>");
    let mut out = Vec::new();
    let mut from = 0usize;
    while let Some(rel) = lower[from..].find(&start_pat) {
        let s = from + rel;
        let after = lower[s..].find('>').map(|i| s + i + 1).unwrap_or(s);
        if let Some(rel_end) = lower[after..].find(&end_pat) {
            let e = after + rel_end;
            out.push(xml[after..e].to_string());
            from = e + end_pat.len();
        } else {
            break;
        }
    }
    out
}

fn extract_image(block: &str) -> Option<RemoteImage> {
    let title = tag_text(block, "title");
    let page = tag_text(block, "link").or_else(|| attr_in_tag(block, "link", "href"));
    let enclosure = attr_in_tag(block, "enclosure", "url")
        .or_else(|| attr_in_tag(block, "media:content", "url"))
        .or_else(|| attr_in_tag(block, "content", "url"));
    let from_html = tag_text(block, "description")
        .or_else(|| tag_text(block, "content"))
        .or_else(|| tag_text(block, "summary"))
        .and_then(|html| first_img_src(&html));

    let image_url = enclosure
        .filter(|u| looks_like_image(u) || u.starts_with("http"))
        .or(from_html)
        .or_else(|| page.clone().filter(|u| looks_like_image(u)))?;

    if !image_url.starts_with("http://") && !image_url.starts_with("https://") {
        return None;
    }
    Some(RemoteImage {
        image_url: unescape(&image_url),
        page_url: page.map(|s| unescape(&s)),
        title: title.map(|s| unescape(&s)),
    })
}

fn looks_like_image(url: &str) -> bool {
    let path = url.split('?').next().unwrap_or(url).to_ascii_lowercase();
    [".jpg", ".jpeg", ".png", ".webp", ".gif", ".bmp"]
        .iter()
        .any(|ext| path.ends_with(ext))
}

fn first_img_src(html: &str) -> Option<String> {
    let lower = html.to_ascii_lowercase();
    let idx = lower.find("<img")?;
    let rest = &html[idx..];
    attr_generic(rest, "src")
}

fn tag_text(block: &str, tag: &str) -> Option<String> {
    let lower = block.to_ascii_lowercase();
    let open = format!("<{tag}");
    let close = format!("</{tag}>");
    let s = lower.find(&open)?;
    let gt = lower[s..].find('>')? + s;
    let inner_start = gt + 1;
    let e = lower[inner_start..].find(&close)? + inner_start;
    let raw = block[inner_start..e].trim();
    let stripped = strip_cdata(raw);
    if stripped.is_empty() {
        None
    } else {
        Some(stripped)
    }
}

fn attr_in_tag(block: &str, tag: &str, attr: &str) -> Option<String> {
    let lower = block.to_ascii_lowercase();
    let open = format!("<{tag}");
    let s = lower.find(&open)?;
    let gt = lower[s..].find('>')? + s;
    attr_generic(&block[s..=gt], attr)
}

fn attr_generic(fragment: &str, attr: &str) -> Option<String> {
    let lower = fragment.to_ascii_lowercase();
    let key = format!("{attr}=");
    let s = lower.find(&key)?;
    let after = s + key.len();
    let bytes = fragment.as_bytes();
    if after >= bytes.len() {
        return None;
    }
    let quote = bytes[after];
    if quote == b'"' || quote == b'\'' {
        let q = quote as char;
        let rest = &fragment[after + 1..];
        let end = rest.find(q)?;
        let v = rest[..end].trim();
        if v.is_empty() {
            None
        } else {
            Some(v.to_string())
        }
    } else {
        None
    }
}

fn strip_cdata(s: &str) -> String {
    let t = s.trim();
    if let Some(inner) = t
        .strip_prefix("<![CDATA[")
        .and_then(|x| x.strip_suffix("]]"))
        .or_else(|| t.strip_prefix("<![CDATA[").and_then(|x| x.strip_suffix("]]>")) )
    {
        inner.trim().to_string()
    } else {
        t.to_string()
    }
}

fn unescape(s: &str) -> String {
    s.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
}

fn fetch_html_images(page_url: &str) -> Result<Vec<RemoteImage>, String> {
    if page_url.starts_with("http://") || page_url.starts_with("https://") {
        if !robots_allows(page_url) {
            return Err("robots.txt 禁止抓取该路径".into());
        }
    }
    let html = fetch_text(page_url)?;
    let items = parse_html_images(&html, page_url);
    log::info!(
        target: "inspirex",
        "html_page_parse url={} images={}",
        page_url,
        items.len()
    );
    if items.is_empty() {
        return Err("页面中没有可入库的公开图片".into());
    }
    Ok(items)
}

pub fn parse_html_images(html: &str, base_url: &str) -> Vec<RemoteImage> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    let mut push = |raw: String, title: Option<String>| {
        let Some(url) = resolve_url(base_url, &raw) else {
            return;
        };
        if skip_noise_url(&url) || !seen.insert(url.clone()) {
            return;
        }
        if out.len() >= MAX_IMAGES_PER_PULL {
            return;
        }
        out.push(RemoteImage {
            image_url: url,
            page_url: Some(base_url.to_string()),
            title,
        });
    };

    for meta in find_tags(html, "meta") {
        let prop = attr_generic(&meta, "property")
            .or_else(|| attr_generic(&meta, "name"))
            .unwrap_or_default()
            .to_ascii_lowercase();
        if prop == "og:image" || prop == "og:image:url" || prop == "twitter:image" {
            if let Some(c) = attr_generic(&meta, "content") {
                push(c, Some("og:image".into()));
            }
        }
    }

    for img in find_tags(html, "img") {
        let alt = attr_generic(&img, "alt");
        if let Some(srcset) = attr_generic(&img, "srcset").and_then(|s| parse_srcset(&s)) {
            push(srcset, alt.clone());
        }
        if let Some(src) = attr_generic(&img, "src") {
            push(src, alt);
        }
    }

    out
}

fn find_tags(html: &str, tag: &str) -> Vec<String> {
    let lower = html.to_ascii_lowercase();
    let start_pat = format!("<{tag}");
    let mut out = Vec::new();
    let mut from = 0usize;
    while let Some(rel) = lower[from..].find(&start_pat) {
        let s = from + rel;
        let after_name = s + start_pat.len();
        if after_name < lower.len() {
            let c = lower.as_bytes()[after_name];
            if c.is_ascii_alphanumeric() {
                from = after_name;
                continue;
            }
        }
        if let Some(rel_gt) = lower[s..].find('>') {
            out.push(html[s..s + rel_gt + 1].to_string());
            from = s + rel_gt + 1;
        } else {
            break;
        }
    }
    out
}

fn parse_srcset(srcset: &str) -> Option<String> {
    let mut best: Option<(f64, String)> = None;
    for part in srcset.split(',') {
        let mut it = part.split_whitespace();
        let Some(u) = it.next() else {
            continue;
        };
        let w = it
            .next()
            .and_then(|d| {
                d.trim_end_matches('w')
                    .trim_end_matches('x')
                    .parse::<f64>()
                    .ok()
            })
            .unwrap_or(1.0);
        if best.as_ref().map(|(bw, _)| w >= *bw).unwrap_or(true) {
            best = Some((w, u.to_string()));
        }
    }
    best.map(|(_, u)| u)
}

fn resolve_url(base: &str, href: &str) -> Option<String> {
    let href = unescape(href.trim());
    if href.is_empty()
        || href.starts_with("data:")
        || href.starts_with("javascript:")
        || href.starts_with('#')
    {
        return None;
    }
    if href.starts_with("http://") || href.starts_with("https://") {
        return Some(href);
    }
    if href.starts_with("//") {
        let scheme = if base.starts_with("https") {
            "https:"
        } else {
            "http:"
        };
        return Some(format!("{scheme}{href}"));
    }
    let origin = origin_of(base)?;
    if href.starts_with('/') {
        return Some(format!("{origin}{href}"));
    }
    let dir = base_dir(base);
    Some(format!("{dir}/{href}"))
}

fn origin_of(url: &str) -> Option<String> {
    let rest = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))?;
    let host = rest.split('/').next().unwrap_or(rest);
    if url.starts_with("https://") {
        Some(format!("https://{host}"))
    } else {
        Some(format!("http://{host}"))
    }
}

fn origin_and_path(url: &str) -> Option<(String, String)> {
    let origin = origin_of(url)?;
    let rest = url.get(origin.len()..).unwrap_or("/");
    let path = rest.split('?').next().unwrap_or(rest);
    let path = if path.is_empty() { "/" } else { path };
    Some((origin, path.to_string()))
}

fn base_dir(url: &str) -> String {
    let no_q = url.split('?').next().unwrap_or(url);
    if let Some(i) = no_q.rfind('/') {
        if i >= 8 {
            return no_q[..i].to_string();
        }
    }
    origin_of(url).unwrap_or_else(|| no_q.to_string())
}

fn skip_noise_url(url: &str) -> bool {
    let l = url.split('?').next().unwrap_or(url).to_ascii_lowercase();
    l.contains("favicon")
        || l.ends_with(".ico")
        || l.ends_with(".svg")
        || l.contains("1x1")
        || l.contains("pixel.gif")
        || l.contains("/spacer")
}

fn robots_allows(page_url: &str) -> bool {
    let Some((origin, path)) = origin_and_path(page_url) else {
        return true;
    };
    let robots_url = format!("{origin}/robots.txt");
    match fetch_text(&robots_url) {
        Ok(text) => {
            let allowed = path_allowed_by_robots(&text, &path);
            if !allowed {
                log::info!(
                    target: "inspirex",
                    "robots_disallow origin={} path={}",
                    origin,
                    path
                );
            }
            allowed
        }
        Err(e) => {
            log::warn!(
                target: "inspirex",
                "robots_check_skip url={} err={}",
                robots_url,
                e
            );
            true
        }
    }
}

pub fn path_allowed_by_robots(robots: &str, path: &str) -> bool {
    let mut in_star = false;
    let mut rules: Vec<(bool, String)> = Vec::new();
    for raw in robots.lines() {
        let line = raw.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        let Some((k, v)) = line.split_once(':') else {
            continue;
        };
        let k = k.trim().to_ascii_lowercase();
        let v = v.trim();
        if k == "user-agent" {
            in_star = v == "*" || v.eq_ignore_ascii_case("inspirex");
        } else if in_star && k == "disallow" {
            rules.push((false, v.to_string()));
        } else if in_star && k == "allow" {
            rules.push((true, v.to_string()));
        }
    }
    let mut best_len = -1i32;
    let mut allowed = true;
    for (allow, prefix) in rules {
        if prefix.is_empty() {
            continue;
        }
        if path.starts_with(&prefix) {
            let len = prefix.len() as i32;
            if len > best_len || (len == best_len && allow) {
                best_len = len;
                allowed = allow;
            }
        }
    }
    allowed
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::AppState;
    use image::{Rgb, RgbImage};
    use std::io::Cursor;
    use std::sync::atomic::{AtomicU16, Ordering};
    use tempfile::tempdir;
    use tiny_http::{Header, Response, Server, StatusCode};

    static PORT: AtomicU16 = AtomicU16::new(19880);

    #[test]
    fn parse_rss_enclosure() {
        let xml = r#"<?xml version="1.0"?>
        <rss><channel>
          <item>
            <title>Red square</title>
            <link>https://example.com/post</link>
            <enclosure url="https://cdn.example.com/a.png" type="image/png"/>
          </item>
        </channel></rss>"#;
        let items = parse_feed(xml);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].image_url, "https://cdn.example.com/a.png");
        assert_eq!(items[0].page_url.as_deref(), Some("https://example.com/post"));
    }

    #[test]
    fn parse_rss_img_in_description() {
        let xml = r#"<rss><channel><item>
            <title>x</title>
            <description><![CDATA[<img src="https://img.example.com/b.jpg">]]></description>
        </item></channel></rss>"#;
        let items = parse_feed(xml);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].image_url, "https://img.example.com/b.jpg");
    }

    #[test]
    fn rss_pull_then_skip_duplicate() {
        let dir = tempdir().unwrap();
        let lib = dir.path().join("Library");
        let state = AppState::for_test(lib);
        state.ensure_dirs().unwrap();

        let mut img = RgbImage::new(16, 16);
        for p in img.pixels_mut() {
            *p = Rgb([30, 140, 80]);
        }
        let png_path = dir.path().join("sample.png");
        img.save(&png_path).unwrap();
        let png_bytes = std::fs::read(&png_path).unwrap();

        let port = PORT.fetch_add(1, Ordering::SeqCst);
        let addr = format!("127.0.0.1:{port}");
        let server = Server::http(&addr).expect("bind fixture http");
        let feed = format!(
            r#"<?xml version="1.0"?><rss><channel>
            <item><title>fixture</title>
            <link>http://{addr}/post</link>
            <enclosure url="http://{addr}/sample.png" type="image/png"/>
            </item></channel></rss>"#
        );
        let feed_bytes = feed.into_bytes();
        let png_clone = png_bytes.clone();
        std::thread::spawn(move || {
            for request in server.incoming_requests() {
                let path = request.url().split('?').next().unwrap_or("/").to_string();
                if path.contains("sample.png") {
                    let cursor = Cursor::new(png_clone.clone());
                    let mut resp = Response::new(StatusCode(200), vec![], cursor, Some(png_clone.len()), None);
                    let _ = resp.add_header(
                        Header::from_bytes(&b"Content-Type"[..], &b"image/png"[..]).unwrap(),
                    );
                    let _ = request.respond(resp);
                } else {
                    let cursor = Cursor::new(feed_bytes.clone());
                    let mut resp = Response::new(
                        StatusCode(200),
                        vec![],
                        cursor,
                        Some(feed_bytes.len()),
                        None,
                    );
                    let _ = resp.add_header(
                        Header::from_bytes(&b"Content-Type"[..], &b"application/rss+xml"[..]).unwrap(),
                    );
                    let _ = request.respond(resp);
                }
            }
        });
        std::thread::sleep(Duration::from_millis(80));

        let src = create_source(
            &state,
            CreateSource {
                name: "fixture".into(),
                kind: "rss".into(),
                url: format!("http://{addr}/feed.xml"),
                interval_min: Some(60),
                folder_id: None,
            },
        )
        .unwrap();
        let first = pull_source(&state, &src.id).unwrap();
        assert_eq!(first.imported, 1, "{first:?}");
        let second = pull_source(&state, &src.id).unwrap();
        assert_eq!(second.imported, 0, "{second:?}");
        assert!(second.skipped >= 1);

        let conn = state.open_db().unwrap();
        let assets = db::list_assets(&conn, None, 10).unwrap();
        assert_eq!(assets[0].source_kind, "web");
        assert_eq!(assets[0].source_ref.as_deref(), Some(src.id.as_str()));
        assert!(assets[0].source_url.as_deref().unwrap().contains("/post"));
    }

    #[test]
    fn parse_html_images_og_and_img() {
        let html = r#"<html><head>
          <meta property="og:image" content="/hero.png">
          <link rel="icon" href="/favicon.ico">
        </head><body>
          <img src="data:image/png;base64,aaa">
          <img src="a.jpg" alt="one">
          <img src="icon.svg">
          <img srcset="b-400.jpg 400w, b-800.jpg 800w" alt="two">
          <img src="//cdn.example.com/c.webp">
        </body></html>"#;
        let items = parse_html_images(html, "https://example.com/gallery/page");
        let urls: Vec<_> = items.iter().map(|i| i.image_url.as_str()).collect();
        assert!(urls.contains(&"https://example.com/hero.png"), "{urls:?}");
        assert!(urls.contains(&"https://example.com/gallery/a.jpg"), "{urls:?}");
        assert!(urls.contains(&"https://example.com/gallery/b-800.jpg"), "{urls:?}");
        assert!(urls.contains(&"https://cdn.example.com/c.webp"), "{urls:?}");
        assert!(!urls.iter().any(|u| u.contains("favicon") || u.contains(".svg") || u.contains("data:")));
    }

    #[test]
    fn robots_longest_match() {
        let robots = "User-agent: *\nDisallow: /\nAllow: /public\n";
        assert!(!path_allowed_by_robots(robots, "/secret"));
        assert!(path_allowed_by_robots(robots, "/public/img.png"));
        assert!(path_allowed_by_robots("User-agent: *\nDisallow:\n", "/any"));
    }

    #[test]
    fn html_page_pull_then_skip_duplicate() {
        let dir = tempdir().unwrap();
        let lib = dir.path().join("Library");
        let state = AppState::for_test(lib);
        state.ensure_dirs().unwrap();

        let mut img = RgbImage::new(16, 16);
        for p in img.pixels_mut() {
            *p = Rgb([200, 80, 40]);
        }
        let png_path = dir.path().join("photo.png");
        img.save(&png_path).unwrap();
        let png_bytes = std::fs::read(&png_path).unwrap();

        let port = PORT.fetch_add(1, Ordering::SeqCst);
        let addr = format!("127.0.0.1:{port}");
        let server = Server::http(&addr).expect("bind html fixture");
        let page = format!(
            r#"<html><body>
            <img src="http://{addr}/photo.png" alt="fixture">
            <img src="http://{addr}/photo.png">
            </body></html>"#
        );
        let robots = "User-agent: *\nAllow: /\n".to_string();
        let page_bytes = page.into_bytes();
        let robots_bytes = robots.into_bytes();
        let png_clone = png_bytes.clone();
        std::thread::spawn(move || {
            for request in server.incoming_requests() {
                let path = request.url().split('?').next().unwrap_or("/").to_string();
                if path.contains("photo.png") {
                    let cursor = Cursor::new(png_clone.clone());
                    let mut resp =
                        Response::new(StatusCode(200), vec![], cursor, Some(png_clone.len()), None);
                    let _ = resp.add_header(
                        Header::from_bytes(&b"Content-Type"[..], &b"image/png"[..]).unwrap(),
                    );
                    let _ = request.respond(resp);
                } else if path.contains("robots.txt") {
                    let cursor = Cursor::new(robots_bytes.clone());
                    let resp = Response::new(
                        StatusCode(200),
                        vec![],
                        cursor,
                        Some(robots_bytes.len()),
                        None,
                    );
                    let _ = request.respond(resp);
                } else {
                    let cursor = Cursor::new(page_bytes.clone());
                    let mut resp = Response::new(
                        StatusCode(200),
                        vec![],
                        cursor,
                        Some(page_bytes.len()),
                        None,
                    );
                    let _ = resp.add_header(
                        Header::from_bytes(&b"Content-Type"[..], &b"text/html"[..]).unwrap(),
                    );
                    let _ = request.respond(resp);
                }
            }
        });
        std::thread::sleep(Duration::from_millis(80));

        let src = create_source(
            &state,
            CreateSource {
                name: "html fixture".into(),
                kind: "html_page".into(),
                url: format!("http://{addr}/gallery"),
                interval_min: Some(60),
                folder_id: None,
            },
        )
        .unwrap();
        let first = pull_source(&state, &src.id).unwrap();
        assert_eq!(first.imported, 1, "{first:?}");
        let second = pull_source(&state, &src.id).unwrap();
        assert_eq!(second.imported, 0, "{second:?}");
        assert!(second.skipped >= 1);

        let conn = state.open_db().unwrap();
        let assets = db::list_assets(&conn, None, 10).unwrap();
        assert_eq!(assets[0].source_kind, "web");
        assert_eq!(assets[0].source_ref.as_deref(), Some(src.id.as_str()));
        assert!(assets[0].source_url.as_deref().unwrap().contains("/gallery"));
    }

    #[test]
    fn html_page_respects_robots_disallow() {
        let dir = tempdir().unwrap();
        let lib = dir.path().join("Library");
        let state = AppState::for_test(lib);
        state.ensure_dirs().unwrap();

        let port = PORT.fetch_add(1, Ordering::SeqCst);
        let addr = format!("127.0.0.1:{port}");
        let server = Server::http(&addr).expect("bind robots fixture");
        let robots = b"User-agent: *\nDisallow: /blocked\n".to_vec();
        let page = b"<html><img src='/x.png'></html>".to_vec();
        std::thread::spawn(move || {
            for request in server.incoming_requests() {
                let path = request.url().split('?').next().unwrap_or("/").to_string();
                if path.contains("robots.txt") {
                    let cursor = Cursor::new(robots.clone());
                    let _ = request.respond(Response::new(
                        StatusCode(200),
                        vec![],
                        cursor,
                        Some(robots.len()),
                        None,
                    ));
                } else {
                    let cursor = Cursor::new(page.clone());
                    let _ = request.respond(Response::new(
                        StatusCode(200),
                        vec![],
                        cursor,
                        Some(page.len()),
                        None,
                    ));
                }
            }
        });
        std::thread::sleep(Duration::from_millis(80));

        let src = create_source(
            &state,
            CreateSource {
                name: "blocked".into(),
                kind: "html_page".into(),
                url: format!("http://{addr}/blocked/page"),
                interval_min: Some(60),
                folder_id: None,
            },
        )
        .unwrap();
        let result = pull_source(&state, &src.id).unwrap();
        assert_eq!(result.imported, 0, "{result:?}");
        let conn = state.open_db().unwrap();
        let sources = db::list_sources(&conn).unwrap();
        let got = sources.iter().find(|s| s.id == src.id).unwrap();
        assert!(
            got.last_error
                .as_deref()
                .unwrap_or("")
                .contains("robots.txt"),
            "{got:?}"
        );
    }
}
