use std::{fs, sync::atomic::Ordering};
use stratum_domain::*;
use stratum_platform::{
    scanner::{ScanControl, ScanMessage, scan},
    secure_fs,
};
#[test]
fn traversal_sizes_links_and_exclusions() {
    let temp = tempfile::tempdir().unwrap();
    let root = fs::canonicalize(temp.path()).unwrap();
    fs::create_dir(root.join("nested")).unwrap();
    fs::write(root.join("nested/a.txt"), "hello").unwrap();
    fs::write(root.join("skip"), "exclude").unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(&root, root.join("cycle")).unwrap();
    let mut entries = vec![];
    let req = ScanRequest {
        exclusions: vec![root.join("skip").to_string_lossy().into()],
        ..Default::default()
    };
    scan(&root, &req, &ScanControl::default(), |m| {
        if let ScanMessage::Entry(e) = m {
            entries.push(*e);
        }
        true
    })
    .unwrap();
    assert_eq!(entries.last().unwrap().logical_bytes, 5);
    assert!(!entries.iter().any(|e| e.name == "skip"));
    assert!(entries.iter().any(|e| e.category == "documents"));
    #[cfg(unix)]
    assert_eq!(
        entries.iter().find(|e| e.name == "cycle").unwrap().kind,
        EntryKind::Symlink
    );
}
#[test]
fn cancelled_and_paused_scan_are_interruptible() {
    let temp = tempfile::tempdir().unwrap();
    let ctl = ScanControl::default();
    ctl.paused.store(true, Ordering::Relaxed);
    ctl.cancel();
    let e = scan(temp.path(), &ScanRequest::default(), &ctl, |_| true).unwrap_err();
    assert_eq!(e.code, "scan_cancelled");
}
#[test]
fn missing_root_is_typed() {
    let temp = tempfile::tempdir().unwrap();
    let e = scan(
        &temp.path().join("missing"),
        &ScanRequest::default(),
        &ScanControl::default(),
        |_| true,
    )
    .unwrap_err();
    assert_eq!(e.code, "path_not_found");
}
#[test]
fn no_clobber_rename_preserves_both_files() {
    let temp = tempfile::tempdir().unwrap();
    let root = fs::canonicalize(temp.path()).unwrap();
    let a = root.join("a");
    let b = root.join("b");
    fs::write(&a, "first").unwrap();
    fs::write(&b, "second").unwrap();
    assert!(secure_fs::rename_no_replace(&a, &b).is_err());
    assert_eq!(fs::read_to_string(a).unwrap(), "first");
    assert_eq!(fs::read_to_string(b).unwrap(), "second");
}
#[cfg(unix)]
#[test]
fn secure_access_rejects_symlink_parents_and_leaf() {
    let temp = tempfile::tempdir().unwrap();
    let root = fs::canonicalize(temp.path()).unwrap();
    fs::create_dir(root.join("real")).unwrap();
    fs::write(root.join("real/file"), "sensitive").unwrap();
    std::os::unix::fs::symlink(root.join("real"), root.join("alias")).unwrap();
    std::os::unix::fs::symlink(root.join("real/file"), root.join("leaf")).unwrap();
    assert!(secure_fs::open_regular(&root.join("alias/file")).is_err());
    assert!(secure_fs::open_regular(&root.join("leaf")).is_err());
}
#[cfg(unix)]
#[test]
fn permission_errors_surface_without_claiming_full_coverage() {
    use std::os::unix::fs::PermissionsExt;
    let temp = tempfile::tempdir().unwrap();
    let restricted = temp.path().join("restricted");
    fs::create_dir(&restricted).unwrap();
    fs::write(restricted.join("file"), "data").unwrap();
    fs::set_permissions(&restricted, fs::Permissions::from_mode(0o0)).unwrap();
    let mut warnings = 0;
    scan(
        temp.path(),
        &ScanRequest::default(),
        &ScanControl::default(),
        |m| {
            if matches!(m, ScanMessage::Warning { .. }) {
                warnings += 1;
            }
            true
        },
    )
    .unwrap();
    fs::set_permissions(&restricted, fs::Permissions::from_mode(0o700)).unwrap();
    assert!(warnings > 0, "Test requires a non-root user");
}
#[cfg(unix)]
#[test]
fn sparse_file_logical_and_allocated_are_distinct() {
    let temp = tempfile::tempdir().unwrap();
    let file = fs::File::create(temp.path().join("sparse")).unwrap();
    file.set_len(64 * 1024 * 1024).unwrap();
    let e = stratum_platform::scanner::read_entry(&temp.path().join("sparse"), 0).unwrap();
    assert_eq!(e.logical_bytes, 64 * 1024 * 1024);
    assert!(e.allocated_bytes < e.logical_bytes);
}
#[cfg(unix)]
#[test]
fn non_utf8_names_are_reported_not_lossily_indexed() {
    use std::os::unix::ffi::OsStringExt;
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join(std::ffi::OsString::from_vec(vec![0xff]));
    assert_eq!(
        stratum_platform::scanner::read_entry(&path, 0)
            .unwrap_err()
            .code,
        "unsupported_path_encoding"
    );
    if cfg!(target_os = "macos") {
        return;
    } // APFS rejects invalid UTF-8 names at creation.
    fs::write(&path, "x").unwrap();
    let mut found = false;
    scan(
        temp.path(),
        &ScanRequest::default(),
        &ScanControl::default(),
        |m| {
            if let ScanMessage::Warning { code, .. } = m {
                found |= code == "unsupported_path_encoding";
            }
            true
        },
    )
    .unwrap();
    assert!(found);
}
