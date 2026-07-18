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
        4
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
fn version_two_backfills_only_published_categories() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("db");
    let connection = rusqlite::Connection::open(&path).unwrap();
    connection
        .execute_batch(include_str!("../../../migrations/001_initial.sql"))
        .unwrap();
    connection
        .execute_batch(include_str!("../../../migrations/002_query_paths.sql"))
        .unwrap();
    for (id, status, size) in [
        ("published", "completed", 1234),
        ("staged", "running", 9999),
    ] {
        connection
            .execute(
                "INSERT INTO scans(id,root,started,status) VALUES(?1,'/fixture',1,?2)",
                rusqlite::params![id, status],
            )
            .unwrap();
        connection.execute("INSERT INTO entries(scan_id,path,parent,name,kind,logical,allocated,extension,category,device,inode,depth,data) VALUES(?1,'/fixture/a','/fixture','a','file',?2,4096,'','documents',1,1,1,'{}')",rusqlite::params![id,size]).unwrap();
    }
    connection
        .execute(
            "INSERT INTO roots(path,scan_id) VALUES('/fixture','published')",
            [],
        )
        .unwrap();
    drop(connection);
    let store = Store::open(&path).unwrap();
    let categories = store.categories().unwrap();
    assert_eq!(categories.len(), 1);
    assert_eq!(categories[0].logical_bytes, 1234);
    assert_eq!(categories[0].files, 1);
    drop(store);
    let reopened = Store::open(&path).unwrap();
    assert_eq!(reopened.categories().unwrap()[0].logical_bytes, 1234);
}
