//! 复用本机「照片」App 已算好的归类：人物 / 地点 / 关键词。
//! 只读 `Photos.sqlite`（与 browse 列文件同一路径），不调用私有 ML API。
//! Schema 随系统版本变化，查询前用 pragma 探测列名。

use rusqlite::{Connection, OpenFlags};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PhotosTaxonomyItem {
    pub name: String,
    pub count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct PhotosTaxonomy {
    pub library_path: String,
    pub people: Vec<PhotosTaxonomyItem>,
    pub places: Vec<PhotosTaxonomyItem>,
    pub keywords: Vec<PhotosTaxonomyItem>,
    pub available: bool,
    pub detail: Option<String>,
}

struct FaceCols {
    asset: String,
    person: String,
}

fn open_photos_db(lib: &Path) -> Result<Connection, String> {
    let db = lib.join("database/Photos.sqlite");
    if !db.is_file() {
        return Err(format!("无 Photos.sqlite: {}", db.display()));
    }
    // 优先只读打开原库；若被锁，再拷贝到临时文件
    match Connection::open_with_flags(&db, OpenFlags::SQLITE_OPEN_READ_ONLY) {
        Ok(conn) => {
            let _ = conn.busy_timeout(Duration::from_millis(800));
            // 探测能否读一张表
            if conn
                .prepare("SELECT 1 FROM ZASSET LIMIT 1")
                .and_then(|mut s| s.exists([]))
                .unwrap_or(false)
                || table_exists(&conn, "ZASSET")
            {
                return Ok(conn);
            }
            Ok(conn)
        }
        Err(e) => {
            log::warn!(
                target: "inspirex",
                "photos_db_direct_fail lib={} err={}; try_copy",
                lib.display(),
                e
            );
            open_photos_db_via_copy(&db)
        }
    }
}

fn open_photos_db_via_copy(db: &Path) -> Result<Connection, String> {
    let tmp_dir = std::env::temp_dir().join(format!(
        "inspirex-photos-{}",
        std::process::id()
    ));
    let _ = std::fs::create_dir_all(&tmp_dir);
    let dst = tmp_dir.join("Photos.sqlite");
    std::fs::copy(db, &dst).map_err(|e| format!("拷贝 Photos.sqlite 失败: {e}"))?;
    for suffix in ["-wal", "-shm"] {
        let src = PathBuf::from(format!("{}{suffix}", db.display()));
        if src.is_file() {
            let _ = std::fs::copy(&src, PathBuf::from(format!("{}{suffix}", dst.display())));
        }
    }
    let conn = Connection::open_with_flags(&dst, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|e| e.to_string())?;
    let _ = conn.busy_timeout(Duration::from_millis(800));
    Ok(conn)
}

fn table_exists(conn: &Connection, name: &str) -> bool {
    conn.query_row(
        "SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1",
        [name],
        |_| Ok(()),
    )
    .is_ok()
}

fn columns_of(conn: &Connection, table: &str) -> HashSet<String> {
    let mut out = HashSet::new();
    let Ok(mut stmt) = conn.prepare(&format!("PRAGMA table_info({table})")) else {
        return out;
    };
    let rows = stmt.query_map([], |r| r.get::<_, String>(1));
    if let Ok(rows) = rows {
        for row in rows.flatten() {
            out.insert(row);
        }
    }
    out
}

fn pick_col(cols: &HashSet<String>, candidates: &[&str]) -> Option<String> {
    candidates
        .iter()
        .find(|c| cols.contains(**c))
        .map(|s| (*s).to_string())
}

fn detect_face_cols(conn: &Connection) -> Option<FaceCols> {
    if !table_exists(conn, "ZDETECTEDFACE") {
        return None;
    }
    let cols = columns_of(conn, "ZDETECTEDFACE");
    let asset = pick_col(&cols, &["ZASSETFORFACE", "ZASSET"])?;
    let person = pick_col(&cols, &["ZPERSONFORFACE", "ZPERSON"])?;
    Some(FaceCols { asset, person })
}

fn person_name_expr(cols: &HashSet<String>) -> &'static str {
    if cols.contains("ZDISPLAYNAME") && cols.contains("ZFULLNAME") {
        "COALESCE(NULLIF(TRIM(ZDISPLAYNAME),''), NULLIF(TRIM(ZFULLNAME),''))"
    } else if cols.contains("ZFULLNAME") {
        "NULLIF(TRIM(ZFULLNAME),'')"
    } else if cols.contains("ZDISPLAYNAME") {
        "NULLIF(TRIM(ZDISPLAYNAME),'')"
    } else {
        "NULL"
    }
}

/// 列出人物（已命名优先）。
pub fn list_people(conn: &Connection) -> Result<Vec<PhotosTaxonomyItem>, String> {
    if !table_exists(conn, "ZPERSON") {
        return Ok(Vec::new());
    }
    let Some(face) = detect_face_cols(conn) else {
        return Ok(Vec::new());
    };
    let pcols = columns_of(conn, "ZPERSON");
    let name_expr = person_name_expr(&pcols);
    if name_expr == "NULL" {
        return Ok(Vec::new());
    }
    let sql = format!(
        "SELECT {name_expr} AS name, COUNT(DISTINCT f.{asset}) AS cnt
         FROM ZPERSON p
         JOIN ZDETECTEDFACE f ON f.{person} = p.Z_PK
         WHERE {name_expr} IS NOT NULL
         GROUP BY name
         HAVING cnt > 0
         ORDER BY cnt DESC, name COLLATE NOCASE
         LIMIT 200",
        asset = face.asset,
        person = face.person,
    );
    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |r| {
            Ok(PhotosTaxonomyItem {
                name: r.get(0)?,
                count: r.get(1)?,
            })
        })
        .map_err(|e| e.to_string())?;
    Ok(rows.filter_map(|r| r.ok()).collect())
}

/// 用户关键词（非 ML 场景，但是相册里现成的归类）。
pub fn list_keywords(conn: &Connection) -> Result<Vec<PhotosTaxonomyItem>, String> {
    if !table_exists(conn, "ZKEYWORD") {
        return Ok(Vec::new());
    }
    let kcols = columns_of(conn, "ZKEYWORD");
    let title = pick_col(&kcols, &["ZTITLE", "ZNAME"]).unwrap_or_else(|| "ZTITLE".into());
    // 关键词→素材 的中间表名随版本变；探测 Z_1KEYWORDS 一类
    let join_tables: Vec<String> = {
        let mut stmt = conn
            .prepare(
                "SELECT name FROM sqlite_master WHERE type='table' AND name LIKE 'Z_%KEYWORDS'",
            )
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], |r| r.get::<_, String>(0))
            .map_err(|e| e.to_string())?;
        rows.filter_map(|r| r.ok()).collect()
    };
    if join_tables.is_empty() {
        // 退化：只列关键词名，count=0 表示未知
        let sql = format!(
            "SELECT TRIM({title}) AS name, 0 AS cnt FROM ZKEYWORD
             WHERE TRIM({title}) != ''
             ORDER BY name COLLATE NOCASE LIMIT 200"
        );
        let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], |r| {
                Ok(PhotosTaxonomyItem {
                    name: r.get(0)?,
                    count: r.get(1)?,
                })
            })
            .map_err(|e| e.to_string())?;
        return Ok(rows.filter_map(|r| r.ok()).collect());
    }

    let mut best: HashMap<String, i64> = HashMap::new();
    for jt in join_tables {
        let jcols = columns_of(conn, &jt);
        let kw_col = jcols
            .iter()
            .find(|c| c.contains("KEYWORDS") && !c.starts_with("Z_1") && *c != "Z_1KEYWORDS")
            .cloned()
            .or_else(|| {
                jcols
                    .iter()
                    .find(|c| c.contains("KEYWORD"))
                    .cloned()
            });
        let Some(kw_col) = kw_col else { continue };
        // 另一列通常指向 asset attributes
        let other = jcols.iter().find(|c| **c != kw_col).cloned();
        let Some(_other) = other else { continue };
        let sql = format!(
            "SELECT TRIM(k.{title}) AS name, COUNT(*) AS cnt
             FROM {jt} j
             JOIN ZKEYWORD k ON k.Z_PK = j.{kw_col}
             WHERE TRIM(k.{title}) != ''
             GROUP BY name"
        );
        let Ok(mut stmt) = conn.prepare(&sql) else {
            continue;
        };
        let Ok(rows) = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))
        else {
            continue;
        };
        for row in rows.flatten() {
            let e = best.entry(row.0).or_insert(0);
            *e = (*e).max(row.1);
        }
    }
    let mut out: Vec<_> = best
        .into_iter()
        .map(|(name, count)| PhotosTaxonomyItem { name, count })
        .collect();
    out.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.name.cmp(&b.name)));
    out.truncate(200);
    Ok(out)
}

/// 按 GPS 粗聚合地点（相册逆地理 blob 难解析，用坐标桶 + 展示）。
pub fn list_places(conn: &Connection) -> Result<Vec<PhotosTaxonomyItem>, String> {
    if !table_exists(conn, "ZASSET") {
        return Ok(Vec::new());
    }
    let cols = columns_of(conn, "ZASSET");
    if !cols.contains("ZLATITUDE") || !cols.contains("ZLONGITUDE") {
        return Ok(Vec::new());
    }
    let sql = "SELECT ROUND(ZLATITUDE, 1), ROUND(ZLONGITUDE, 1), COUNT(*)
               FROM ZASSET
               WHERE IFNULL(ZTRASHEDSTATE,0)=0 AND IFNULL(ZHIDDEN,0)=0
                 AND ZLATITUDE IS NOT NULL AND ZLONGITUDE IS NOT NULL
                 AND ABS(ZLATITUDE) > 0.01 AND ABS(ZLONGITUDE) > 0.01
               GROUP BY 1, 2
               ORDER BY 3 DESC
               LIMIT 200";
    let mut stmt = conn.prepare(sql).map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |r| {
            let lat: f64 = r.get(0)?;
            let lng: f64 = r.get(1)?;
            let count: i64 = r.get(2)?;
            Ok((lat, lng, count))
        })
        .map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for row in rows.flatten() {
        let (lat, lng, count) = row;
        let name = crate::classify::place_label(lat, lng);
        out.push(PhotosTaxonomyItem { name, count });
    }
    // 合并同名城市
    let mut merged: HashMap<String, i64> = HashMap::new();
    for item in out {
        *merged.entry(item.name).or_insert(0) += item.count;
    }
    let mut out: Vec<_> = merged
        .into_iter()
        .map(|(name, count)| PhotosTaxonomyItem { name, count })
        .collect();
    out.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.name.cmp(&b.name)));
    Ok(out)
}

pub fn load_taxonomy(lib: &Path) -> PhotosTaxonomy {
    let t0 = Instant::now();
    let mut tax = PhotosTaxonomy {
        library_path: lib.display().to_string(),
        available: false,
        ..Default::default()
    };
    let conn = match open_photos_db(lib) {
        Ok(c) => c,
        Err(e) => {
            tax.detail = Some(e);
            log::warn!(
                target: "inspirex",
                "photos_taxonomy_unavailable lib={} err={}",
                lib.display(),
                tax.detail.as_deref().unwrap_or("")
            );
            return tax;
        }
    };
    tax.people = list_people(&conn).unwrap_or_default();
    tax.places = list_places(&conn).unwrap_or_default();
    tax.keywords = list_keywords(&conn).unwrap_or_default();
    tax.available = true;
    tax.detail = Some(format!(
        "people={} places={} keywords={} ms={}",
        tax.people.len(),
        tax.places.len(),
        tax.keywords.len(),
        t0.elapsed().as_millis()
    ));
    log::info!(
        target: "inspirex",
        "photos_taxonomy lib={} {}",
        lib.display(),
        tax.detail.as_deref().unwrap_or("")
    );
    tax
}

/// 某人物对应的 (directory, filename) 集合。
#[cfg(test)]
pub fn filenames_for_person(conn: &Connection, person: &str) -> Result<HashSet<(String, String)>, String> {
    let Some(face) = detect_face_cols(conn) else {
        return Ok(HashSet::new());
    };
    let pcols = columns_of(conn, "ZPERSON");
    let name_expr = person_name_expr(&pcols);
    if name_expr == "NULL" {
        return Ok(HashSet::new());
    }
    let sql = format!(
        "SELECT IFNULL(a.ZDIRECTORY,''), a.ZFILENAME
         FROM ZASSET a
         JOIN ZDETECTEDFACE f ON f.{asset} = a.Z_PK
         JOIN ZPERSON p ON f.{person} = p.Z_PK
         WHERE {name_expr} = ?1
           AND IFNULL(a.ZTRASHEDSTATE,0)=0 AND IFNULL(a.ZHIDDEN,0)=0
           AND a.ZFILENAME IS NOT NULL",
        asset = face.asset,
        person = face.person,
    );
    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([person], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
        .map_err(|e| e.to_string())?;
    Ok(rows.filter_map(|r| r.ok()).collect())
}

/// 为已解析出的相册文件附加人物名（批量，供列表 enrichment）。
pub fn people_by_filename(conn: &Connection) -> HashMap<(String, String), Vec<String>> {
    let mut map: HashMap<(String, String), Vec<String>> = HashMap::new();
    let Some(face) = detect_face_cols(conn) else {
        return map;
    };
    let pcols = columns_of(conn, "ZPERSON");
    let name_expr = person_name_expr(&pcols);
    if name_expr == "NULL" {
        return map;
    }
    let sql = format!(
        "SELECT IFNULL(a.ZDIRECTORY,''), a.ZFILENAME, {name_expr}
         FROM ZASSET a
         JOIN ZDETECTEDFACE f ON f.{asset} = a.Z_PK
         JOIN ZPERSON p ON f.{person} = p.Z_PK
         WHERE {name_expr} IS NOT NULL
           AND IFNULL(a.ZTRASHEDSTATE,0)=0
           AND a.ZFILENAME IS NOT NULL",
        asset = face.asset,
        person = face.person,
    );
    let Ok(mut stmt) = conn.prepare(&sql) else {
        return map;
    };
    let Ok(rows) = stmt.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
        ))
    }) else {
        return map;
    };
    for row in rows.flatten() {
        let (dir, file, name) = row;
        let entry = map.entry((dir, file)).or_default();
        if !entry.iter().any(|n| n == &name) {
            entry.push(name);
        }
    }
    map
}

pub fn place_by_filename(conn: &Connection) -> HashMap<(String, String), String> {
    let mut map = HashMap::new();
    let cols = columns_of(conn, "ZASSET");
    if !cols.contains("ZLATITUDE") || !cols.contains("ZLONGITUDE") {
        return map;
    }
    let sql = "SELECT IFNULL(ZDIRECTORY,''), ZFILENAME, ZLATITUDE, ZLONGITUDE
               FROM ZASSET
               WHERE IFNULL(ZTRASHEDSTATE,0)=0
                 AND ZFILENAME IS NOT NULL
                 AND ZLATITUDE IS NOT NULL AND ZLONGITUDE IS NOT NULL
                 AND ABS(ZLATITUDE) > 0.01 AND ABS(ZLONGITUDE) > 0.01";
    let Ok(mut stmt) = conn.prepare(sql) else {
        return map;
    };
    let Ok(rows) = stmt.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, f64>(2)?,
            r.get::<_, f64>(3)?,
        ))
    }) else {
        return map;
    };
    for row in rows.flatten() {
        let (dir, file, lat, lng) = row;
        map.insert((dir, file), crate::classify::place_label(lat, lng));
    }
    map
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn fixture_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            r#"
            CREATE TABLE ZASSET (
              Z_PK INTEGER PRIMARY KEY,
              ZDIRECTORY TEXT,
              ZFILENAME TEXT,
              ZTRASHEDSTATE INTEGER DEFAULT 0,
              ZHIDDEN INTEGER DEFAULT 0,
              ZLATITUDE REAL,
              ZLONGITUDE REAL,
              ZKIND INTEGER,
              ZDATECREATED REAL
            );
            CREATE TABLE ZPERSON (
              Z_PK INTEGER PRIMARY KEY,
              ZDISPLAYNAME TEXT,
              ZFULLNAME TEXT
            );
            CREATE TABLE ZDETECTEDFACE (
              Z_PK INTEGER PRIMARY KEY,
              ZASSETFORFACE INTEGER,
              ZPERSONFORFACE INTEGER
            );
            CREATE TABLE ZKEYWORD (Z_PK INTEGER PRIMARY KEY, ZTITLE TEXT);
            INSERT INTO ZASSET VALUES (1,'A','a.jpg',0,0,31.23,121.47,0,1);
            INSERT INTO ZASSET VALUES (2,'B','b.jpg',0,0,39.90,116.40,0,2);
            INSERT INTO ZPERSON VALUES (1,'小明','小明');
            INSERT INTO ZPERSON VALUES (2,'','未命名');
            INSERT INTO ZDETECTEDFACE VALUES (1,1,1);
            INSERT INTO ZDETECTEDFACE VALUES (2,2,1);
            INSERT INTO ZKEYWORD VALUES (1,'旅行');
            "#,
        )
        .unwrap();
        conn
    }

    #[test]
    fn people_and_places_from_fixture() {
        let conn = fixture_db();
        let people = list_people(&conn).unwrap();
        assert_eq!(people.len(), 1);
        assert_eq!(people[0].name, "小明");
        assert_eq!(people[0].count, 2);
        let places = list_places(&conn).unwrap();
        assert!(places.iter().any(|p| p.name.contains("上海") || p.name.contains("北京")));
        let set = filenames_for_person(&conn, "小明").unwrap();
        assert_eq!(set.len(), 2);
    }
}
