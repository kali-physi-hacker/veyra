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
#[test]
fn directories_follow_their_descendants_and_totals_add_up() {
    let temp = tempfile::tempdir().unwrap();
    let root = fs::canonicalize(temp.path()).unwrap();
    let mut expected = 0u64;
    for a in 0..6 {
        for b in 0..4 {
            let dir = root.join(format!("a{a}/b{b}/c"));
            fs::create_dir_all(&dir).unwrap();
            for f in 0..3 {
                let size = (a * 100 + b * 10 + f + 1) as u64;
                fs::write(dir.join(format!("f{f}")), vec![0u8; size as usize]).unwrap();
                expected += size;
            }
        }
    }
    fs::write(root.join(".hidden"), "x").unwrap();
    let mut seen: Vec<String> = vec![];
    let (mut provisional, mut excluded) = (0u64, 0u64);
    stratum_platform::scanner::scan_with_threads(
        &root,
        &ScanRequest {
            include_hidden: false,
            ..Default::default()
        },
        &ScanControl::default(),
        4,
        |m| {
            match m {
                ScanMessage::Entry(e) => {
                    if e.kind == EntryKind::Directory {
                        let prefix = format!("{}/", e.path);
                        let descendants = seen.iter().filter(|p| p.starts_with(&prefix)).count();
                        assert!(
                            descendants > 0 || e.logical_bytes == 0,
                            "{} arrived before its children",
                            e.path
                        );
                        if e.depth == 0 {
                            assert_eq!(e.logical_bytes, expected);
                        }
                    }
                    seen.push(e.path);
                }
                ScanMessage::Provisional(e) => {
                    assert_eq!(e.kind, EntryKind::Directory);
                    provisional += 1;
                }
                ScanMessage::Excluded(n) => excluded += n,
                ScanMessage::Warning { .. } => panic!("unexpected warning"),
            }
            true
        },
    )
    .unwrap();
    assert_eq!(seen.last().unwrap(), root.to_str().unwrap());
    assert_eq!(seen.len(), 1 + 6 + 24 + 24 + 72);
    assert_eq!(excluded, 1);
    let _ = provisional;
}
#[test]
fn deep_nesting_is_reported_and_skipped() {
    let temp = tempfile::tempdir().unwrap();
    let mut deep = fs::canonicalize(temp.path()).unwrap();
    for _ in 0..260 {
        deep.push("d");
    }
    fs::create_dir_all(&deep).unwrap();
    let mut depth_warnings = 0;
    let mut deepest = 0;
    scan(
        temp.path(),
        &ScanRequest::default(),
        &ScanControl::default(),
        |m| {
            match m {
                ScanMessage::Warning { code, .. } if code == "depth_limit" => depth_warnings += 1,
                ScanMessage::Entry(e) => deepest = deepest.max(e.depth),
                _ => {}
            }
            true
        },
    )
    .unwrap();
    assert_eq!(depth_warnings, 1);
    assert!(deepest < 256);
}
#[cfg(unix)]
#[test]
fn verified_removal_deletes_only_the_object_it_was_shown() {
    let temp = tempfile::tempdir().unwrap();
    let root = fs::canonicalize(temp.path()).unwrap();
    let file = root.join("held");
    fs::write(&file, "quarantined bytes").unwrap();
    let (_, shown) = secure_fs::hash_file(&file, false, || false).unwrap();
    // Replaced by another object under the same name: refused, and the newcomer stays. The
    // sizes differ because a filesystem may hand the new file the old inode number and mtime.
    fs::remove_file(&file).unwrap();
    fs::write(&file, "another file").unwrap();
    assert_eq!(
        secure_fs::remove_regular(&file, &shown).unwrap_err().code,
        "filesystem_changed"
    );
    assert!(file.is_file());
    // A symlink in the leaf is never followed.
    let (_, current) = secure_fs::hash_file(&file, false, || false).unwrap();
    std::os::unix::fs::symlink(&file, root.join("alias")).unwrap();
    assert!(secure_fs::remove_regular(&root.join("alias"), &current).is_err());
    assert!(file.is_file());
    // A second hard link means the bytes live on elsewhere: refused.
    fs::hard_link(&file, root.join("twin")).unwrap();
    assert!(secure_fs::remove_regular(&file, &current).is_err());
    fs::remove_file(root.join("twin")).unwrap();
    secure_fs::remove_regular(&file, &current).unwrap();
    assert!(!file.exists());
    assert!(
        root.join("alias").symlink_metadata().is_ok(),
        "only the named file goes"
    );
}
#[cfg(unix)]
#[test]
fn trees_are_surveyed_and_removed_without_following_symlinks() {
    use std::os::unix::fs::{PermissionsExt, symlink};
    let temp = tempfile::tempdir().unwrap();
    let root = fs::canonicalize(temp.path()).unwrap();
    let outside = root.join("outside");
    fs::create_dir(&outside).unwrap();
    fs::write(outside.join("keep"), "outside bytes").unwrap();
    let tree = root.join("target");
    fs::create_dir_all(tree.join("debug/deps")).unwrap();
    fs::write(tree.join("debug/deps/a.rlib"), vec![1u8; 4096]).unwrap();
    fs::write(tree.join("debug/b"), "bb").unwrap();
    symlink(&outside, tree.join("debug/to-outside")).unwrap();
    symlink(outside.join("keep"), tree.join("file-link")).unwrap();
    fs::create_dir(tree.join("locked")).unwrap();
    fs::write(tree.join("locked/c"), "c").unwrap();
    fs::set_permissions(tree.join("locked"), fs::Permissions::from_mode(0o555)).unwrap();

    let (identity, totals) = secure_fs::survey_tree(&tree, &[".git"]).unwrap();
    assert_eq!(totals.directories, 3, "debug, debug/deps and locked");
    assert_eq!(totals.files, 5, "three files and two symlinks");
    assert!(
        totals.logical_bytes >= 4099,
        "three files: 4096, 2 and 1 bytes"
    );
    // A protected name anywhere inside refuses the whole tree.
    fs::create_dir(tree.join("debug/.git")).unwrap();
    assert_eq!(
        secure_fs::survey_tree(&tree, &[".git"]).unwrap_err().code,
        "protected_path"
    );
    fs::remove_dir(tree.join("debug/.git")).unwrap();
    // A symlink to the tree is neither surveyed nor removed.
    symlink(&tree, root.join("alias")).unwrap();
    assert!(secure_fs::survey_tree(&root.join("alias"), &[]).is_err());
    assert!(secure_fs::remove_tree(&root.join("alias"), &identity).is_err());
    // Another directory's identity is refused before anything goes.
    fs::create_dir(root.join("other")).unwrap();
    let other = secure_fs::directory_identity(&root.join("other")).unwrap();
    assert_eq!(
        secure_fs::remove_tree(&tree, &other).unwrap_err().code,
        "filesystem_changed"
    );
    assert!(tree.join("debug/b").exists());

    let removed = secure_fs::remove_tree(&tree, &identity).unwrap();
    assert_eq!((removed.files, removed.directories), (5, 3));
    assert!(!tree.exists());
    assert_eq!(
        fs::read_to_string(outside.join("keep")).unwrap(),
        "outside bytes",
        "symlink targets survive"
    );
    assert!(root.join("alias").symlink_metadata().is_ok());
}
