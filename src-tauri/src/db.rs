use crate::models::{Asset, Folder, GenJob, Source, Tag};
use rusqlite::{params, Connection, OptionalExtension};
use std::path::Path;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum DbError {
    #[error("sqlite: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("{0}")]
    Msg(String),
}

pub type DbResult<T> = Result<T, DbError>;

pub fn open_db(db_path: &Path) -> DbResult<Connection> {
    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| DbError::Msg(e.to_string()))?;
    }
    let conn = Connection::open(db_path)?;
    conn.execute_batch("PRAGMA foreign_keys = ON; PRAGMA journal_mode = WAL;")?;
    migrate(&conn)?;
    Ok(conn)
}

fn migrate(conn: &Connection) -> DbResult<()> {
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS folders (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            parent_id TEXT,
            created_at TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS tags (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL UNIQUE,
            color TEXT,
            created_at TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS assets (
            id TEXT PRIMARY KEY,
            filename TEXT NOT NULL,
            original_path TEXT,
            stored_path TEXT NOT NULL UNIQUE,
            thumb_path TEXT,
            mime TEXT,
            width INTEGER,
            height INTEGER,
            file_size INTEGER,
            source_url TEXT,
            notes TEXT,
            folder_id TEXT,
            dominant_color TEXT,
            colors_json TEXT,
            content_hash TEXT,
            clip_status TEXT NOT NULL DEFAULT 'pending',
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS asset_tags (
            asset_id TEXT NOT NULL,
            tag_id TEXT NOT NULL,
            PRIMARY KEY (asset_id, tag_id)
        );

        CREATE TABLE IF NOT EXISTS embeddings (
            asset_id TEXT PRIMARY KEY,
            dim INTEGER NOT NULL,
            vector_json TEXT NOT NULL,
            model TEXT NOT NULL,
            created_at TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS pipeline_runs (
            run_id TEXT PRIMARY KEY,
            kind TEXT NOT NULL,
            status TEXT NOT NULL,
            detail TEXT,
            created_at TEXT NOT NULL
        );

        CREATE VIRTUAL TABLE IF NOT EXISTS assets_fts USING fts5(
            asset_id UNINDEXED,
            filename,
            notes,
            source_url,
            tags
        );

        CREATE TABLE IF NOT EXISTS sources (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            kind TEXT NOT NULL,
            url TEXT NOT NULL,
            interval_min INTEGER NOT NULL DEFAULT 360,
            folder_id TEXT,
            enabled INTEGER NOT NULL DEFAULT 1,
            last_run_at TEXT,
            last_error TEXT,
            created_at TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS gen_jobs (
            id TEXT PRIMARY KEY,
            prompt TEXT NOT NULL,
            n INTEGER NOT NULL,
            size TEXT NOT NULL,
            provider TEXT NOT NULL,
            model TEXT NOT NULL,
            status TEXT NOT NULL,
            folder_id TEXT,
            error TEXT,
            run_id TEXT,
            imported INTEGER NOT NULL DEFAULT 0,
            created_at TEXT NOT NULL,
            finished_at TEXT,
            output_paths TEXT NOT NULL DEFAULT '[]'
        );

        CREATE TABLE IF NOT EXISTS folder_items (
            folder_id TEXT NOT NULL,
            kind TEXT NOT NULL,
            item_ref TEXT NOT NULL,
            added_at TEXT NOT NULL,
            PRIMARY KEY (folder_id, kind, item_ref)
        );
        "#,
    )?;
    add_column_if_missing(conn, "assets", "source_kind", "TEXT NOT NULL DEFAULT 'local'")?;
    add_column_if_missing(conn, "assets", "source_ref", "TEXT")?;
    add_column_if_missing(conn, "gen_jobs", "output_paths", "TEXT NOT NULL DEFAULT '[]'")?;
    Ok(())
}

fn add_column_if_missing(conn: &Connection, table: &str, col: &str, decl: &str) -> DbResult<()> {
    let sql = format!("SELECT COUNT(*) FROM pragma_table_info('{table}') WHERE name = '{col}'");
    let n: i64 = conn.query_row(&sql, [], |r| r.get(0))?;
    if n == 0 {
        conn.execute_batch(&format!("ALTER TABLE {table} ADD COLUMN {col} {decl}"))?;
        log::info!(target: "inspirex", "migrate_add_column table={} col={}", table, col);
    }
    Ok(())
}

pub fn asset_count(conn: &Connection) -> DbResult<i64> {
    let n: i64 = conn.query_row("SELECT COUNT(*) FROM assets", [], |r| r.get(0))?;
    Ok(n)
}

pub fn insert_folder(conn: &Connection, folder: &Folder) -> DbResult<()> {
    conn.execute(
        "INSERT INTO folders (id, name, parent_id, created_at) VALUES (?1, ?2, ?3, ?4)",
        params![folder.id, folder.name, folder.parent_id, folder.created_at],
    )?;
    Ok(())
}

pub fn list_folders(conn: &Connection) -> DbResult<Vec<Folder>> {
    let mut stmt = conn.prepare(
        r#"SELECT id, name, parent_id, created_at,
              (SELECT COUNT(*) FROM folder_items WHERE folder_id = folders.id) AS item_count
           FROM folders ORDER BY name COLLATE NOCASE"#,
    )?;
    let rows = stmt.query_map([], |r| {
        Ok(Folder {
            id: r.get(0)?,
            name: r.get(1)?,
            parent_id: r.get(2)?,
            created_at: r.get(3)?,
            item_count: r.get(4)?,
        })
    })?;
    Ok(rows.filter_map(|r| r.ok()).collect())
}

pub fn get_folder(conn: &Connection, folder_id: &str) -> DbResult<Option<Folder>> {
    conn.query_row(
        r#"SELECT id, name, parent_id, created_at,
              (SELECT COUNT(*) FROM folder_items WHERE folder_id = ?1) AS item_count
           FROM folders WHERE id = ?1"#,
        params![folder_id],
        |r| {
            Ok(Folder {
                id: r.get(0)?,
                name: r.get(1)?,
                parent_id: r.get(2)?,
                created_at: r.get(3)?,
                item_count: r.get(4)?,
            })
        },
    )
    .optional()
    .map_err(Into::into)
}

#[derive(Debug, Clone)]
pub struct FolderItemRow {
    pub kind: String,
    pub item_ref: String,
    #[allow(dead_code)]
    pub added_at: String,
}

pub fn list_folder_items(conn: &Connection, folder_id: &str) -> DbResult<Vec<FolderItemRow>> {
    let mut stmt = conn.prepare(
        "SELECT kind, item_ref, added_at FROM folder_items WHERE folder_id = ?1 ORDER BY added_at DESC",
    )?;
    let rows = stmt.query_map(params![folder_id], |r| {
        Ok(FolderItemRow {
            kind: r.get(0)?,
            item_ref: r.get(1)?,
            added_at: r.get(2)?,
        })
    })?;
    Ok(rows.filter_map(|r| r.ok()).collect())
}

/// 虚拟存入：只写索引，不复制文件。kind = asset | path
pub fn add_folder_items(
    conn: &Connection,
    folder_id: &str,
    items: &[(String, String)],
) -> DbResult<usize> {
    let now = chrono::Utc::now().to_rfc3339();
    let mut n = 0usize;
    for (kind, item_ref) in items {
        n += conn.execute(
            "INSERT OR IGNORE INTO folder_items (folder_id, kind, item_ref, added_at) VALUES (?1, ?2, ?3, ?4)",
            params![folder_id, kind, item_ref, now],
        )?;
    }
    Ok(n)
}

pub fn remove_folder_items(
    conn: &Connection,
    folder_id: &str,
    items: &[(String, String)],
) -> DbResult<usize> {
    let mut n = 0usize;
    for (kind, item_ref) in items {
        n += conn.execute(
            "DELETE FROM folder_items WHERE folder_id = ?1 AND kind = ?2 AND item_ref = ?3",
            params![folder_id, kind, item_ref],
        )?;
    }
    Ok(n)
}

/// 收集文件夹自身及全部子文件夹 id（广度优先）。
fn collect_folder_subtree(conn: &Connection, root_id: &str) -> DbResult<Vec<String>> {
    let mut ids = vec![root_id.to_string()];
    let mut i = 0;
    while i < ids.len() {
        let parent = ids[i].clone();
        let mut stmt = conn.prepare("SELECT id FROM folders WHERE parent_id = ?1")?;
        let children = stmt.query_map(params![parent], |r| r.get::<_, String>(0))?;
        for child in children {
            ids.push(child?);
        }
        i += 1;
    }
    Ok(ids)
}

/// 删除文件夹：解除素材上的 folder_id 索引，并删掉文件夹记录（含子孙）。
/// 不删除素材文件 / embeddings / FTS。
pub fn delete_folder(conn: &Connection, folder_id: &str) -> DbResult<(usize, usize)> {
    let exists: bool = conn
        .query_row(
            "SELECT 1 FROM folders WHERE id = ?1",
            params![folder_id],
            |_| Ok(true),
        )
        .optional()?
        .unwrap_or(false);
    if !exists {
        return Err(DbError::Msg(format!("文件夹不存在: {folder_id}")));
    }

    let ids = collect_folder_subtree(conn, folder_id)?;
    let now = chrono::Utc::now().to_rfc3339();
    let tx = conn.unchecked_transaction()?;
    let mut unlinked = 0usize;
    let mut deleted = 0usize;

    for id in &ids {
        tx.execute(
            "DELETE FROM folder_items WHERE folder_id = ?1",
            params![id],
        )?;
        unlinked += tx.execute(
            "UPDATE assets SET folder_id = NULL, updated_at = ?1 WHERE folder_id = ?2",
            params![now, id],
        )?;
    }
    // 先删子孙，再删根
    for id in ids.iter().rev() {
        deleted += tx.execute("DELETE FROM folders WHERE id = ?1", params![id])?;
    }
    tx.commit()?;

    log::info!(
        target: "inspirex",
        "delete_folder root={} subtree={} unlinked_assets={} deleted_folders={}",
        folder_id,
        ids.len(),
        unlinked,
        deleted
    );
    Ok((unlinked, deleted))
}

pub fn insert_tag(conn: &Connection, tag: &Tag) -> DbResult<()> {
    conn.execute(
        "INSERT INTO tags (id, name, color, created_at) VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(name) DO NOTHING",
        params![tag.id, tag.name, tag.color, tag.created_at],
    )?;
    Ok(())
}

pub fn list_tags(conn: &Connection) -> DbResult<Vec<Tag>> {
    let mut stmt =
        conn.prepare("SELECT id, name, color, created_at FROM tags ORDER BY name COLLATE NOCASE")?;
    let rows = stmt.query_map([], |r| {
        Ok(Tag {
            id: r.get(0)?,
            name: r.get(1)?,
            color: r.get(2)?,
            created_at: r.get(3)?,
        })
    })?;
    Ok(rows.filter_map(|r| r.ok()).collect())
}

pub fn find_tag_by_name(conn: &Connection, name: &str) -> DbResult<Option<Tag>> {
    conn.query_row(
        "SELECT id, name, color, created_at FROM tags WHERE name = ?1",
        params![name],
        |r| {
            Ok(Tag {
                id: r.get(0)?,
                name: r.get(1)?,
                color: r.get(2)?,
                created_at: r.get(3)?,
            })
        },
    )
    .optional()
    .map_err(Into::into)
}

pub fn content_hash_exists(conn: &Connection, hash: &str) -> DbResult<bool> {
    let n: i64 = conn.query_row(
        "SELECT COUNT(*) FROM assets WHERE content_hash = ?1",
        params![hash],
        |r| r.get(0),
    )?;
    Ok(n > 0)
}

pub fn insert_asset(
    conn: &Connection,
    id: &str,
    filename: &str,
    original_path: Option<&str>,
    stored_path: &str,
    mime: Option<&str>,
    file_size: Option<i64>,
    content_hash: &str,
    folder_id: Option<&str>,
    source_url: Option<&str>,
    notes: Option<&str>,
    source_kind: &str,
    source_ref: Option<&str>,
    now: &str,
) -> DbResult<()> {
    conn.execute(
        r#"INSERT INTO assets (
            id, filename, original_path, stored_path, mime, file_size, content_hash,
            folder_id, source_url, notes, source_kind, source_ref,
            clip_status, created_at, updated_at
        ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,'pending',?13,?13)"#,
        params![
            id,
            filename,
            original_path,
            stored_path,
            mime,
            file_size,
            content_hash,
            folder_id,
            source_url,
            notes,
            source_kind,
            source_ref,
            now
        ],
    )?;
    conn.execute(
        "INSERT INTO assets_fts (asset_id, filename, notes, source_url, tags) VALUES (?1,?2,?3,?4,'')",
        params![
            id,
            filename,
            notes.unwrap_or(""),
            source_url.unwrap_or("")
        ],
    )?;
    if let Some(fid) = folder_id {
        add_folder_items(conn, fid, &[("asset".to_string(), id.to_string())])?;
    }
    Ok(())
}

pub fn update_asset_pipeline(
    conn: &Connection,
    id: &str,
    thumb_path: Option<&str>,
    width: Option<i64>,
    height: Option<i64>,
    dominant_color: Option<&str>,
    colors_json: Option<&str>,
    clip_status: &str,
    now: &str,
) -> DbResult<()> {
    conn.execute(
        r#"UPDATE assets SET
            thumb_path = COALESCE(?2, thumb_path),
            width = COALESCE(?3, width),
            height = COALESCE(?4, height),
            dominant_color = COALESCE(?5, dominant_color),
            colors_json = COALESCE(?6, colors_json),
            clip_status = ?7,
            updated_at = ?8
         WHERE id = ?1"#,
        params![
            id,
            thumb_path,
            width,
            height,
            dominant_color,
            colors_json,
            clip_status,
            now
        ],
    )?;
    Ok(())
}

pub fn upsert_embedding(
    conn: &Connection,
    asset_id: &str,
    dim: usize,
    vector_json: &str,
    model: &str,
    now: &str,
) -> DbResult<()> {
    conn.execute(
        r#"INSERT INTO embeddings (asset_id, dim, vector_json, model, created_at)
           VALUES (?1, ?2, ?3, ?4, ?5)
           ON CONFLICT(asset_id) DO UPDATE SET
             dim=excluded.dim, vector_json=excluded.vector_json,
             model=excluded.model, created_at=excluded.created_at"#,
        params![asset_id, dim as i64, vector_json, model, now],
    )?;
    Ok(())
}

pub fn set_asset_tags(conn: &Connection, asset_id: &str, tag_names: &[String]) -> DbResult<()> {
    conn.execute(
        "DELETE FROM asset_tags WHERE asset_id = ?1",
        params![asset_id],
    )?;
    for name in tag_names {
        let tag = if let Some(t) = find_tag_by_name(conn, name)? {
            t
        } else {
            let t = Tag {
                id: uuid::Uuid::new_v4().to_string(),
                name: name.clone(),
                color: None,
                created_at: chrono::Utc::now().to_rfc3339(),
            };
            insert_tag(conn, &t)?;
            t
        };
        conn.execute(
            "INSERT OR IGNORE INTO asset_tags (asset_id, tag_id) VALUES (?1, ?2)",
            params![asset_id, tag.id],
        )?;
    }
    let tags_joined = tag_names.join(" ");
    let (filename, notes, source_url): (String, Option<String>, Option<String>) = conn.query_row(
        "SELECT filename, notes, source_url FROM assets WHERE id = ?1",
        params![asset_id],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )?;
    conn.execute(
        "DELETE FROM assets_fts WHERE asset_id = ?1",
        params![asset_id],
    )?;
    conn.execute(
        "INSERT INTO assets_fts (asset_id, filename, notes, source_url, tags) VALUES (?1,?2,?3,?4,?5)",
        params![
            asset_id,
            filename,
            notes.unwrap_or_default(),
            source_url.unwrap_or_default(),
            tags_joined
        ],
    )?;
    Ok(())
}

fn load_tags_for(conn: &Connection, asset_id: &str) -> DbResult<Vec<String>> {
    let mut stmt = conn.prepare(
        "SELECT t.name FROM tags t
         JOIN asset_tags at ON at.tag_id = t.id
         WHERE at.asset_id = ?1 ORDER BY t.name",
    )?;
    let rows = stmt.query_map(params![asset_id], |r| r.get::<_, String>(0))?;
    Ok(rows.filter_map(|r| r.ok()).collect())
}

fn map_asset(conn: &Connection, row: &rusqlite::Row<'_>) -> rusqlite::Result<Asset> {
    let id: String = row.get(0)?;
    let tags = load_tags_for(conn, &id).unwrap_or_default();
    Ok(Asset {
        id,
        filename: row.get(1)?,
        stored_path: row.get(2)?,
        thumb_path: row.get(3)?,
        mime: row.get(4)?,
        width: row.get(5)?,
        height: row.get(6)?,
        file_size: row.get(7)?,
        source_url: row.get(8)?,
        notes: row.get(9)?,
        folder_id: row.get(10)?,
        dominant_color: row.get(11)?,
        clip_status: row.get(12)?,
        created_at: row.get(13)?,
        tags,
        source_kind: row
            .get::<_, Option<String>>(14)?
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "local".to_string()),
        source_ref: row.get(15)?,
    })
}

const ASSET_SELECT: &str = r#"
    SELECT id, filename, stored_path, thumb_path, mime, width, height, file_size,
           source_url, notes, folder_id, dominant_color, clip_status, created_at,
           source_kind, source_ref
    FROM assets
"#;

pub fn list_assets(
    conn: &Connection,
    folder_id: Option<&str>,
    limit: i64,
) -> DbResult<Vec<Asset>> {
    list_assets_ex(conn, folder_id, None, None, limit)
}

pub fn list_assets_ex(
    conn: &Connection,
    folder_id: Option<&str>,
    source_kind: Option<&str>,
    source_ref: Option<&str>,
    limit: i64,
) -> DbResult<Vec<Asset>> {
    let sql = format!(
        "{ASSET_SELECT}
         WHERE (?1 = '' OR folder_id = ?1)
           AND (?2 = '' OR source_kind = ?2)
           AND (?3 = '' OR source_ref = ?3)
         ORDER BY created_at DESC LIMIT ?4"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(
        params![
            folder_id.unwrap_or(""),
            source_kind.unwrap_or(""),
            source_ref.unwrap_or(""),
            limit
        ],
        |r| map_asset(conn, r),
    )?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}

pub fn fts_search(conn: &Connection, query: &str, limit: i64) -> DbResult<Vec<Asset>> {
    let mut stmt = conn.prepare(&format!(
        r#"{ASSET_SELECT}
           WHERE id IN (
             SELECT asset_id FROM assets_fts WHERE assets_fts MATCH ?1 LIMIT ?2
           )
           ORDER BY created_at DESC"#
    ))?;
    // escape: use simple token
    let q = query
        .split_whitespace()
        .map(|t| format!("\"{}\"", t.replace('"', "")))
        .collect::<Vec<_>>()
        .join(" ");
    let rows = stmt.query_map(params![q, limit], |r| map_asset(conn, r))?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}

pub fn list_embeddings(conn: &Connection) -> DbResult<Vec<(String, Vec<f32>)>> {
    let mut stmt = conn.prepare("SELECT asset_id, vector_json FROM embeddings")?;
    let rows = stmt.query_map([], |r| {
        let id: String = r.get(0)?;
        let json: String = r.get(1)?;
        Ok((id, json))
    })?;
    let mut out = Vec::new();
    for row in rows {
        let (id, json) = row?;
        let vec: Vec<f32> = serde_json::from_str(&json).unwrap_or_default();
        out.push((id, vec));
    }
    Ok(out)
}

pub fn get_assets_by_ids(conn: &Connection, ids: &[String]) -> DbResult<Vec<Asset>> {
    let mut out = Vec::new();
    for id in ids {
        let asset = conn
            .query_row(
                &format!("{ASSET_SELECT} WHERE id = ?1"),
                params![id],
                |r| map_asset(conn, r),
            )
            .optional()?;
        if let Some(a) = asset {
            out.push(a);
        }
    }
    Ok(out)
}

pub fn get_asset(conn: &Connection, id: &str) -> DbResult<Option<Asset>> {
    conn.query_row(
        &format!("{ASSET_SELECT} WHERE id = ?1"),
        params![id],
        |r| map_asset(conn, r),
    )
    .optional()
    .map_err(DbError::from)
}

/// 返回 (stored_path, thumb_path) 便于删文件。
pub fn delete_asset(conn: &Connection, asset_id: &str) -> DbResult<Option<(String, Option<String>)>> {
    let paths: Option<(String, Option<String>)> = conn
        .query_row(
            "SELECT stored_path, thumb_path FROM assets WHERE id = ?1",
            params![asset_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    let Some(paths) = paths else {
        return Ok(None);
    };

    let tx = conn.unchecked_transaction()?;
    tx.execute(
        "DELETE FROM folder_items WHERE kind = 'asset' AND item_ref = ?1",
        params![asset_id],
    )?;
    tx.execute(
        "DELETE FROM asset_tags WHERE asset_id = ?1",
        params![asset_id],
    )?;
    tx.execute(
        "DELETE FROM embeddings WHERE asset_id = ?1",
        params![asset_id],
    )?;
    tx.execute(
        "DELETE FROM assets_fts WHERE asset_id = ?1",
        params![asset_id],
    )?;
    tx.execute("DELETE FROM assets WHERE id = ?1", params![asset_id])?;
    tx.commit()?;
    log::info!(target: "inspirex", "delete_asset id={} stored={}", asset_id, paths.0);
    Ok(Some(paths))
}

pub fn update_asset_meta(
    conn: &Connection,
    asset_id: &str,
    folder_id: Option<Option<String>>,
    source_url: Option<String>,
    notes: Option<String>,
) -> DbResult<()> {
    let now = chrono::Utc::now().to_rfc3339();
    if let Some(fid) = folder_id {
        conn.execute(
            "UPDATE assets SET folder_id = ?2, updated_at = ?3 WHERE id = ?1",
            params![asset_id, fid, now],
        )?;
        if let Some(ref folder) = fid {
            add_folder_items(conn, folder, &[("asset".to_string(), asset_id.to_string())])?;
        }
    }
    if let Some(url) = source_url {
        conn.execute(
            "UPDATE assets SET source_url = ?2, updated_at = ?3 WHERE id = ?1",
            params![asset_id, url, now],
        )?;
    }
    if let Some(n) = notes {
        conn.execute(
            "UPDATE assets SET notes = ?2, updated_at = ?3 WHERE id = ?1",
            params![asset_id, n, now],
        )?;
    }
    refresh_fts(conn, asset_id)?;
    Ok(())
}

fn refresh_fts(conn: &Connection, asset_id: &str) -> DbResult<()> {
    let (filename, notes, source_url): (String, Option<String>, Option<String>) = conn.query_row(
        "SELECT filename, notes, source_url FROM assets WHERE id = ?1",
        params![asset_id],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )?;
    let tags = load_tags_for(conn, asset_id)?.join(" ");
    conn.execute(
        "DELETE FROM assets_fts WHERE asset_id = ?1",
        params![asset_id],
    )?;
    conn.execute(
        "INSERT INTO assets_fts (asset_id, filename, notes, source_url, tags) VALUES (?1,?2,?3,?4,?5)",
        params![
            asset_id,
            filename,
            notes.unwrap_or_default(),
            source_url.unwrap_or_default(),
            tags
        ],
    )?;
    Ok(())
}

pub fn list_assets_by_tag(conn: &Connection, tag: &str, limit: i64) -> DbResult<Vec<Asset>> {
    let mut stmt = conn.prepare(&format!(
        r#"{ASSET_SELECT}
           WHERE id IN (
             SELECT at.asset_id FROM asset_tags at
             JOIN tags t ON t.id = at.tag_id
             WHERE t.name = ?1
           )
           ORDER BY created_at DESC LIMIT ?2"#
    ))?;
    let rows = stmt.query_map(params![tag, limit], |r| map_asset(conn, r))?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}

/// 列出带主色的素材，供颜色距离排序。
pub fn list_assets_with_color(conn: &Connection, limit: i64) -> DbResult<Vec<(Asset, String)>> {
    let mut stmt = conn.prepare(&format!(
        r#"{ASSET_SELECT}
           WHERE dominant_color IS NOT NULL AND dominant_color != ''
           ORDER BY created_at DESC LIMIT ?1"#
    ))?;
    let rows = stmt.query_map(params![limit], |r| {
        let a = map_asset(conn, r)?;
        let color = a.dominant_color.clone().unwrap_or_default();
        Ok((a, color))
    })?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}

pub fn insert_pipeline_run(
    conn: &Connection,
    run_id: &str,
    kind: &str,
    status: &str,
    detail: &str,
    now: &str,
) -> DbResult<()> {
    conn.execute(
        "INSERT INTO pipeline_runs (run_id, kind, status, detail, created_at) VALUES (?1,?2,?3,?4,?5)",
        params![run_id, kind, status, detail, now],
    )?;
    Ok(())
}

fn map_source(row: &rusqlite::Row<'_>) -> rusqlite::Result<Source> {
    Ok(Source {
        id: row.get(0)?,
        name: row.get(1)?,
        kind: row.get(2)?,
        url: row.get(3)?,
        interval_min: row.get(4)?,
        folder_id: row.get(5)?,
        enabled: row.get::<_, i64>(6)? != 0,
        last_run_at: row.get(7)?,
        last_error: row.get(8)?,
        created_at: row.get(9)?,
    })
}

const SOURCE_SELECT: &str = r#"
    SELECT id, name, kind, url, interval_min, folder_id, enabled, last_run_at, last_error, created_at
    FROM sources
"#;

pub fn insert_source(conn: &Connection, source: &Source) -> DbResult<()> {
    conn.execute(
        r#"INSERT INTO sources (
            id, name, kind, url, interval_min, folder_id, enabled,
            last_run_at, last_error, created_at
        ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)"#,
        params![
            source.id,
            source.name,
            source.kind,
            source.url,
            source.interval_min,
            source.folder_id,
            source.enabled as i64,
            source.last_run_at,
            source.last_error,
            source.created_at
        ],
    )?;
    Ok(())
}

pub fn list_sources(conn: &Connection) -> DbResult<Vec<Source>> {
    let mut stmt = conn.prepare(&format!("{SOURCE_SELECT} ORDER BY created_at DESC"))?;
    let rows = stmt.query_map([], map_source)?;
    Ok(rows.filter_map(|r| r.ok()).collect())
}

pub fn get_source(conn: &Connection, id: &str) -> DbResult<Option<Source>> {
    conn.query_row(
        &format!("{SOURCE_SELECT} WHERE id = ?1"),
        params![id],
        map_source,
    )
    .optional()
    .map_err(Into::into)
}

pub fn delete_source(conn: &Connection, id: &str) -> DbResult<bool> {
    let n = conn.execute("DELETE FROM sources WHERE id = ?1", params![id])?;
    Ok(n > 0)
}

pub fn set_source_enabled(conn: &Connection, id: &str, enabled: bool) -> DbResult<()> {
    conn.execute(
        "UPDATE sources SET enabled = ?2 WHERE id = ?1",
        params![id, enabled as i64],
    )?;
    Ok(())
}

pub fn update_source_run(
    conn: &Connection,
    id: &str,
    last_run_at: &str,
    last_error: Option<&str>,
) -> DbResult<()> {
    conn.execute(
        "UPDATE sources SET last_run_at = ?2, last_error = ?3 WHERE id = ?1",
        params![id, last_run_at, last_error],
    )?;
    Ok(())
}

fn parse_output_paths(raw: Option<String>) -> Vec<String> {
    raw.and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn output_paths_json(paths: &[String]) -> String {
    serde_json::to_string(paths).unwrap_or_else(|_| "[]".into())
}

fn map_gen_job(row: &rusqlite::Row<'_>) -> rusqlite::Result<GenJob> {
    let output_raw: Option<String> = row.get(13)?;
    Ok(GenJob {
        id: row.get(0)?,
        prompt: row.get(1)?,
        n: row.get(2)?,
        size: row.get(3)?,
        provider: row.get(4)?,
        model: row.get(5)?,
        status: row.get(6)?,
        folder_id: row.get(7)?,
        error: row.get(8)?,
        run_id: row.get(9)?,
        imported: row.get(10)?,
        created_at: row.get(11)?,
        finished_at: row.get(12)?,
        output_paths: parse_output_paths(output_raw),
    })
}

const GEN_JOB_SELECT: &str = r#"
    SELECT id, prompt, n, size, provider, model, status, folder_id, error, run_id,
           imported, created_at, finished_at, output_paths
    FROM gen_jobs
"#;

pub fn insert_gen_job(conn: &Connection, job: &GenJob) -> DbResult<()> {
    conn.execute(
        r#"INSERT INTO gen_jobs (
            id, prompt, n, size, provider, model, status, folder_id, error, run_id,
            imported, created_at, finished_at, output_paths
        ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14)"#,
        params![
            job.id,
            job.prompt,
            job.n,
            job.size,
            job.provider,
            job.model,
            job.status,
            job.folder_id,
            job.error,
            job.run_id,
            job.imported,
            job.created_at,
            job.finished_at,
            output_paths_json(&job.output_paths)
        ],
    )?;
    Ok(())
}

pub fn update_gen_job(conn: &Connection, job: &GenJob) -> DbResult<()> {
    conn.execute(
        r#"UPDATE gen_jobs SET
            status = ?2, error = ?3, run_id = ?4, imported = ?5, finished_at = ?6,
            output_paths = ?7, folder_id = ?8
         WHERE id = ?1"#,
        params![
            job.id,
            job.status,
            job.error,
            job.run_id,
            job.imported,
            job.finished_at,
            output_paths_json(&job.output_paths),
            job.folder_id
        ],
    )?;
    Ok(())
}

pub fn list_gen_jobs(conn: &Connection, limit: i64) -> DbResult<Vec<GenJob>> {
    let mut stmt = conn.prepare(&format!("{GEN_JOB_SELECT} ORDER BY created_at DESC LIMIT ?1"))?;
    let rows = stmt.query_map(params![limit], map_gen_job)?;
    Ok(rows.filter_map(|r| r.ok()).collect())
}

pub fn get_gen_job(conn: &Connection, id: &str) -> DbResult<Option<GenJob>> {
    conn.query_row(
        &format!("{GEN_JOB_SELECT} WHERE id = ?1"),
        params![id],
        map_gen_job,
    )
    .optional()
    .map_err(Into::into)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn migrate_and_count() {
        let dir = tempdir().unwrap();
        let db = dir.path().join("t.db");
        let conn = open_db(&db).unwrap();
        assert_eq!(asset_count(&conn).unwrap(), 0);
        assert!(list_sources(&conn).unwrap().is_empty());
        assert!(list_gen_jobs(&conn, 10).unwrap().is_empty());
    }

    #[test]
    fn virtual_folder_items_no_copy() {
        let dir = tempdir().unwrap();
        let db = dir.path().join("t.db");
        let conn = open_db(&db).unwrap();
        let folder = Folder {
            id: "f1".into(),
            name: "灵感".into(),
            parent_id: None,
            created_at: chrono::Utc::now().to_rfc3339(),
            item_count: 0,
        };
        insert_folder(&conn, &folder).unwrap();
        let n = add_folder_items(
            &conn,
            "f1",
            &[("path".into(), "/Volumes/Disk/a.jpg".into())],
        )
        .unwrap();
        assert_eq!(n, 1);
        let dup = add_folder_items(
            &conn,
            "f1",
            &[("path".into(), "/Volumes/Disk/a.jpg".into())],
        )
        .unwrap();
        assert_eq!(dup, 0);
        let listed = list_folders(&conn).unwrap();
        assert_eq!(listed[0].item_count, 1);
        let items = list_folder_items(&conn, "f1").unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].kind, "path");
        let removed = remove_folder_items(
            &conn,
            "f1",
            &[("path".into(), "/Volumes/Disk/a.jpg".into())],
        )
        .unwrap();
        assert_eq!(removed, 1);
        add_folder_items(&conn, "f1", &[("path".into(), "/tmp/b.png".into())]).unwrap();
        let (unlinked, deleted) = delete_folder(&conn, "f1").unwrap();
        assert_eq!(unlinked, 0);
        assert_eq!(deleted, 1);
        assert!(list_folder_items(&conn, "f1").unwrap().is_empty());
        assert!(list_folders(&conn).unwrap().is_empty());
    }
}
