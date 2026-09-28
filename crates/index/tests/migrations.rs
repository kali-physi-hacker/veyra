use stratum_index::Store;
#[test]
fn version_one_upgrades_without_losing_existing_records() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("db");
    let connection = rusqlite::Connection::open(&path).unwrap();
    connection
        .execute_batch(include_str!("../../../migrations/001_initial.sql"))
        .unwrap();
    connection.execute("INSERT INTO audit(timestamp,action,resource_id,detail) VALUES(1,'before_upgrade','test','preserve')",[]).unwrap();
    drop(connection);
    let store = Store::open(&path).unwrap();
    assert_eq!(
        store.audit_records(10, 0).unwrap()[0].action,
        "before_upgrade"
    );
    drop(store);
    let connection = rusqlite::Connection::open(path).unwrap();
    assert_eq!(
        connection
            .query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        5
    );
}
#[test]
fn migration_is_idempotent_and_rejects_newer_schema() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("db");
    let store = Store::open(&path).unwrap();
    store.audit("test", "one", "evidence").unwrap();
    drop(store);
    let reopened = Store::open(&path).unwrap();
    assert_eq!(reopened.audit_records(10, 0).unwrap().len(), 1);
    drop(reopened);
    let conn = rusqlite::Connection::open(&path).unwrap();
    conn.pragma_update(None, "user_version", 999).unwrap();
    assert_eq!(Store::open(&path).err().unwrap().code, "unsupported_schema");
}

#[test]
fn upgrading_an_earlier_index_drops_its_entries_and_keeps_every_journal() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("db");
    let connection = rusqlite::Connection::open(&path).unwrap();
    for sql in [
        include_str!("../../../migrations/001_initial.sql"),
        include_str!("../../../migrations/002_query_paths.sql"),
        include_str!("../../../migrations/003_experience_queries.sql"),
        include_str!("../../../migrations/004_compact_query_indexes.sql"),
    ] {
        connection.execute_batch(sql).unwrap();
    }
    connection
        .execute(
            "INSERT INTO scans(id,root,started,completed,status,entries) VALUES('old','/fixture',1,2,'completed',1)",
            [],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO roots(path,scan_id) VALUES('/fixture','old')",
            [],
        )
        .unwrap();
    connection.execute("INSERT INTO entries(scan_id,path,parent,name,kind,logical,allocated,modified,created,extension,category,device,inode,depth,data) VALUES('old','/fixture/a.pdf','/fixture','a.pdf','file',1234,4096,5,6,'pdf','documents',9,10,1,'{}')",[]).unwrap();
    connection
        .execute(
            "INSERT INTO category_totals(scan_id,category,logical,allocated,files) VALUES('old','documents',1234,4096,1)",
            [],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO history(scan_id,path,timestamp,logical,allocated,coverage) VALUES('old','/fixture',2,1234,4096,'completed')",
            [],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO documents(kind,id,data) VALUES('scan_policy','old','{\"roots\":[\"/fixture\"]}')",
            [],
        )
        .unwrap();
    drop(connection);
    let store = Store::open(&path).unwrap();
    assert_eq!(
        store.entry("/fixture/a.pdf").unwrap_err().code,
        "path_not_found"
    );
    assert!(store.roots().unwrap().is_empty());
    assert!(store.categories().unwrap().is_empty());
    assert_eq!(store.scans().unwrap()[0].status, "superseded");
    assert_eq!(store.history(Some("/fixture"), 0).unwrap().len(), 1);
    assert_eq!(
        store
            .get::<stratum_domain::ScanRequest>("scan_policy", "old")
            .unwrap()
            .roots,
        vec!["/fixture".to_string()]
    );
    assert!(
        store
            .audit_records(10, 0)
            .unwrap()
            .iter()
            .any(|a| a.action == "index_format_changed")
    );
    drop(store);
    let connection = rusqlite::Connection::open(&path).unwrap();
    let indexes: Vec<String> = connection
        .prepare("SELECT name FROM sqlite_master WHERE type='index' AND tbl_name='entries'")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    for gone in [
        "entries_extension",
        "entries_identity",
        "entries_allocated",
        "entries_category",
    ] {
        assert!(!indexes.iter().any(|i| i == gone), "{gone} should be gone");
    }
    for kept in [
        "entries_size",
        "entries_parent_size",
        "entries_path",
        "entries_modified",
        "entries_directory_name",
    ] {
        assert!(indexes.iter().any(|i| i == kept), "{kept} should exist");
    }
    assert_eq!(
        connection
            .query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        5
    );
}
