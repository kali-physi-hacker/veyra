//! SQLite persistence. One writer connection publishes staged generations atomically; a pool of
//! reader connections serves queries concurrently through WAL, so a running scan never blocks a
//! page. Readers see the published index, or, when they opt into the visible generation, a
//! root's first scan while it is still running.
use rusqlite::{Connection, OptionalExtension, Row, params, params_from_iter, types::Value};
use serde::{Serialize, de::DeserializeOwned};
use std::{
    collections::HashMap,
    ops::{Deref, DerefMut},
    path::{Path, PathBuf},
    sync::{
        Mutex, MutexGuard,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::Duration,
};
use stratum_domain::*;

fn db(e: rusqlite::Error) -> Error {
    Error::new("database_error", e.to_string())
}
const SCHEMA_VERSION: u32 = 5;
/// The secondary indexes, kept over published rows only. Publication rebuilds them in one
/// sorted pass rather than touching them once per row, and the rebuild packs their pages.
/// Sorting by allocated bytes and filtering by category read the size index instead; both
/// are rare, and every index here is paid for on every publication.
const INDEXES: [(&str, &str); 5] = [
    (
        "entries_size",
        "ON entries(scan_id,kind,logical DESC,path) WHERE published=1",
    ),
    (
        "entries_modified",
        "ON entries(scan_id,modified) WHERE published=1",
    ),
    ("entries_path", "ON entries(path,scan_id) WHERE published=1"),
    (
        "entries_directory_name",
        "ON entries(scan_id,name,logical DESC) WHERE kind='directory' AND published=1",
    ),
    (
        "entries_parent_size",
        "ON entries(scan_id,parent,logical DESC,path,allocated) WHERE published=1",
    ),
];
/// Reader connections kept open between queries.
const READER_POOL: usize = 6;
/// The entry columns, in the order `entry_row` reads them; `e` must alias the entries table.
const COLUMNS: &str = "e.path,e.parent,e.name,e.kind,e.logical,e.allocated,e.modified,e.created,e.accessed,e.extension,e.category,e.confidence,e.reason,e.device,e.inode,e.size,e.modified_ns,e.changed_ns,e.links,e.depth";
const INSERT: &str = "INSERT OR REPLACE INTO entries(scan_id,path,parent,name,kind,logical,allocated,modified,created,accessed,extension,category,confidence,reason,device,inode,size,modified_ns,changed_ns,links,depth,published,data) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21,?22,'')";

/// The half-open path range of everything below `path`: `path/` up to the next byte after `/`.
fn subtree_bounds(path: &str) -> (String, String) {
    let lower = format!("{}/", path.trim_end_matches('/'));
    let mut upper = lower.clone();
    upper.pop();
    upper.push('0');
    (lower, upper)
}
fn sort_entries(items: &mut [Entry], sort: &str) {
    items.sort_by(|a, b| match sort {
        "allocated_bytes" => b
            .allocated_bytes
            .cmp(&a.allocated_bytes)
            .then_with(|| a.path.cmp(&b.path)),
        "modified_at" => b
            .modified_at
            .cmp(&a.modified_at)
            .then_with(|| a.path.cmp(&b.path)),
        "path" => a.path.cmp(&b.path),
        _ => b
            .logical_bytes
            .cmp(&a.logical_bytes)
            .then_with(|| a.path.cmp(&b.path)),
    });
}

fn entry_row(r: &Row<'_>) -> rusqlite::Result<Entry> {
    let kind: String = r.get(3)?;
    let reason: String = r.get(12)?;
    Ok(Entry {
        path: r.get(0)?,
        parent: r.get(1)?,
        name: r.get(2)?,
        kind: match kind.as_str() {
            "file" => EntryKind::File,
            "directory" => EntryKind::Directory,
            "symlink" => EntryKind::Symlink,
            _ => EntryKind::Other,
        },
        logical_bytes: r.get::<_, i64>(4)? as u64,
        allocated_bytes: r.get::<_, i64>(5)? as u64,
        modified_at: r.get(6)?,
        created_at: r.get(7)?,
        accessed_at: r.get(8)?,
        extension: r.get(9)?,
        category: r.get(10)?,
        confidence: r.get::<_, f64>(11)? as f32,
        evidence: vec![Evidence::new("path_classification", reason)],
        identity: Identity {
            device: r.get::<_, i64>(13)? as u64,
            inode: r.get::<_, i64>(14)? as u64,
            size: r.get::<_, i64>(15)? as u64,
            modified_ns: r.get(16)?,
            changed_ns: r.get(17)?,
            links: r.get::<_, i64>(18)? as u64,
        },
        depth: r.get::<_, i64>(19)? as u32,
    })
}
fn signed(bytes: u64) -> Result<i64> {
    i64::try_from(bytes)
        .map_err(|_| Error::invalid("File size exceeds SQLite signed integer range"))
}
fn insert_entry(
    stmt: &mut rusqlite::Statement<'_>,
    scan_id: &str,
    e: &Entry,
    published: bool,
) -> Result<()> {
    stmt.execute(params![
        scan_id,
        e.path,
        e.parent,
        e.name,
        e.kind.as_str(),
        signed(e.logical_bytes)?,
        signed(e.allocated_bytes)?,
        e.modified_at,
        e.created_at,
        e.accessed_at,
        e.extension,
        e.category,
        f64::from(e.confidence),
        e.evidence.first().map_or("", |v| v.detail.as_str()),
        e.identity.device as i64,
        e.identity.inode as i64,
        e.identity.size as i64,
        e.identity.modified_ns,
        e.identity.changed_ns,
        e.identity.links as i64,
        i64::from(e.depth),
        i64::from(published),
    ])
    .map_err(db)?;
    Ok(())
}

pub struct Store {
    path: PathBuf,
    writer: Mutex<Connection>,
    readers: Mutex<Vec<Connection>>,
    live: AtomicBool,
    version: AtomicU64,
}
pub struct FingerprintUpdate {
    pub path: String,
    pub identity: Identity,
    pub sample: String,
    pub full: Option<String>,
}
/// A pooled reader connection, returned to the pool when dropped.
pub struct Reader<'a> {
    store: &'a Store,
    connection: Option<Connection>,
}
impl Deref for Reader<'_> {
    type Target = Connection;
    fn deref(&self) -> &Connection {
        self.connection
            .as_ref()
            .expect("connection is held until drop")
    }
}
impl DerefMut for Reader<'_> {
    fn deref_mut(&mut self) -> &mut Connection {
        self.connection
            .as_mut()
            .expect("connection is held until drop")
    }
}
impl Drop for Reader<'_> {
    fn drop(&mut self) {
        if let Some(connection) = self.connection.take()
            && let Ok(mut pool) = self.store.readers.lock()
            && pool.len() < READER_POOL
        {
            pool.push(connection);
        }
    }
}

const MIGRATIONS: [(u32, &str); 5] = [
    (1, include_str!("../../../migrations/001_initial.sql")),
    (2, include_str!("../../../migrations/002_query_paths.sql")),
    (
        3,
        include_str!("../../../migrations/003_experience_queries.sql"),
    ),
    (
        4,
        include_str!("../../../migrations/004_compact_query_indexes.sql"),
    ),
    (
        5,
        include_str!("../../../migrations/005_visible_generations.sql"),
    ),
];
/// Tables carried over from a database written before schema version 5. Entries, roots and
/// category totals are not among them: those generations are dropped, see migration 5.
const CARRIED_TABLES: [&str; 8] = [
    "scans",
    "warnings",
    "history",
    "documents",
    "fingerprints",
    "duplicate_members",
    "audit",
    "system_history",
];

fn open_connection(path: &Path) -> Result<Connection> {
    let connection = Connection::open(path).map_err(db)?;
    connection.busy_timeout(BUSY).map_err(db)?;
    // Larger pages suit long path keys and sequential index builds. The size only takes
    // effect on a new database; an existing one keeps the size it was created with. Whenever
    // the WAL restarts from the beginning, SQLite cuts the file back to journal_size_limit.
    connection.execute_batch(&format!("PRAGMA page_size=16384; PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL; PRAGMA foreign_keys=ON; PRAGMA cache_size=-16384; PRAGMA temp_store=FILE; PRAGMA journal_size_limit={WAL_KEEP};")).map_err(db)?;
    Ok(connection)
}
/// How long a connection waits for a lock before reporting the database busy.
const BUSY: Duration = Duration::from_secs(5);
/// The WAL file is cut back to this size whenever SQLite restarts it.
const WAL_KEEP: u64 = 64 << 20;
/// While a scan streams, a WAL past this size is checkpointed and truncated between batches,
/// waiting briefly for readers to finish.
const WAL_SOFT_LIMIT: u64 = 512 << 20;
/// Past this size the truncation waits longer for readers, pausing the scan for a moment rather
/// than letting live queries keep the WAL from ever restarting.
const WAL_HARD_LIMIT: u64 = 2 << 30;

/// Copies every WAL frame into the database and truncates the WAL to nothing, waiting at most
/// `wait` for readers to move off it. False when readers or another checkpoint held it.
fn truncate_wal(connection: &Connection, wait: Duration) -> Result<bool> {
    connection.busy_timeout(wait).map_err(db)?;
    let busy: std::result::Result<i64, _> =
        connection.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |r| r.get(0));
    connection.busy_timeout(BUSY).map_err(db)?;
    match busy {
        Ok(busy) => Ok(busy == 0),
        Err(rusqlite::Error::SqliteFailure(e, _))
            if e.code == rusqlite::ErrorCode::DatabaseBusy =>
        {
            Ok(false)
        }
        Err(e) => Err(db(e)),
    }
}
fn schema_version(connection: &Connection) -> Result<u32> {
    connection
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .map_err(db)
}
/// A database from before schema version 5 stored every entry as JSON and can be tens of
/// gigabytes. Freeing that table page by page takes SQLite as long as reading it, so the file
/// is rebuilt instead: the small tables are copied into a fresh file at the same schema
/// version, which then takes the old file's place, and the ordinary migrations follow.
fn wal_bytes(path: &Path) -> u64 {
    std::fs::metadata(sidecar(path, "-wal"))
        .map(|m| m.len())
        .unwrap_or(0)
}
fn sidecar(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(suffix);
    PathBuf::from(name)
}
fn rebuild_legacy(path: &Path, version: u32) -> Result<()> {
    let rebuilt = sidecar(path, ".rebuild");
    for stale in [
        rebuilt.clone(),
        sidecar(&rebuilt, "-journal"),
        sidecar(&rebuilt, "-wal"),
        sidecar(&rebuilt, "-shm"),
    ] {
        if stale.exists() {
            std::fs::remove_file(&stale)?;
        }
    }
    {
        let fresh = Connection::open(&rebuilt).map_err(db)?;
        fresh.execute_batch("PRAGMA page_size=16384;").map_err(db)?;
        for (target, sql) in MIGRATIONS {
            if target <= version {
                fresh.execute_batch(sql).map_err(db)?;
            }
        }
        fresh
            .execute(
                "ATTACH DATABASE ?1 AS old",
                [path
                    .to_str()
                    .ok_or_else(|| Error::invalid("Non UTF-8 state path"))?],
            )
            .map_err(db)?;
        fresh.execute_batch("BEGIN").map_err(db)?;
        for table in CARRIED_TABLES {
            fresh
                .execute_batch(&format!("INSERT INTO {table} SELECT * FROM old.{table}"))
                .map_err(db)?;
        }
        fresh
            .execute_batch("COMMIT; DETACH DATABASE old;")
            .map_err(db)?;
    }
    let (wal, shm) = (sidecar(path, "-wal"), sidecar(path, "-shm"));
    std::fs::rename(&rebuilt, path)?;
    for leftover in [wal, shm] {
        if leftover.exists() {
            std::fs::remove_file(&leftover)?;
        }
    }
    Ok(())
}

impl Store {
    pub fn open(path: &Path) -> Result<Self> {
        let mut connection = open_connection(path)?;
        let mut version = schema_version(&connection)?;
        if version > SCHEMA_VERSION {
            return Err(Error::new(
                "unsupported_schema",
                "Database was created by a newer Stratum release",
            ));
        }
        if (1..5).contains(&version) {
            drop(connection);
            rebuild_legacy(path, version)?;
            connection = open_connection(path)?;
            version = schema_version(&connection)?;
        }
        for (target, sql) in MIGRATIONS {
            if version < target {
                connection
                    .execute_batch(&format!("BEGIN IMMEDIATE;{sql}COMMIT;"))
                    .map_err(db)?;
            }
        }
        // A WAL left large by a crash, or by a release that never truncated it, is folded back
        // into the database now. Another process reading the index can keep it for later.
        if wal_bytes(path) > 0 {
            match truncate_wal(&connection, Duration::from_millis(250)) {
                Ok(true) => tracing::debug!("truncated the write-ahead log on open"),
                Ok(false) => tracing::debug!("write-ahead log in use elsewhere; left for later"),
                Err(e) => {
                    tracing::warn!(error = %e.message, "could not truncate the write-ahead log")
                }
            }
        }
        Ok(Self {
            path: path.to_path_buf(),
            writer: Mutex::new(connection),
            readers: Mutex::new(Vec::new()),
            live: AtomicBool::new(false),
            version: AtomicU64::new(1),
        })
    }
    fn writer(&self) -> Result<MutexGuard<'_, Connection>> {
        self.writer
            .lock()
            .map_err(|_| Error::new("database_error", "Database mutex poisoned"))
    }
    fn reader(&self) -> Result<Reader<'_>> {
        let pooled = self
            .readers
            .lock()
            .map_err(|_| Error::new("database_error", "Reader pool mutex poisoned"))?
            .pop();
        let connection = match pooled {
            Some(connection) => connection,
            None => {
                let connection = Connection::open(&self.path).map_err(db)?;
                connection.busy_timeout(BUSY).map_err(db)?;
                connection
                    .execute_batch("PRAGMA query_only=ON; PRAGMA cache_size=-16384;")
                    .map_err(db)?;
                connection
            }
        };
        Ok(Reader {
            store: self,
            connection: Some(connection),
        })
    }
    /// Bumped by every write, so cached derivations can tell whether they are current.
    pub fn version(&self) -> u64 {
        self.version.load(Ordering::Acquire)
    }
    fn bump(&self) {
        self.version.fetch_add(1, Ordering::AcqRel);
    }
    /// Read the visible generation of every root (a first scan while it runs) instead of only
    /// published generations. Rescans stay invisible until they publish either way.
    pub fn set_live_view(&self, on: bool) {
        self.live.store(on, Ordering::Relaxed);
        self.bump();
    }
    pub fn live_view(&self) -> bool {
        self.live.load(Ordering::Relaxed)
    }
    fn roots_source(&self) -> &'static str {
        if self.live_view() {
            "visible_roots"
        } else {
            "roots"
        }
    }
    fn entries_source(&self) -> &'static str {
        if self.live_view() {
            "visible_entries"
        } else {
            "current_entries"
        }
    }
    /// Bulk mode trades commit durability for throughput while a scan streams entries: WAL
    /// commits stop forcing a disk flush, checkpoints become rare, and the page cache grows so
    /// the pages a generation touches stay resident until publication has indexed it. The scan
    /// holds the writer lock, so no journal record of a cleanup is written meanwhile.
    pub fn set_bulk(&self, on: bool) -> Result<()> {
        let conn = self.writer()?;
        conn.execute_batch(if on {
            "PRAGMA synchronous=NORMAL; PRAGMA cache_size=-262144; PRAGMA wal_autocheckpoint=25000; PRAGMA threads=4;"
        } else {
            "PRAGMA synchronous=FULL; PRAGMA cache_size=-16384; PRAGMA wal_autocheckpoint=1000; PRAGMA threads=0;"
        })
        .map_err(db)?;
        if !on {
            // The scan and its publication can leave gigabytes of WAL behind. Fold them into
            // the database and give the space back, waiting for the pages that refresh on
            // completion to finish reading.
            for _ in 0..3 {
                if truncate_wal(&conn, Duration::from_secs(2))? {
                    return Ok(());
                }
            }
            tracing::warn!(
                wal_bytes = wal_bytes(&self.path),
                "write-ahead log still in use after the scan; it is truncated when next idle"
            );
            conn.execute_batch("PRAGMA wal_checkpoint(PASSIVE);")
                .map_err(db)?;
        }
        Ok(())
    }
    /// The size of the write-ahead log on disk.
    pub fn wal_bytes(&self) -> u64 {
        wal_bytes(&self.path)
    }
    /// Scans that are running for roots without a published generation.
    fn running_first_scans(&self) -> Result<Vec<String>> {
        let conn = self.reader()?;
        let mut s = conn
            .prepare("SELECT id FROM scans WHERE status='running' AND root NOT IN (SELECT path FROM roots) ORDER BY started,id")
            .map_err(db)?;
        s.query_map([], |r| r.get(0))
            .map_err(db)?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(db)
    }
    pub fn recover_scans(&self) -> Result<()> {
        let conn = self.writer()?;
        conn.execute(
            "UPDATE scans SET status='interrupted',freshness='unknown' WHERE status='running'",
            [],
        )
        .map_err(db)?;
        conn.execute("DELETE FROM entries WHERE scan_id IN (SELECT id FROM scans WHERE status IN ('interrupted','failed','cancelled'))", []).map_err(db)?;
        conn.execute("DELETE FROM category_totals WHERE scan_id IN (SELECT id FROM scans WHERE status IN ('interrupted','failed','cancelled'))", []).map_err(db)?;
        drop(conn);
        self.bump();
        Ok(())
    }
    pub fn begin_scan(&self, scan: &ScanRecord) -> Result<()> {
        self.writer()?
            .execute(
                "INSERT INTO scans(id,root,started,status) VALUES(?1,?2,?3,'running')",
                params![scan.id, scan.root, scan.started_at],
            )
            .map_err(db)?;
        self.bump();
        self.audit("scan_started", &scan.id, &scan.root)
    }
    /// Insert a batch in one transaction. Rows go in path order, which packs the primary key's
    /// pages, and they are unpublished, so no secondary index is touched until publication.
    /// File category totals are kept current so a running first scan reads correctly.
    pub fn insert_batch(&self, scan_id: &str, entries: &[Entry]) -> Result<()> {
        if entries.is_empty() {
            return Ok(());
        }
        let mut order: Vec<&Entry> = entries.iter().collect();
        order.sort_by(|a, b| a.path.cmp(&b.path));
        let mut totals: HashMap<&str, (i64, i64, i64)> = HashMap::new();
        let mut conn = self.writer()?;
        let tx = conn.transaction().map_err(db)?;
        {
            let mut stmt = tx.prepare_cached(INSERT).map_err(db)?;
            for e in order {
                insert_entry(&mut stmt, scan_id, e, false)?;
                if e.kind == EntryKind::File {
                    let t = totals.entry(e.category.as_str()).or_default();
                    t.0 += signed(e.logical_bytes)?;
                    t.1 += signed(e.allocated_bytes)?;
                    t.2 += 1;
                }
            }
            let mut stmt = tx.prepare_cached("INSERT INTO category_totals(scan_id,category,logical,allocated,files) VALUES(?1,?2,?3,?4,?5) ON CONFLICT(scan_id,category) DO UPDATE SET logical=logical+excluded.logical,allocated=allocated+excluded.allocated,files=files+excluded.files").map_err(db)?;
            for (category, (logical, allocated, files)) in totals {
                stmt.execute(params![scan_id, category, logical, allocated, files])
                    .map_err(db)?;
            }
        }
        tx.commit().map_err(db)?;
        // Pages that read the running scan can keep SQLite from ever restarting the WAL, so a
        // long scan truncates it here once it grows, waiting a little longer the larger it is.
        let wal = wal_bytes(&self.path);
        if wal > WAL_SOFT_LIMIT {
            let wait = if wal > WAL_HARD_LIMIT {
                Duration::from_secs(2)
            } else {
                Duration::from_millis(100)
            };
            if !truncate_wal(&conn, wait)? {
                tracing::debug!(
                    wal_bytes = wal,
                    "write-ahead log busy with readers; truncating after a later batch"
                );
            }
        }
        drop(conn);
        self.bump();
        Ok(())
    }
    pub fn warning(&self, scan_id: &str, path: &str, code: &str, message: &str) -> Result<()> {
        self.writer()?.execute("INSERT INTO warnings(scan_id,path,code,message) SELECT ?1,?2,?3,?4 WHERE (SELECT count(*) FROM warnings WHERE scan_id=?1)<10000", params![scan_id,path,code,message]).map_err(db)?;
        Ok(())
    }
    pub fn finish_scan(&self, scan: &ScanRecord, retain_days: u32) -> Result<()> {
        let mut conn = self.writer()?;
        // Publication rewrites the whole generation in one transaction; start it from an empty
        // WAL so the file peaks at the size of that transaction alone.
        if wal_bytes(&self.path) > WAL_KEEP {
            truncate_wal(&conn, Duration::from_secs(2))?;
        }
        let tx = conn.transaction().map_err(db)?;
        tx.execute("UPDATE scans SET completed=?2,status=?3,entries=?4,warnings=?5,excluded=?6,logical=?7,allocated=?8,freshness=?9 WHERE id=?1", params![scan.id,scan.completed_at,scan.status,scan.entries as i64,scan.warnings as i64,scan.excluded as i64,scan.logical_bytes as i64,scan.allocated_bytes as i64,scan.freshness]).map_err(db)?;
        if scan.status == "completed" || scan.status == "partial" {
            // With the secondary indexes dropped, publishing the new generation and removing
            // the old one are sequential passes over the table; the rebuild then sorts every
            // published row once per index. Readers keep the previous snapshot until commit.
            for (name, _) in INDEXES {
                tx.execute_batch(&format!("DROP INDEX IF EXISTS {name}"))
                    .map_err(db)?;
            }
            tx.execute(
                "UPDATE entries SET published=1 WHERE scan_id=?1",
                [&scan.id],
            )
            .map_err(db)?;
            tx.execute("DELETE FROM entries WHERE scan_id IN (SELECT id FROM scans WHERE root=?1 AND id<>?2)",params![scan.root,scan.id]).map_err(db)?;
            for (name, definition) in INDEXES {
                tx.execute_batch(&format!("CREATE INDEX {name} {definition}"))
                    .map_err(db)?;
            }
            tx.execute(
                "DELETE FROM category_totals WHERE scan_id IN (SELECT id FROM scans WHERE root=?1)",
                [&scan.root],
            )
            .map_err(db)?;
            tx.execute("INSERT INTO category_totals SELECT scan_id,category,sum(logical),sum(allocated),count(*) FROM entries INDEXED BY sqlite_autoindex_entries_1 WHERE scan_id=?1 AND kind='file' GROUP BY category", [&scan.id]).map_err(db)?;
            tx.execute("INSERT INTO roots(path,scan_id) VALUES(?1,?2) ON CONFLICT(path) DO UPDATE SET scan_id=excluded.scan_id", params![scan.root,scan.id]).map_err(db)?;
            tx.execute("INSERT INTO history SELECT scan_id,path,?2,logical,allocated,?3 FROM entries INDEXED BY sqlite_autoindex_entries_1 WHERE scan_id=?1 AND kind='directory' AND depth<=1", params![scan.id,scan.completed_at,scan.status]).map_err(db)?;
            tx.execute(
                "DELETE FROM fingerprints WHERE path NOT IN (SELECT path FROM current_entries)",
                [],
            )
            .map_err(db)?;
        } else {
            tx.execute("DELETE FROM entries WHERE scan_id=?1", [&scan.id])
                .map_err(db)?;
            tx.execute("DELETE FROM category_totals WHERE scan_id=?1", [&scan.id])
                .map_err(db)?;
        }
        tx.execute(
            "DELETE FROM history WHERE timestamp < ?1",
            [now() - i64::from(retain_days) * 86400],
        )
        .map_err(db)?;
        tx.commit().map_err(db)?;
        drop(conn);
        self.bump();
        self.audit("scan_finished", &scan.id, &scan.status)
    }
    pub fn scans(&self) -> Result<Vec<ScanRecord>> {
        self.scan_records(false)
    }
    pub fn published_scans(&self) -> Result<Vec<ScanRecord>> {
        self.scan_records(true)
    }
    fn scan_records(&self, published: bool) -> Result<Vec<ScanRecord>> {
        let conn = self.reader()?;
        let condition = if published {
            format!("WHERE id IN (SELECT scan_id FROM {})", self.roots_source())
        } else {
            String::new()
        };
        let mut s = conn.prepare(&format!("SELECT id,root,started,completed,status,entries,warnings,excluded,logical,allocated,freshness FROM scans {condition} ORDER BY started DESC,id LIMIT 1000")).map_err(db)?;
        s.query_map([], |r| {
            Ok(ScanRecord {
                id: r.get(0)?,
                root: r.get(1)?,
                started_at: r.get(2)?,
                completed_at: r.get(3)?,
                status: r.get(4)?,
                entries: r.get::<_, i64>(5)? as u64,
                warnings: r.get::<_, i64>(6)? as u64,
                excluded: r.get::<_, i64>(7)? as u64,
                logical_bytes: r.get::<_, i64>(8)? as u64,
                allocated_bytes: r.get::<_, i64>(9)? as u64,
                freshness: r.get(10)?,
            })
        })
        .map_err(db)?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(db)
    }
    /// Roots with a published generation.
    pub fn roots(&self) -> Result<Vec<String>> {
        let conn = self.reader()?;
        let mut s = conn
            .prepare("SELECT path FROM roots ORDER BY path")
            .map_err(db)?;
        s.query_map([], |r| r.get(0))
            .map_err(db)?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(db)
    }
    pub fn scan_policy(&self, root: &str) -> Result<ScanRequest> {
        let text:String=self.reader()?.query_row("SELECT d.data FROM documents d JOIN roots r ON d.id=r.scan_id WHERE d.kind='scan_policy' AND r.path=?1",[root],|r|r.get(0)).map_err(db)?;
        Ok(serde_json::from_str(&text)?)
    }
    pub fn mark_stale(&self, root: &str) -> Result<()> {
        self.writer()?.execute("UPDATE scans SET freshness='stale' WHERE id IN (SELECT scan_id FROM roots WHERE path=?1)",[root]).map_err(db)?;
        self.bump();
        Ok(())
    }
    pub fn warnings(&self, scan_id: &str) -> Result<Vec<Evidence>> {
        let conn = self.reader()?;
        let mut s = conn
            .prepare(
                "SELECT code,path,message FROM warnings WHERE scan_id=?1 ORDER BY id LIMIT 10000",
            )
            .map_err(db)?;
        s.query_map([scan_id], |r| {
            Ok(Evidence::new(
                &r.get::<_, String>(0)?,
                format!("{}: {}", r.get::<_, String>(1)?, r.get::<_, String>(2)?),
            ))
        })
        .map_err(db)?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(db)
    }
    pub fn files(&self, q: &FileQuery) -> Result<Page<Entry>> {
        if q.limit == 0 || q.limit > 1000 || q.offset > i64::MAX as u64 {
            return Err(Error::invalid(
                "limit must be 1..1000 and offset must fit i64",
            ));
        }
        if !matches!(
            q.sort.as_str(),
            "logical_bytes" | "allocated_bytes" | "modified_at" | "path"
        ) {
            return Err(Error::invalid("Unsupported sort"));
        }
        let running = if self.live_view() {
            self.running_first_scans()?
        } else {
            Vec::new()
        };
        if running.is_empty() {
            let mut items = self.published_files(q, q.offset as i64, i64::from(q.limit) + 1)?;
            let has_more = items.len() > q.limit as usize;
            items.truncate(q.limit as usize);
            return Ok(Page {
                items,
                limit: q.limit,
                offset: q.offset,
                has_more,
            });
        }
        // Published generations answer from their indexes; a running first scan answers from
        // its primary key. The merge keeps one ordering across both.
        let fetch = (q.offset as i64)
            .saturating_add(i64::from(q.limit))
            .saturating_add(1);
        let mut items = self.published_files(q, 0, fetch)?;
        for scan_id in &running {
            items.extend(self.running_files(q, scan_id, fetch)?);
        }
        sort_entries(&mut items, &q.sort);
        let has_more = items.len() as u64 > q.offset + u64::from(q.limit);
        let items = items
            .into_iter()
            .skip(q.offset as usize)
            .take(q.limit as usize)
            .collect();
        Ok(Page {
            items,
            limit: q.limit,
            offset: q.offset,
            has_more,
        })
    }
    /// The WHERE terms shared by every file query, appended to `sql` with their values.
    fn file_filters(q: &FileQuery, sql: &mut String, values: &mut Vec<Value>) {
        for (column, value) in [
            ("e.parent", &q.parent),
            ("e.extension", &q.extension),
            ("e.kind", &q.kind),
            ("e.category", &q.category),
        ] {
            if let Some(value) = value {
                sql.push_str(&format!(" AND {column}=?"));
                values.push(Value::Text(value.clone()));
            }
        }
        if let Some(path) = &q.path {
            sql.push_str(" AND (e.path=? OR substr(e.path,1,length(?))=?)");
            let prefix = format!("{}/", path.trim_end_matches('/'));
            values.extend([
                Value::Text(path.clone()),
                Value::Text(prefix.clone()),
                Value::Text(prefix),
            ]);
        }
        if let Some(name) = &q.name {
            sql.push_str(" AND e.name GLOB ?");
            values.push(Value::Text(name.clone()));
        }
        if !q.names.is_empty() {
            sql.push_str(" AND e.name IN (");
            sql.push_str(&vec!["?"; q.names.len()].join(","));
            sql.push(')');
            values.extend(q.names.iter().map(|n| Value::Text(n.clone())));
        }
        for (column, op, value) in [
            (
                "e.logical",
                ">=",
                q.min_size.map(|n| i64::try_from(n).unwrap_or(i64::MAX)),
            ),
            (
                "e.logical",
                "<=",
                q.max_size.map(|n| i64::try_from(n).unwrap_or(i64::MAX)),
            ),
            ("e.modified", "<", q.modified_before),
            ("e.modified", ">", q.modified_after),
            ("e.created", "<", q.created_before),
            ("e.created", ">", q.created_after),
        ] {
            if let Some(v) = value {
                sql.push_str(&format!(" AND {column}{op}?"));
                values.push(Value::Integer(v));
            }
        }
    }
    fn sort_clause(sort: &str) -> &'static str {
        match sort {
            "allocated_bytes" => "e.allocated DESC,e.path",
            "modified_at" => "e.modified DESC,e.path",
            "path" => "e.path",
            _ => "e.logical DESC,e.path",
        }
    }
    fn published_files(&self, q: &FileQuery, offset: i64, fetch: i64) -> Result<Vec<Entry>> {
        // Fresh bulk-loaded databases may not yet have planner statistics. Pick the
        // narrow domain index explicitly so a directory query cannot scan every file.
        let index = if q.parent.is_some() {
            "entries_parent_size"
        } else if (q.name.is_some() || !q.names.is_empty())
            && q.kind.as_deref() == Some("directory")
        {
            "entries_directory_name"
        } else {
            match q.sort.as_str() {
                "modified_at" => "entries_modified",
                "path" => "entries_path",
                _ => "entries_size",
            }
        };
        let mut sql = format!(
            "SELECT {COLUMNS} FROM roots r CROSS JOIN entries e INDEXED BY {index} ON e.scan_id=r.scan_id WHERE e.published=1"
        );
        if index == "entries_directory_name" {
            sql.push_str(" AND e.kind='directory'");
        }
        let mut values = vec![];
        Self::file_filters(q, &mut sql, &mut values);
        sql.push_str(&format!(
            " ORDER BY {} LIMIT ? OFFSET ?",
            Self::sort_clause(&q.sort)
        ));
        values.push(Value::Integer(fetch));
        values.push(Value::Integer(offset));
        let conn = self.reader()?;
        let mut s = conn.prepare(&sql).map_err(db)?;
        s.query_map(params_from_iter(values), entry_row)
            .map_err(db)?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(db)
    }
    /// A running scan has no secondary indexes yet: its rows are read by primary key, and a
    /// parent or subtree filter becomes a path range so only that part of the tree is touched.
    fn running_files(&self, q: &FileQuery, scan_id: &str, fetch: i64) -> Result<Vec<Entry>> {
        let mut sql =
            format!("SELECT {COLUMNS} FROM entries e WHERE e.scan_id=? AND e.published=0");
        let mut values = vec![Value::Text(scan_id.to_owned())];
        if let Some(scope) = q.parent.as_deref().or(q.path.as_deref()) {
            let (lower, upper) = subtree_bounds(scope);
            sql.push_str(" AND ((e.path>=? AND e.path<?) OR e.path=?)");
            values.extend([
                Value::Text(lower),
                Value::Text(upper),
                Value::Text(scope.to_owned()),
            ]);
        }
        Self::file_filters(q, &mut sql, &mut values);
        sql.push_str(&format!(" ORDER BY {} LIMIT ?", Self::sort_clause(&q.sort)));
        values.push(Value::Integer(fetch));
        let conn = self.reader()?;
        let mut s = conn.prepare(&sql).map_err(db)?;
        s.query_map(params_from_iter(values), entry_row)
            .map_err(db)?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(db)
    }
    pub fn entry(&self, path: &str) -> Result<Entry> {
        self.reader()?
            .query_row(
                &format!(
                    "SELECT {COLUMNS} FROM {} e WHERE e.path=?1",
                    self.entries_source()
                ),
                [path],
                entry_row,
            )
            .optional()
            .map_err(db)?
            .ok_or_else(|| Error::new("path_not_found", "Path is not in the published index"))
    }
    /// One SQLite read snapshot: immediate files and directories plus an exact bounded remainder.
    pub fn directory_breakdown(&self, path: &str, limit: u32) -> Result<DirectoryBreakdown> {
        if !(1..=200).contains(&limit) {
            return Err(Error::invalid("Breakdown limit must be 1..200"));
        }
        let mut conn = self.reader()?;
        let tx = conn.transaction().map_err(db)?;
        let found: Option<(String, bool)> = tx
            .query_row(
                &format!(
                    "SELECT scan_id,published FROM {} WHERE path=?1",
                    self.entries_source()
                ),
                [path],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
            .map_err(db)?;
        let (scan_id, published) =
            found.ok_or_else(|| Error::new("path_not_found", "Directory is not indexed"))?;
        let directory: Entry = tx
            .query_row(
                &format!("SELECT {COLUMNS} FROM entries e WHERE e.scan_id=?1 AND e.path=?2"),
                params![scan_id, path],
                entry_row,
            )
            .map_err(db)?;
        if directory.kind != EntryKind::Directory {
            return Err(Error::invalid("Breakdown requires an indexed directory"));
        }
        // A published generation reads its parent index; a running one reads the path range
        // below the directory by primary key.
        let (lower, upper) = subtree_bounds(path);
        let (count, logical, allocated): (i64, i64, i64) = if published {
            tx.query_row("SELECT count(*),coalesce(sum(logical),0),coalesce(sum(allocated),0) FROM entries INDEXED BY entries_parent_size WHERE scan_id=?1 AND published=1 AND parent=?2 AND path<>?2", params![scan_id,path], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).map_err(db)?
        } else {
            tx.query_row("SELECT count(*),coalesce(sum(logical),0),coalesce(sum(allocated),0) FROM entries WHERE scan_id=?1 AND path>=?3 AND path<?4 AND parent=?2", params![scan_id,path,lower,upper], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).map_err(db)?
        };
        let children: Vec<Entry> = {
            let sql = if published {
                format!(
                    "SELECT {COLUMNS} FROM entries e INDEXED BY entries_parent_size WHERE e.scan_id=?1 AND e.published=1 AND e.parent=?2 AND e.path<>?2 ORDER BY e.logical DESC,e.path LIMIT ?3"
                )
            } else {
                format!(
                    "SELECT {COLUMNS} FROM entries e WHERE e.scan_id=?1 AND e.path>=?4 AND e.path<?5 AND e.parent=?2 ORDER BY e.logical DESC,e.path LIMIT ?3"
                )
            };
            let mut query = tx.prepare(&sql).map_err(db)?;
            let rows = if published {
                query.query_map(params![scan_id, path, limit], entry_row)
            } else {
                query.query_map(params![scan_id, path, limit, lower, upper], entry_row)
            };
            rows.map_err(db)?
                .collect::<std::result::Result<Vec<_>, _>>()
                .map_err(db)?
        };
        tx.commit().map_err(db)?;
        Ok(DirectoryBreakdown {
            directory,
            child_count: count as u64,
            children_logical_bytes: logical as u64,
            children_allocated_bytes: allocated as u64,
            omitted_count: count as u64 - children.len() as u64,
            omitted_logical_bytes: (logical as u64)
                .saturating_sub(children.iter().map(|e| e.logical_bytes).sum()),
            omitted_allocated_bytes: (allocated as u64)
                .saturating_sub(children.iter().map(|e| e.allocated_bytes).sum()),
            children,
        })
    }
    pub fn categories(&self) -> Result<Vec<CategoryTotal>> {
        let conn = self.reader()?;
        let mut s=conn.prepare(&format!("SELECT category,sum(logical),sum(allocated),sum(files) FROM category_totals c JOIN {} r ON c.scan_id=r.scan_id GROUP BY category ORDER BY sum(logical) DESC,category",self.roots_source())).map_err(db)?;
        s.query_map([], |r| {
            Ok(CategoryTotal {
                category: r.get(0)?,
                logical_bytes: r.get::<_, i64>(1)? as u64,
                allocated_bytes: r.get::<_, i64>(2)? as u64,
                files: r.get::<_, i64>(3)? as u64,
            })
        })
        .map_err(db)?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(db)
    }
    pub fn history(&self, path: Option<&str>, since: i64) -> Result<Vec<HistoryPoint>> {
        let conn = self.reader()?;
        let mut s=conn.prepare("SELECT scan_id,path,timestamp,logical,allocated,coverage,rowid FROM history WHERE (?1 IS NULL OR path=?1) AND timestamp>=?2 ORDER BY timestamp DESC,rowid DESC LIMIT 10000").map_err(db)?;
        s.query_map(params![path, since], |r| {
            Ok(HistoryPoint {
                sequence: r.get(6)?,
                scan_id: r.get(0)?,
                path: r.get(1)?,
                timestamp: r.get(2)?,
                logical_bytes: r.get::<_, i64>(3)? as u64,
                allocated_bytes: r.get::<_, i64>(4)? as u64,
                coverage: r.get(5)?,
            })
        })
        .map_err(db)?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(db)
    }
    pub fn put<T: Serialize>(&self, kind: &str, id: &str, value: &T) -> Result<()> {
        self.writer()?.execute("INSERT INTO documents(kind,id,data) VALUES(?1,?2,?3) ON CONFLICT(kind,id) DO UPDATE SET data=excluded.data",params![kind,id,serde_json::to_string(value)?]).map_err(db)?;
        self.bump();
        Ok(())
    }
    pub fn get<T: DeserializeOwned>(&self, kind: &str, id: &str) -> Result<T> {
        let data: Option<String> = self
            .reader()?
            .query_row(
                "SELECT data FROM documents WHERE kind=?1 AND id=?2",
                params![kind, id],
                |r| r.get(0),
            )
            .optional()
            .map_err(db)?;
        Ok(serde_json::from_str(&data.ok_or_else(|| {
            Error::new("not_found", format!("{kind} {id} not found"))
        })?)?)
    }
    pub fn documents<T: DeserializeOwned>(&self, kind: &str) -> Result<Vec<T>> {
        let conn = self.reader()?;
        let mut s = conn
            .prepare("SELECT data FROM documents WHERE kind=?1 ORDER BY id LIMIT 1000")
            .map_err(db)?;
        s.query_map([kind], |r| r.get::<_, String>(0))
            .map_err(db)?
            .map(|v| Ok(serde_json::from_str(&v.map_err(db)?)?))
            .collect()
    }
    pub fn audit(&self, action: &str, id: &str, detail: &str) -> Result<()> {
        self.writer()?
            .execute(
                "INSERT INTO audit(timestamp,action,resource_id,detail) VALUES(?1,?2,?3,?4)",
                params![now(), action, id, detail],
            )
            .map_err(db)?;
        self.bump();
        Ok(())
    }
    pub fn audit_records(&self, limit: u32, offset: u64) -> Result<Vec<AuditRecord>> {
        let conn = self.reader()?;
        let mut s=conn.prepare("SELECT id,timestamp,action,resource_id,detail FROM audit ORDER BY id DESC LIMIT ?1 OFFSET ?2").map_err(db)?;
        s.query_map(
            params![limit.min(1000), offset.min(i64::MAX as u64) as i64],
            |r| {
                Ok(AuditRecord {
                    id: r.get(0)?,
                    timestamp: r.get(1)?,
                    action: r.get(2)?,
                    resource_id: r.get(3)?,
                    detail: r.get(4)?,
                })
            },
        )
        .map_err(db)?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(db)
    }
    /// Keyset cursor avoids offset scans over millions of duplicate candidates.
    pub fn duplicate_candidates(&self, after: &str) -> Result<Vec<Entry>> {
        let conn = self.reader()?;
        let mut s=conn.prepare(&format!("SELECT {COLUMNS} FROM entries e INDEXED BY entries_path JOIN roots r ON e.scan_id=r.scan_id WHERE e.published=1 AND e.kind='file' AND e.logical>0 AND e.path>?1 AND EXISTS(SELECT 1 FROM current_entries b WHERE b.kind='file' AND b.logical=e.logical AND (b.device<>e.device OR b.inode<>e.inode) LIMIT 1) ORDER BY e.path LIMIT 256")).map_err(db)?;
        s.query_map([after], entry_row)
            .map_err(db)?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(db)
    }
    pub fn fingerprint(
        &self,
        path: &str,
        identity: &Identity,
    ) -> Result<Option<(String, Option<String>)>> {
        self.reader()?
            .query_row(
                "SELECT sample,full_hash FROM fingerprints WHERE path=?1 AND identity=?2",
                params![path, serde_json::to_string(identity)?],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
            .map_err(db)
    }
    pub fn save_fingerprint(
        &self,
        path: &str,
        identity: &Identity,
        sample: &str,
        full: Option<&str>,
    ) -> Result<()> {
        self.writer()?.execute("INSERT INTO fingerprints(path,identity,sample,full_hash) VALUES(?1,?2,?3,?4) ON CONFLICT(path) DO UPDATE SET identity=excluded.identity,sample=excluded.sample,full_hash=excluded.full_hash",params![path,serde_json::to_string(identity)?,sample,full]).map_err(db)?;
        Ok(())
    }
    pub fn sample_matches(&self, sample: &str) -> Result<u64> {
        self.reader()?.query_row(&format!("SELECT count(*) FROM (SELECT 1 FROM fingerprints f JOIN {} e ON f.path=e.path WHERE sample=?1 LIMIT 2)",self.entries_source()),[sample],|r|Ok(r.get::<_,i64>(0)? as u64)).map_err(db)
    }
    pub fn save_fingerprints(&self, updates: &[FingerprintUpdate]) -> Result<()> {
        let mut conn = self.writer()?;
        let tx = conn.transaction().map_err(db)?;
        {
            let mut s=tx.prepare_cached("INSERT INTO fingerprints(path,identity,sample,full_hash) VALUES(?1,?2,?3,?4) ON CONFLICT(path) DO UPDATE SET identity=excluded.identity,sample=excluded.sample,full_hash=excluded.full_hash").map_err(db)?;
            for u in updates {
                s.execute(params![
                    u.path,
                    serde_json::to_string(&u.identity)?,
                    u.sample,
                    u.full
                ])
                .map_err(db)?;
            }
        }
        tx.commit().map_err(db)
    }
    pub fn save_duplicate_members(
        &self,
        operation_id: &str,
        updates: &[FingerprintUpdate],
    ) -> Result<()> {
        let mut conn = self.writer()?;
        let tx = conn.transaction().map_err(db)?;
        {
            let mut s=tx.prepare_cached("INSERT OR IGNORE INTO duplicate_members(operation_id,device,inode,path,size,hash) VALUES(?1,?2,?3,?4,?5,?6)").map_err(db)?;
            for u in updates {
                if let Some(hash) = &u.full {
                    s.execute(params![
                        operation_id,
                        u.identity.device as i64,
                        u.identity.inode as i64,
                        u.path,
                        u.identity.size as i64,
                        hash
                    ])
                    .map_err(db)?;
                }
            }
        }
        tx.commit().map_err(db)
    }
    pub fn duplicate_group_count(&self, operation_id: &str) -> Result<u64> {
        self.reader()?.query_row("SELECT count(*) FROM (SELECT hash,size FROM duplicate_members WHERE operation_id=?1 GROUP BY hash,size HAVING count(*)>1)",[operation_id],|r|Ok(r.get::<_,i64>(0)? as u64)).map_err(db)
    }
    pub fn publish_duplicates(&self, report: &DuplicateReport) -> Result<()> {
        let mut conn = self.writer()?;
        let tx = conn.transaction().map_err(db)?;
        tx.execute("INSERT INTO documents(kind,id,data) VALUES('duplicates','latest',?1) ON CONFLICT(kind,id) DO UPDATE SET data=excluded.data",[serde_json::to_string(report)?]).map_err(db)?;
        tx.execute(
            "DELETE FROM duplicate_members WHERE operation_id<>?1",
            [&report.operation_id],
        )
        .map_err(db)?;
        tx.commit().map_err(db)?;
        drop(conn);
        self.bump();
        Ok(())
    }
    pub fn duplicate_groups(
        &self,
        operation_id: &str,
        limit: u32,
        offset: u64,
    ) -> Result<Page<DuplicateGroup>> {
        if limit == 0 || limit > 1000 || offset > i64::MAX as u64 {
            return Err(Error::invalid("Invalid pagination"));
        }
        let conn = self.reader()?;
        let mut s=conn.prepare("SELECT hash,size,count(*) FROM duplicate_members WHERE operation_id=?1 GROUP BY hash,size HAVING count(*)>1 ORDER BY size*(count(*)-1) DESC,hash LIMIT ?2 OFFSET ?3").map_err(db)?;
        let summaries = s
            .query_map(
                params![operation_id, i64::from(limit) + 1, offset as i64],
                |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, i64>(1)? as u64,
                        r.get::<_, i64>(2)? as u64,
                    ))
                },
            )
            .map_err(db)?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(db)?;
        let has_more = summaries.len() > limit as usize;
        let mut items = Vec::new();
        for (hash, size, count) in summaries.into_iter().take(limit as usize) {
            let mut s=conn.prepare("SELECT path FROM duplicate_members WHERE operation_id=?1 AND hash=?2 AND size=?3 ORDER BY path LIMIT 1000").map_err(db)?;
            let files = s
                .query_map(params![operation_id, hash, size as i64], |r| {
                    r.get::<_, String>(0)
                })
                .map_err(db)?
                .collect::<std::result::Result<Vec<_>, _>>()
                .map_err(db)?;
            items.push(DuplicateGroup{file_count:count,id:format!("{size}-{hash}"),files,file_size:size,total_size:size.saturating_mul(count),reclaimable_size:size.saturating_mul(count-1),verification:"blake3_full_content_metadata_checked; excludes hard-link aliases; at most 1000 paths shown per group; physical savings depend on sparse/clone allocation".into(),hash});
        }
        Ok(Page {
            items,
            limit,
            offset,
            has_more,
        })
    }
    pub fn record_system(&self, s: &SystemSnapshot, days: u32) -> Result<()> {
        let conn = self.writer()?;
        conn.execute(
            "INSERT OR REPLACE INTO system_history(timestamp,cpu,memory,swap) VALUES(?1,?2,?3,?4)",
            params![
                s.timestamp,
                s.cpu_percent,
                s.used_memory as i64,
                s.used_swap as i64
            ],
        )
        .map_err(db)?;
        conn.execute(
            "DELETE FROM system_history WHERE timestamp<?1",
            [now() - i64::from(days) * 86400],
        )
        .map_err(db)?;
        Ok(())
    }

    /// Update one existing leaf or a new leaf with an indexed parent, including ancestor totals.
    /// Directory changes require reconciliation; all mutations here are one transaction.
    pub fn replace_leaf(
        &self,
        root: &str,
        path: &str,
        replacement: Option<&Entry>,
    ) -> Result<bool> {
        let mut conn = self.writer()?;
        let tx = conn.transaction().map_err(db)?;
        let scan_id: String = tx
            .query_row("SELECT scan_id FROM roots WHERE path=?1", [root], |r| {
                r.get(0)
            })
            .map_err(db)?;
        let old: Option<Entry> = tx
            .query_row(
                &format!("SELECT {COLUMNS} FROM entries e WHERE e.scan_id=?1 AND e.path=?2"),
                params![scan_id, path],
                entry_row,
            )
            .optional()
            .map_err(db)?;
        if old.as_ref().is_some_and(|e| e.kind == EntryKind::Directory)
            || replacement.is_some_and(|e| e.kind == EntryKind::Directory)
        {
            return Ok(false);
        }
        if old.is_none() && replacement.is_none() {
            return Ok(true);
        }
        let parent = replacement
            .or(old.as_ref())
            .expect("one entry exists")
            .parent
            .clone();
        let parent_exists: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM entries WHERE scan_id=?1 AND path=?2 AND kind='directory')",params![scan_id,parent],|r|r.get(0)).map_err(db)?;
        if !parent_exists {
            return Ok(false);
        }
        let delta_logical = replacement.map_or(0, |e| e.logical_bytes as i128)
            - old.as_ref().map_or(0, |e| e.logical_bytes as i128);
        let delta_allocated = replacement.map_or(0, |e| e.allocated_bytes as i128)
            - old.as_ref().map_or(0, |e| e.allocated_bytes as i128);
        let count_delta = i64::from(replacement.is_some()) - i64::from(old.is_some());
        for (entry, direction) in [(old.as_ref(), -1i64), (replacement, 1i64)] {
            if let Some(entry) = entry.filter(|e| e.kind == EntryKind::File) {
                tx.execute("INSERT INTO category_totals(scan_id,category,logical,allocated,files) VALUES(?1,?2,?3,?4,?5) ON CONFLICT(scan_id,category) DO UPDATE SET logical=logical+excluded.logical,allocated=allocated+excluded.allocated,files=files+excluded.files", params![scan_id,entry.category,entry.logical_bytes as i64 * direction,entry.allocated_bytes as i64 * direction,direction]).map_err(db)?;
            }
        }
        tx.execute(
            "DELETE FROM category_totals WHERE scan_id=?1 AND files=0",
            [&scan_id],
        )
        .map_err(db)?;
        tx.execute(
            "DELETE FROM entries WHERE scan_id=?1 AND path=?2",
            params![scan_id, path],
        )
        .map_err(db)?;
        if let Some(e) = replacement {
            let mut stmt = tx.prepare_cached(INSERT).map_err(db)?;
            insert_entry(&mut stmt, &scan_id, e, true)?;
        }
        for ancestor in Path::new(&parent)
            .ancestors()
            .take_while(|p| p.starts_with(root))
        {
            let path = ancestor
                .to_str()
                .ok_or_else(|| Error::invalid("Non UTF-8 ancestor"))?;
            let (logical, allocated): (i64, i64) = tx
                .query_row(
                    "SELECT logical,allocated FROM entries WHERE scan_id=?1 AND path=?2",
                    params![scan_id, path],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .map_err(db)?;
            let logical = i64::try_from(i128::from(logical) + delta_logical)
                .ok()
                .filter(|v| *v >= 0)
                .ok_or_else(|| Error::new("database_error", "Aggregate underflow"))?;
            let allocated = i64::try_from(i128::from(allocated) + delta_allocated)
                .ok()
                .filter(|v| *v >= 0)
                .ok_or_else(|| Error::new("database_error", "Aggregate underflow"))?;
            tx.execute(
                "UPDATE entries SET logical=?3,allocated=?4 WHERE scan_id=?1 AND path=?2",
                params![scan_id, path, logical, allocated],
            )
            .map_err(db)?;
        }
        tx.execute("DELETE FROM fingerprints WHERE path=?1", [path])
            .map_err(db)?;
        tx.execute("UPDATE scans SET logical=logical+?2,allocated=allocated+?3,entries=entries+?4 WHERE id=?1",params![scan_id,delta_logical as i64,delta_allocated as i64,count_delta]).map_err(db)?;
        tx.commit().map_err(db)?;
        drop(conn);
        self.bump();
        Ok(true)
    }
    pub fn snapshot_roots(&self, roots: &[String], retain_days: u32) -> Result<()> {
        let mut conn = self.writer()?;
        let tx = conn.transaction().map_err(db)?;
        let snapshot = id();
        for root in roots {
            tx.execute("INSERT INTO history SELECT ?1,e.path,?2,e.logical,e.allocated,s.status FROM roots r CROSS JOIN entries e INDEXED BY entries_size ON r.scan_id=e.scan_id JOIN scans s ON s.id=r.scan_id WHERE r.path=?3 AND e.published=1 AND e.kind='directory' AND e.depth<=1",params![snapshot,now(),root]).map_err(db)?;
        }
        tx.execute(
            "DELETE FROM history WHERE timestamp<?1",
            [now() - i64::from(retain_days) * 86400],
        )
        .map_err(db)?;
        tx.commit().map_err(db)?;
        drop(conn);
        self.bump();
        Ok(())
    }
    pub fn system_history(&self) -> Result<Vec<serde_json::Value>> {
        let conn = self.reader()?;
        let mut s=conn.prepare("SELECT timestamp,cpu,memory,swap FROM system_history ORDER BY timestamp DESC LIMIT 1000").map_err(db)?;
        s.query_map([],|r|Ok(serde_json::json!({"timestamp":r.get::<_,i64>(0)?,"cpu_percent":r.get::<_,f64>(1)?,"used_memory":r.get::<_,i64>(2)?,"used_swap":r.get::<_,i64>(3)?}))).map_err(db)?.collect::<std::result::Result<Vec<_>,_>>().map_err(db)
    }
}
