CREATE TABLE category_totals (
 scan_id TEXT NOT NULL REFERENCES scans(id), category TEXT NOT NULL,
 logical INTEGER NOT NULL, allocated INTEGER NOT NULL, files INTEGER NOT NULL,
 PRIMARY KEY(scan_id,category)
) WITHOUT ROWID;
INSERT INTO category_totals
 SELECT scan_id,category,sum(logical),sum(allocated),count(*)
 FROM current_entries WHERE kind='file' GROUP BY scan_id,category;
PRAGMA user_version=3;
