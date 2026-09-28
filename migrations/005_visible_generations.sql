-- Earlier releases stored every entry as JSON, which repeated the path three times, and kept
-- nine indexes over every generation. Converting a large index in place needs more free space
-- than the index itself and hours of rewriting, so entries from those releases are dropped
-- instead: the index is a rebuildable cache, and a rescan with this release takes minutes.
-- Scan records, directory history, plans, operations, audit records and policies are kept.
DELETE FROM entries;
DELETE FROM category_totals;
DELETE FROM roots;
UPDATE scans SET status='superseded' WHERE status IN ('completed','partial');
INSERT INTO audit(timestamp,action,resource_id,detail)
 SELECT strftime('%s','now'),'index_format_changed','index',
  'Entries indexed by an earlier release were dropped; rescan each location'
 WHERE EXISTS (SELECT 1 FROM scans WHERE status='superseded');
-- Entries are stored as columns; the JSON copy of a row is no longer written.
ALTER TABLE entries ADD COLUMN accessed INTEGER;
ALTER TABLE entries ADD COLUMN confidence REAL NOT NULL DEFAULT 0;
ALTER TABLE entries ADD COLUMN reason TEXT NOT NULL DEFAULT '';
ALTER TABLE entries ADD COLUMN size INTEGER NOT NULL DEFAULT 0;
ALTER TABLE entries ADD COLUMN modified_ns INTEGER NOT NULL DEFAULT 0;
ALTER TABLE entries ADD COLUMN changed_ns INTEGER NOT NULL DEFAULT 0;
ALTER TABLE entries ADD COLUMN links INTEGER NOT NULL DEFAULT 0;
ALTER TABLE entries ADD COLUMN published INTEGER NOT NULL DEFAULT 0;
-- Secondary indexes cover published rows only. A running scan touches nothing but the
-- primary key, and publication indexes its whole generation in one pass. The extension and
-- identity indexes served no query plan; allocated-byte order and category filters are rare
-- and read the size index instead. All four are gone.
DROP INDEX IF EXISTS entries_extension;
DROP INDEX IF EXISTS entries_identity;
DROP INDEX IF EXISTS entries_size;
DROP INDEX IF EXISTS entries_allocated;
DROP INDEX IF EXISTS entries_modified;
DROP INDEX IF EXISTS entries_category;
DROP INDEX IF EXISTS entries_path;
DROP INDEX IF EXISTS entries_directory_name;
DROP INDEX IF EXISTS entries_parent_size;
CREATE INDEX entries_size ON entries(scan_id,kind,logical DESC,path) WHERE published=1;
CREATE INDEX entries_modified ON entries(scan_id,modified) WHERE published=1;
CREATE INDEX entries_path ON entries(path,scan_id) WHERE published=1;
CREATE INDEX entries_directory_name ON entries(scan_id,name,logical DESC) WHERE kind='directory' AND published=1;
CREATE INDEX entries_parent_size ON entries(scan_id,parent,logical DESC,path,allocated) WHERE published=1;
-- A root's visible generation is its published scan, or the scan still running when nothing
-- has been published for that root yet. Readers that opt in see first scans as they progress;
-- a rescan keeps the published generation visible until it completes.
DROP VIEW IF EXISTS current_entries;
CREATE VIEW current_entries AS
 SELECT e.* FROM roots r CROSS JOIN entries e ON e.scan_id=r.scan_id WHERE e.published=1;
CREATE VIEW IF NOT EXISTS visible_roots AS
 SELECT path,scan_id FROM roots
 UNION ALL
 SELECT s.root AS path,s.id AS scan_id FROM scans s
 WHERE s.status='running' AND s.root NOT IN (SELECT path FROM roots);
CREATE VIEW IF NOT EXISTS visible_entries AS
 SELECT e.* FROM roots r CROSS JOIN entries e ON e.scan_id=r.scan_id WHERE e.published=1
 UNION ALL
 SELECT e.* FROM scans s JOIN entries e ON e.scan_id=s.id
 WHERE s.status='running' AND s.root NOT IN (SELECT path FROM roots) AND e.published=0;
PRAGMA user_version=5;
