CREATE INDEX IF NOT EXISTS entries_path ON entries(path,scan_id);
DROP VIEW current_entries;
CREATE VIEW current_entries AS SELECT e.* FROM roots r CROSS JOIN entries e ON e.scan_id=r.scan_id;
PRAGMA user_version=2;
