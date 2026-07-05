CREATE TABLE IF NOT EXISTS scans (
 id TEXT PRIMARY KEY, root TEXT NOT NULL, started INTEGER NOT NULL,
 completed INTEGER, status TEXT NOT NULL, entries INTEGER NOT NULL DEFAULT 0,
 warnings INTEGER NOT NULL DEFAULT 0, excluded INTEGER NOT NULL DEFAULT 0,
 logical INTEGER NOT NULL DEFAULT 0, allocated INTEGER NOT NULL DEFAULT 0,
 freshness TEXT NOT NULL DEFAULT 'unknown'
);
CREATE TABLE IF NOT EXISTS roots (path TEXT PRIMARY KEY, scan_id TEXT NOT NULL REFERENCES scans(id));
CREATE TABLE IF NOT EXISTS entries (
 scan_id TEXT NOT NULL REFERENCES scans(id), path TEXT NOT NULL, parent TEXT NOT NULL,
 name TEXT NOT NULL, kind TEXT NOT NULL, logical INTEGER NOT NULL, allocated INTEGER NOT NULL,
 modified INTEGER, created INTEGER, extension TEXT NOT NULL, category TEXT NOT NULL,
 device INTEGER NOT NULL, inode INTEGER NOT NULL, depth INTEGER NOT NULL, data TEXT NOT NULL,
 PRIMARY KEY(scan_id,path)
) WITHOUT ROWID;
CREATE INDEX IF NOT EXISTS entries_size ON entries(scan_id,kind,logical DESC,path);
CREATE INDEX IF NOT EXISTS entries_allocated ON entries(scan_id,kind,allocated DESC,path);
CREATE INDEX IF NOT EXISTS entries_parent ON entries(scan_id,parent);
CREATE INDEX IF NOT EXISTS entries_category ON entries(scan_id,category,kind);
CREATE INDEX IF NOT EXISTS entries_extension ON entries(scan_id,extension);
CREATE INDEX IF NOT EXISTS entries_modified ON entries(scan_id,modified);
CREATE INDEX IF NOT EXISTS entries_identity ON entries(scan_id,device,inode);
CREATE VIEW IF NOT EXISTS current_entries AS SELECT e.* FROM entries e JOIN roots r ON e.scan_id=r.scan_id;
CREATE TABLE IF NOT EXISTS warnings (id INTEGER PRIMARY KEY, scan_id TEXT NOT NULL, path TEXT NOT NULL, code TEXT NOT NULL, message TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS history (scan_id TEXT NOT NULL, path TEXT NOT NULL, timestamp INTEGER NOT NULL, logical INTEGER NOT NULL, allocated INTEGER NOT NULL, coverage TEXT NOT NULL, PRIMARY KEY(scan_id,path));
CREATE INDEX IF NOT EXISTS history_path_time ON history(path,timestamp);
CREATE TABLE IF NOT EXISTS documents (kind TEXT NOT NULL, id TEXT NOT NULL, data TEXT NOT NULL, PRIMARY KEY(kind,id)) WITHOUT ROWID;
CREATE TABLE IF NOT EXISTS fingerprints (path TEXT PRIMARY KEY, identity TEXT NOT NULL, sample TEXT NOT NULL, full_hash TEXT);
CREATE INDEX IF NOT EXISTS fingerprints_sample ON fingerprints(sample);
CREATE INDEX IF NOT EXISTS fingerprints_full ON fingerprints(full_hash);
CREATE TABLE IF NOT EXISTS duplicate_members (operation_id TEXT NOT NULL, device INTEGER NOT NULL, inode INTEGER NOT NULL, path TEXT NOT NULL, size INTEGER NOT NULL, hash TEXT NOT NULL, PRIMARY KEY(operation_id,device,inode)) WITHOUT ROWID;
CREATE INDEX IF NOT EXISTS duplicate_members_hash ON duplicate_members(operation_id,hash,size,path);
CREATE TABLE IF NOT EXISTS audit (id INTEGER PRIMARY KEY, timestamp INTEGER NOT NULL, action TEXT NOT NULL, resource_id TEXT NOT NULL, detail TEXT NOT NULL);
CREATE INDEX IF NOT EXISTS audit_time ON audit(timestamp);
CREATE TABLE IF NOT EXISTS system_history (timestamp INTEGER PRIMARY KEY, cpu REAL NOT NULL, memory INTEGER NOT NULL, swap INTEGER NOT NULL);
PRAGMA user_version=1;
