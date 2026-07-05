//! SQLite persistence. Staged generations are published atomically; readers see the prior index during scans.
use rusqlite::{Connection, OptionalExtension, params, params_from_iter, types::Value};
use serde::{Serialize, de::DeserializeOwned};
use std::{
    path::Path,
    sync::{Mutex, MutexGuard},
    time::Duration,
};
use stratum_domain::*;

fn db(e: rusqlite::Error) -> Error {
    Error::new("database_error", e.to_string())
}
pub struct Store {
    connection: Mutex<Connection>,
}
pub struct FingerprintUpdate {
    pub path: String,
    pub identity: Identity,
    pub sample: String,
    pub full: Option<String>,
}
impl Store {
    pub fn open(path: &Path) -> Result<Self> {
        let connection = Connection::open(path).map_err(db)?;
        connection
            .busy_timeout(Duration::from_secs(5))
            .map_err(db)?;
        connection.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL; PRAGMA foreign_keys=ON; PRAGMA cache_size=-16384; PRAGMA temp_store=FILE;").map_err(db)?;
        let version: u32 = connection
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .map_err(db)?;
        if version > 2 {
            return Err(Error::new(
                "unsupported_schema",
                "Database was created by a newer Stratum release",
            ));
        }
        if version == 0 {
            connection
                .execute_batch(concat!(
                    "BEGIN IMMEDIATE;",
                    include_str!("../../../migrations/001_initial.sql"),
                    "COMMIT;"
                ))
                .map_err(db)?;
        }
        if version < 2 {
            connection
                .execute_batch(concat!(
                    "BEGIN IMMEDIATE;",
                    include_str!("../../../migrations/002_query_paths.sql"),
                    "COMMIT;"
                ))
                .map_err(db)?;
        }
        Ok(Self {
            connection: Mutex::new(connection),
        })
    }
    fn conn(&self) -> Result<MutexGuard<'_, Connection>> {
        self.connection
            .lock()
            .map_err(|_| Error::new("database_error", "Database mutex poisoned"))
    }
    pub fn recover_scans(&self) -> Result<()> {
        let conn = self.conn()?;
        conn.execute(
            "UPDATE scans SET status='interrupted',freshness='unknown' WHERE status='running'",
            [],
        )
        .map_err(db)?;
        conn.execute("DELETE FROM entries WHERE scan_id IN (SELECT id FROM scans WHERE status IN ('interrupted','failed','cancelled'))", []).map_err(db)?;
        Ok(())
    }
    pub fn begin_scan(&self, scan: &ScanRecord) -> Result<()> {
        self.conn()?
            .execute(
                "INSERT INTO scans(id,root,started,status) VALUES(?1,?2,?3,'running')",
                params![scan.id, scan.root, scan.started_at],
            )
            .map_err(db)?;
        self.audit("scan_started", &scan.id, &scan.root)
    }
    pub fn insert_batch(&self, scan_id: &str, entries: &[Entry]) -> Result<()> {
        let mut conn = self.conn()?;
        let tx = conn.transaction().map_err(db)?;
        {
            let mut stmt = tx.prepare_cached("INSERT OR REPLACE INTO entries(scan_id,path,parent,name,kind,logical,allocated,modified,created,extension,category,device,inode,depth,data) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15)").map_err(db)?;
            for e in entries {
                if e.logical_bytes > i64::MAX as u64 || e.allocated_bytes > i64::MAX as u64 {
                    return Err(Error::invalid(
                        "File size exceeds SQLite signed integer range",
                    ));
                }
                stmt.execute(params![
                    scan_id,
                    e.path,
                    e.parent,
                    e.name,
                    e.kind.as_str(),
                    e.logical_bytes as i64,
                    e.allocated_bytes as i64,
                    e.modified_at,
                    e.created_at,
                    e.extension,
                    e.category,
                    e.identity.device as i64,
                    e.identity.inode as i64,
                    e.depth,
                    serde_json::to_string(e)?
                ])
                .map_err(db)?;
            }
        }
        tx.commit().map_err(db)
    }
    pub fn warning(&self, scan_id: &str, path: &str, code: &str, message: &str) -> Result<()> {
        self.conn()?.execute("INSERT INTO warnings(scan_id,path,code,message) SELECT ?1,?2,?3,?4 WHERE (SELECT count(*) FROM warnings WHERE scan_id=?1)<10000", params![scan_id,path,code,message]).map_err(db)?;
        Ok(())
    }
    pub fn finish_scan(&self, scan: &ScanRecord, retain_days: u32) -> Result<()> {
        let mut conn = self.conn()?;
        let tx = conn.transaction().map_err(db)?;
        tx.execute("UPDATE scans SET completed=?2,status=?3,entries=?4,warnings=?5,excluded=?6,logical=?7,allocated=?8,freshness=?9 WHERE id=?1", params![scan.id,scan.completed_at,scan.status,scan.entries as i64,scan.warnings as i64,scan.excluded as i64,scan.logical_bytes as i64,scan.allocated_bytes as i64,scan.freshness]).map_err(db)?;
        if scan.status == "completed" || scan.status == "partial" {
            tx.execute("INSERT INTO roots(path,scan_id) VALUES(?1,?2) ON CONFLICT(path) DO UPDATE SET scan_id=excluded.scan_id", params![scan.root,scan.id]).map_err(db)?;
            tx.execute("INSERT INTO history SELECT scan_id,path,?2,logical,allocated,?3 FROM entries WHERE scan_id=?1 AND kind='directory' AND depth<=1", params![scan.id,scan.completed_at,scan.status]).map_err(db)?;
            tx.execute("DELETE FROM entries WHERE scan_id IN (SELECT id FROM scans WHERE root=?1 AND id<>?2)",params![scan.root,scan.id]).map_err(db)?;
            tx.execute(
                "DELETE FROM fingerprints WHERE path NOT IN (SELECT path FROM current_entries)",
                [],
            )
            .map_err(db)?;
        } else {
            tx.execute("DELETE FROM entries WHERE scan_id=?1", [&scan.id])
                .map_err(db)?;
        }
        tx.execute(
            "DELETE FROM history WHERE timestamp < ?1",
            [now() - i64::from(retain_days) * 86400],
        )
        .map_err(db)?;
        tx.commit().map_err(db)?;
        drop(conn);
        self.audit("scan_finished", &scan.id, &scan.status)
    }
    pub fn scans(&self) -> Result<Vec<ScanRecord>> {
        let conn = self.conn()?;
        let mut s = conn.prepare("SELECT id,root,started,completed,status,entries,warnings,excluded,logical,allocated,freshness FROM scans ORDER BY started DESC,id LIMIT 1000").map_err(db)?;
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
    pub fn roots(&self) -> Result<Vec<String>> {
        let conn = self.conn()?;
        let mut s = conn
            .prepare("SELECT path FROM roots ORDER BY path")
            .map_err(db)?;
        s.query_map([], |r| r.get(0))
            .map_err(db)?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(db)
    }
    pub fn scan_policy(&self, root: &str) -> Result<ScanRequest> {
        let text:String=self.conn()?.query_row("SELECT d.data FROM documents d JOIN roots r ON d.id=r.scan_id WHERE d.kind='scan_policy' AND r.path=?1",[root],|r|r.get(0)).map_err(db)?;
        Ok(serde_json::from_str(&text)?)
    }
    pub fn mark_stale(&self, root: &str) -> Result<()> {
        self.conn()?.execute("UPDATE scans SET freshness='stale' WHERE id IN (SELECT scan_id FROM roots WHERE path=?1)",[root]).map_err(db)?;
        Ok(())
    }
    pub fn warnings(&self, scan_id: &str) -> Result<Vec<Evidence>> {
        let conn = self.conn()?;
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
        let sort = match q.sort.as_str() {
            "logical_bytes" => "e.logical DESC,e.path",
            "allocated_bytes" => "e.allocated DESC,e.path",
            "modified_at" => "e.modified DESC,e.path",
            "path" => "e.path",
            _ => return Err(Error::invalid("Unsupported sort")),
        };
        // Fresh bulk-loaded databases may not yet have planner statistics. Pick the
        // narrow domain index explicitly so a directory query cannot scan every file.
        let index = if q.parent.is_some() {
            "entries_parent"
        } else if q.category.is_some() {
            "entries_category"
        } else {
            match q.sort.as_str() {
                "allocated_bytes" => "entries_allocated",
                "modified_at" => "entries_modified",
                "path" => "entries_path",
                _ => "entries_size",
            }
        };
        let mut sql = format!(
            "SELECT e.data FROM roots r CROSS JOIN entries e INDEXED BY {index} ON e.scan_id=r.scan_id WHERE 1=1"
        );
        let mut values = vec![];
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
        sql.push_str(&format!(" ORDER BY {sort} LIMIT ? OFFSET ?"));
        values.push(Value::Integer(i64::from(q.limit) + 1));
        values.push(Value::Integer(q.offset as i64));
        let conn = self.conn()?;
        let mut s = conn.prepare(&sql).map_err(db)?;
        let rows = s
            .query_map(params_from_iter(values), |r| r.get::<_, String>(0))
            .map_err(db)?;
        let mut items: Vec<Entry> = rows
            .map(|r| Ok(serde_json::from_str(&r.map_err(db)?)?))
            .collect::<Result<_>>()?;
        let has_more = items.len() > q.limit as usize;
        items.truncate(q.limit as usize);
        Ok(Page {
            items,
            limit: q.limit,
            offset: q.offset,
            has_more,
        })
    }
    pub fn entry(&self, path: &str) -> Result<Entry> {
        let text: Option<String> = self
            .conn()?
            .query_row(
                "SELECT data FROM current_entries WHERE path=?1",
                [path],
                |r| r.get(0),
            )
            .optional()
            .map_err(db)?;
        serde_json::from_str(
            &text.ok_or_else(|| {
                Error::new("path_not_found", "Path is not in the published index")
            })?,
        )
        .map_err(Into::into)
    }
    pub fn categories(&self) -> Result<Vec<CategoryTotal>> {
        let conn = self.conn()?;
        let mut s=conn.prepare("SELECT category,sum(logical),sum(allocated),count(*) FROM current_entries WHERE kind='file' GROUP BY category ORDER BY sum(logical) DESC").map_err(db)?;
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
        let conn = self.conn()?;
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
        self.conn()?.execute("INSERT INTO documents(kind,id,data) VALUES(?1,?2,?3) ON CONFLICT(kind,id) DO UPDATE SET data=excluded.data",params![kind,id,serde_json::to_string(value)?]).map_err(db)?;
        Ok(())
    }
    pub fn get<T: DeserializeOwned>(&self, kind: &str, id: &str) -> Result<T> {
        let data: Option<String> = self
            .conn()?
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
        let conn = self.conn()?;
        let mut s = conn
            .prepare("SELECT data FROM documents WHERE kind=?1 ORDER BY id LIMIT 1000")
            .map_err(db)?;
        s.query_map([kind], |r| r.get::<_, String>(0))
            .map_err(db)?
            .map(|v| Ok(serde_json::from_str(&v.map_err(db)?)?))
            .collect()
    }
    pub fn audit(&self, action: &str, id: &str, detail: &str) -> Result<()> {
        self.conn()?
            .execute(
                "INSERT INTO audit(timestamp,action,resource_id,detail) VALUES(?1,?2,?3,?4)",
                params![now(), action, id, detail],
            )
            .map_err(db)?;
        Ok(())
    }
    pub fn audit_records(&self, limit: u32, offset: u64) -> Result<Vec<AuditRecord>> {
        let conn = self.conn()?;
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
        let conn = self.conn()?;
        let mut s=conn.prepare("SELECT e.data FROM entries e INDEXED BY entries_path JOIN roots r ON e.scan_id=r.scan_id WHERE e.kind='file' AND e.logical>0 AND e.path>?1 AND EXISTS(SELECT 1 FROM current_entries b WHERE b.kind='file' AND b.logical=e.logical AND (b.device<>e.device OR b.inode<>e.inode) LIMIT 1) ORDER BY e.path LIMIT 256").map_err(db)?;
        s.query_map([after], |r| r.get::<_, String>(0))
            .map_err(db)?
            .map(|v| Ok(serde_json::from_str(&v.map_err(db)?)?))
            .collect()
    }
    pub fn fingerprint(
        &self,
        path: &str,
        identity: &Identity,
    ) -> Result<Option<(String, Option<String>)>> {
        self.conn()?
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
        self.conn()?.execute("INSERT INTO fingerprints(path,identity,sample,full_hash) VALUES(?1,?2,?3,?4) ON CONFLICT(path) DO UPDATE SET identity=excluded.identity,sample=excluded.sample,full_hash=excluded.full_hash",params![path,serde_json::to_string(identity)?,sample,full]).map_err(db)?;
        Ok(())
    }
    pub fn sample_matches(&self, sample: &str) -> Result<u64> {
        self.conn()?.query_row("SELECT count(*) FROM (SELECT 1 FROM fingerprints f JOIN current_entries e ON f.path=e.path WHERE sample=?1 LIMIT 2)",[sample],|r|Ok(r.get::<_,i64>(0)? as u64)).map_err(db)
    }
    pub fn save_fingerprints(&self, updates: &[FingerprintUpdate]) -> Result<()> {
        let mut conn = self.conn()?;
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
        let mut conn = self.conn()?;
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
        self.conn()?.query_row("SELECT count(*) FROM (SELECT hash,size FROM duplicate_members WHERE operation_id=?1 GROUP BY hash,size HAVING count(*)>1)",[operation_id],|r|Ok(r.get::<_,i64>(0)? as u64)).map_err(db)
    }
    pub fn publish_duplicates(&self, report: &DuplicateReport) -> Result<()> {
        let mut conn = self.conn()?;
        let tx = conn.transaction().map_err(db)?;
        tx.execute("INSERT INTO documents(kind,id,data) VALUES('duplicates','latest',?1) ON CONFLICT(kind,id) DO UPDATE SET data=excluded.data",[serde_json::to_string(report)?]).map_err(db)?;
        tx.execute(
            "DELETE FROM duplicate_members WHERE operation_id<>?1",
            [&report.operation_id],
        )
        .map_err(db)?;
        tx.commit().map_err(db)
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
        let conn = self.conn()?;
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
        let conn = self.conn()?;
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
        let mut conn = self.conn()?;
        let tx = conn.transaction().map_err(db)?;
        let scan_id: String = tx
            .query_row("SELECT scan_id FROM roots WHERE path=?1", [root], |r| {
                r.get(0)
            })
            .map_err(db)?;
        let old_json: Option<String> = tx
            .query_row(
                "SELECT data FROM entries WHERE scan_id=?1 AND path=?2",
                params![scan_id, path],
                |r| r.get(0),
            )
            .optional()
            .map_err(db)?;
        let old: Option<Entry> = old_json.as_deref().map(serde_json::from_str).transpose()?;
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
        tx.execute(
            "DELETE FROM entries WHERE scan_id=?1 AND path=?2",
            params![scan_id, path],
        )
        .map_err(db)?;
        if let Some(e) = replacement {
            tx.execute("INSERT INTO entries(scan_id,path,parent,name,kind,logical,allocated,modified,created,extension,category,device,inode,depth,data) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15)",params![scan_id,e.path,e.parent,e.name,e.kind.as_str(),e.logical_bytes as i64,e.allocated_bytes as i64,e.modified_at,e.created_at,e.extension,e.category,e.identity.device as i64,e.identity.inode as i64,e.depth,serde_json::to_string(e)?]).map_err(db)?;
        }
        for ancestor in Path::new(&parent)
            .ancestors()
            .take_while(|p| p.starts_with(root))
        {
            let path = ancestor
                .to_str()
                .ok_or_else(|| Error::invalid("Non UTF-8 ancestor"))?;
            let data: String = tx
                .query_row(
                    "SELECT data FROM entries WHERE scan_id=?1 AND path=?2",
                    params![scan_id, path],
                    |r| r.get(0),
                )
                .map_err(db)?;
            let mut entry: Entry = serde_json::from_str(&data)?;
            entry.logical_bytes = u64::try_from(entry.logical_bytes as i128 + delta_logical)
                .map_err(|_| Error::new("database_error", "Aggregate underflow"))?;
            entry.allocated_bytes = u64::try_from(entry.allocated_bytes as i128 + delta_allocated)
                .map_err(|_| Error::new("database_error", "Aggregate underflow"))?;
            tx.execute(
                "UPDATE entries SET logical=?3,allocated=?4,data=?5 WHERE scan_id=?1 AND path=?2",
                params![
                    scan_id,
                    path,
                    entry.logical_bytes as i64,
                    entry.allocated_bytes as i64,
                    serde_json::to_string(&entry)?
                ],
            )
            .map_err(db)?;
        }
        tx.execute("DELETE FROM fingerprints WHERE path=?1", [path])
            .map_err(db)?;
        tx.execute("UPDATE scans SET logical=logical+?2,allocated=allocated+?3,entries=entries+?4 WHERE id=?1",params![scan_id,delta_logical as i64,delta_allocated as i64,count_delta]).map_err(db)?;
        tx.commit().map_err(db)?;
        Ok(true)
    }
    pub fn snapshot_roots(&self, roots: &[String], retain_days: u32) -> Result<()> {
        let mut conn = self.conn()?;
        let tx = conn.transaction().map_err(db)?;
        let snapshot = id();
        for root in roots {
            tx.execute("INSERT INTO history SELECT ?1,e.path,?2,e.logical,e.allocated,s.status FROM roots r CROSS JOIN entries e INDEXED BY entries_size ON r.scan_id=e.scan_id JOIN scans s ON s.id=r.scan_id WHERE r.path=?3 AND e.kind='directory' AND e.depth<=1",params![snapshot,now(),root]).map_err(db)?;
        }
        tx.execute(
            "DELETE FROM history WHERE timestamp<?1",
            [now() - i64::from(retain_days) * 86400],
        )
        .map_err(db)?;
        tx.commit().map_err(db)
    }
    pub fn system_history(&self) -> Result<Vec<serde_json::Value>> {
        let conn = self.conn()?;
        let mut s=conn.prepare("SELECT timestamp,cpu,memory,swap FROM system_history ORDER BY timestamp DESC LIMIT 1000").map_err(db)?;
        s.query_map([],|r|Ok(serde_json::json!({"timestamp":r.get::<_,i64>(0)?,"cpu_percent":r.get::<_,f64>(1)?,"used_memory":r.get::<_,i64>(2)?,"used_swap":r.get::<_,i64>(3)?}))).map_err(db)?.collect::<std::result::Result<Vec<_>,_>>().map_err(db)
    }
}
