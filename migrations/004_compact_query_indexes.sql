-- Supersede the experimental all-entry name index; only directory rules need it.
DROP INDEX IF EXISTS entries_name;
CREATE INDEX entries_directory_name ON entries(scan_id,name,logical DESC) WHERE kind='directory';
CREATE INDEX IF NOT EXISTS entries_parent_size ON entries(scan_id,parent,logical DESC,path,allocated);
-- The new parent index retains the old prefix and replaces rather than duplicates it.
DROP INDEX IF EXISTS entries_parent;
PRAGMA user_version=4;
