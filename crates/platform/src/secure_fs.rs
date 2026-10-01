//! No-follow descriptor-relative file operations. No custom unsafe code.
use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::{Component, Path},
};
use stratum_domain::*;

#[cfg(unix)]
fn parent(path: &Path) -> Result<(std::os::fd::OwnedFd, std::ffi::OsString)> {
    use rustix::fs::{Mode, OFlags, open, openat};
    if !path.is_absolute() {
        return Err(Error::invalid("Expected absolute path"));
    }
    let mut parts = path.components().peekable();
    let mut fd = open(
        "/",
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .map_err(|e| Error::io(e.into()))?;
    while let Some(part) = parts.next() {
        match part {
            Component::RootDir => {}
            Component::Normal(name) => {
                if parts.peek().is_none() {
                    return Ok((fd, name.to_owned()));
                }
                fd = openat(
                    &fd,
                    name,
                    OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                    Mode::empty(),
                )
                .map_err(|e| Error::io(e.into()))?;
            }
            _ => return Err(Error::invalid("Traversal components are not accepted")),
        }
    }
    Err(Error::new("protected_path", "Filesystem root is protected"))
}

#[cfg(unix)]
pub fn open_regular(path: &Path) -> Result<File> {
    use rustix::fs::{Mode, OFlags, openat};
    let (fd, name) = parent(path)?;
    let f: File = openat(
        &fd,
        name,
        OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .map_err(|e| Error::io(e.into()))?
    .into();
    if !f.metadata()?.is_file() {
        return Err(Error::new(
            "unsupported_file_type",
            "Only regular files are supported",
        ));
    }
    Ok(f)
}
#[cfg(not(unix))]
pub fn open_regular(_path: &Path) -> Result<File> {
    Err(Error::new(
        "unsupported_platform_feature",
        "Secure no-follow file access requires Unix in release 0.1",
    ))
}

/// Atomic no-clobber rename, anchored to opened, no-follow parent directories.
#[cfg(any(target_os = "macos", target_os = "linux"))]
pub fn rename_no_replace(source: &Path, destination: &Path) -> Result<()> {
    use rustix::fs::{RenameFlags, renameat_with};
    let (sfd, source) = parent(source)?;
    let (dfd, destination) = parent(destination)?;
    renameat_with(&sfd, source, &dfd, destination, RenameFlags::NOREPLACE)
        .map_err(|e| Error::io(e.into()))?;
    rustix::fs::fsync(&sfd).map_err(|e| Error::io(e.into()))?;
    rustix::fs::fsync(&dfd).map_err(|e| Error::io(e.into()))?;
    Ok(())
}
#[cfg(not(any(target_os = "macos", target_os = "linux")))]
pub fn rename_no_replace(_source: &Path, _destination: &Path) -> Result<()> {
    Err(Error::new(
        "unsupported_platform_feature",
        "Quarantine requires atomic no-clobber rename support",
    ))
}

/// Permanently removes one regular file, anchored to its opened, no-follow parent directory,
/// and only while the name still refers to the object the caller verified: the same device,
/// inode, size and modification time, with a single link. The parent directory is synced so
/// the removal is durable before the caller records it. A same-user process that swaps the name
/// between this check and the unlink is outside what a local application can prevent.
#[cfg(unix)]
pub fn remove_regular(path: &Path, expected: &Identity) -> Result<()> {
    use rustix::fs::{AtFlags, Mode, OFlags, openat, unlinkat};
    let (fd, name) = parent(path)?;
    let file: File = openat(
        &fd,
        &name,
        OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .map_err(|e| Error::io(e.into()))?
    .into();
    let metadata = file.metadata()?;
    let current = crate::scanner::identity(&metadata);
    if !metadata.is_file()
        || current.device != expected.device
        || current.inode != expected.inode
        || current.size != expected.size
        || current.modified_ns != expected.modified_ns
        || current.links != 1
    {
        return Err(Error::new(
            "filesystem_changed",
            "The file changed after it was verified; nothing was deleted",
        ));
    }
    drop(file);
    unlinkat(&fd, &name, AtFlags::empty()).map_err(|e| Error::io(e.into()))?;
    rustix::fs::fsync(&fd).map_err(|e| Error::io(e.into()))?;
    Ok(())
}
#[cfg(not(unix))]
pub fn remove_regular(_path: &Path, _expected: &Identity) -> Result<()> {
    Err(Error::new(
        "unsupported_platform_feature",
        "Permanent deletion requires Unix in release 0.1",
    ))
}

/// What a directory tree holds, counted without following symlinks. Symlinks and other
/// non-directories count as files.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct TreeTotals {
    pub files: u64,
    pub directories: u64,
    pub logical_bytes: u64,
    pub allocated_bytes: u64,
}
/// Deeper trees are refused rather than followed; the scanner stops at the same depth.
const MAX_TREE_DEPTH: usize = 256;

#[cfg(unix)]
fn open_directory<Fd: std::os::fd::AsFd>(
    dir: Fd,
    name: &std::ffi::OsStr,
) -> Result<std::os::fd::OwnedFd> {
    use rustix::{
        fs::{Mode, OFlags, openat},
        io::Errno,
    };
    openat(
        dir,
        name,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .map_err(|e| match e {
        Errno::LOOP | Errno::NOTDIR => Error::new(
            "unsupported_file_type",
            "Expected a directory; symlinks are never followed",
        ),
        e => Error::io(e.into()),
    })
}

/// The names in one directory, read in full before anything in it changes.
#[cfg(unix)]
fn directory_names(dir: &std::os::fd::OwnedFd) -> Result<Vec<std::ffi::OsString>> {
    use std::os::unix::ffi::OsStrExt;
    let mut names = vec![];
    for entry in rustix::fs::Dir::read_from(dir).map_err(|e| Error::io(e.into()))? {
        let entry = entry.map_err(|e| Error::io(e.into()))?;
        let name = entry.file_name().to_bytes();
        if name != b"." && name != b".." {
            names.push(std::ffi::OsStr::from_bytes(name).to_owned());
        }
    }
    Ok(names)
}

/// The identity of the directory at `path`, opened through no-follow parents. A symlink or any
/// other kind of file is refused.
#[cfg(unix)]
pub fn directory_identity(path: &Path) -> Result<Identity> {
    let (fd, name) = parent(path)?;
    let dir = File::from(open_directory(&fd, &name)?);
    Ok(crate::scanner::identity(&dir.metadata()?))
}
#[cfg(not(unix))]
pub fn directory_identity(_path: &Path) -> Result<Identity> {
    Err(Error::new(
        "unsupported_platform_feature",
        "Folder cleanup requires Unix in release 0.1",
    ))
}

/// Counts a directory tree for review, anchored to opened, no-follow directories. It refuses the
/// tree at the first entry named in `protected` or living on another filesystem, naming it.
#[cfg(unix)]
pub fn survey_tree(path: &Path, protected: &[&str]) -> Result<(Identity, TreeTotals)> {
    let (fd, name) = parent(path)?;
    let root = open_directory(&fd, &name)?;
    let identity = crate::scanner::identity(&File::from(root.try_clone()?).metadata()?);
    let mut totals = TreeTotals::default();
    survey(&root, path, identity.device, protected, &mut totals, 0)?;
    Ok((identity, totals))
}
#[cfg(unix)]
fn survey(
    dir: &std::os::fd::OwnedFd,
    at: &Path,
    device: u64,
    protected: &[&str],
    totals: &mut TreeTotals,
    depth: usize,
) -> Result<()> {
    use rustix::fs::{AtFlags, FileType, statat};
    if depth >= MAX_TREE_DEPTH {
        return Err(Error::new(
            "invalid_cleanup_plan",
            format!(
                "{} is nested deeper than {MAX_TREE_DEPTH} folders",
                at.display()
            ),
        ));
    }
    for name in directory_names(dir)? {
        let path = at.join(&name);
        if protected.iter().any(|p| name == *p) {
            return Err(Error::new(
                "protected_path",
                format!("{} is protected by cleanup policy", path.display()),
            ));
        }
        let stat =
            statat(dir, &name, AtFlags::SYMLINK_NOFOLLOW).map_err(|e| Error::io(e.into()))?;
        if stat.st_dev as u64 != device {
            return Err(Error::new(
                "protected_path",
                format!("{} is on another filesystem", path.display()),
            ));
        }
        if FileType::from_raw_mode(stat.st_mode as _) == FileType::Directory {
            totals.directories += 1;
            let child = open_directory(dir, &name)?;
            survey(&child, &path, device, protected, totals, depth + 1)?;
        } else {
            totals.files += 1;
            totals.logical_bytes += stat.st_size as u64;
            totals.allocated_bytes += stat.st_blocks as u64 * 512;
        }
    }
    Ok(())
}
#[cfg(not(unix))]
pub fn survey_tree(_path: &Path, _protected: &[&str]) -> Result<(Identity, TreeTotals)> {
    Err(Error::new(
        "unsupported_platform_feature",
        "Folder cleanup requires Unix in release 0.1",
    ))
}

/// Permanently removes the directory tree at `path`, and only while its root is still the
/// directory the caller verified: the same device and inode. Every level is opened through
/// no-follow descriptors; symlinks are removed, never followed; nothing on another filesystem is
/// touched, and the removal stops there. A directory its owner cannot write is made writable
/// first, since the tree is going. After a failure, what remains can be removed by calling
/// again.
#[cfg(unix)]
pub fn remove_tree(path: &Path, expected: &Identity) -> Result<TreeTotals> {
    use rustix::fs::{AtFlags, fstat, unlinkat};
    let (fd, name) = parent(path)?;
    let root = open_directory(&fd, &name)?;
    let stat = fstat(&root).map_err(|e| Error::io(e.into()))?;
    if stat.st_dev as u64 != expected.device || stat.st_ino as u64 != expected.inode {
        return Err(Error::new(
            "filesystem_changed",
            "The folder changed after it was verified; nothing was deleted",
        ));
    }
    let mut totals = TreeTotals::default();
    empty_directory(&root, path, stat.st_dev as u64, &mut totals, 0)?;
    drop(root);
    unlinkat(&fd, &name, AtFlags::REMOVEDIR).map_err(|e| Error::io(e.into()))?;
    rustix::fs::fsync(&fd).map_err(|e| Error::io(e.into()))?;
    Ok(totals)
}
#[cfg(unix)]
fn empty_directory(
    dir: &std::os::fd::OwnedFd,
    at: &Path,
    device: u64,
    totals: &mut TreeTotals,
    depth: usize,
) -> Result<()> {
    use rustix::fs::{AtFlags, FileType, Mode, fchmod, fstat, statat, unlinkat};
    if depth >= MAX_TREE_DEPTH {
        return Err(Error::new(
            "invalid_cleanup_plan",
            format!(
                "{} is nested deeper than {MAX_TREE_DEPTH} folders",
                at.display()
            ),
        ));
    }
    let failed = |path: &Path, e: rustix::io::Errno| {
        let error = Error::io(e.into());
        Error::new(error.code, format!("{}: {}", path.display(), error.message))
    };
    let mode = fstat(dir).map_err(|e| failed(at, e))?.st_mode as u32;
    if mode & 0o300 != 0o300 {
        fchmod(dir, Mode::from_raw_mode(((mode & 0o7777) | 0o700) as _))
            .map_err(|e| failed(at, e))?;
    }
    for name in directory_names(dir)? {
        let path = at.join(&name);
        let stat = statat(dir, &name, AtFlags::SYMLINK_NOFOLLOW).map_err(|e| failed(&path, e))?;
        if FileType::from_raw_mode(stat.st_mode as _) == FileType::Directory {
            if stat.st_dev as u64 != device {
                return Err(Error::new(
                    "protected_path",
                    format!(
                        "{} is on another filesystem; it and its parents were left in place",
                        path.display()
                    ),
                ));
            }
            let child = open_directory(dir, &name)?;
            empty_directory(&child, &path, device, totals, depth + 1)?;
            drop(child);
            unlinkat(dir, &name, AtFlags::REMOVEDIR).map_err(|e| failed(&path, e))?;
            totals.directories += 1;
        } else {
            unlinkat(dir, &name, AtFlags::empty()).map_err(|e| failed(&path, e))?;
            totals.files += 1;
            totals.logical_bytes += stat.st_size as u64;
            totals.allocated_bytes += stat.st_blocks as u64 * 512;
        }
    }
    Ok(())
}
#[cfg(not(unix))]
pub fn remove_tree(_path: &Path, _expected: &Identity) -> Result<TreeTotals> {
    Err(Error::new(
        "unsupported_platform_feature",
        "Folder cleanup requires Unix in release 0.1",
    ))
}

pub fn hash_file(
    path: &Path,
    sample: bool,
    cancelled: impl Fn() -> bool,
) -> Result<(String, Identity)> {
    let mut f = open_regular(path)?;
    let before = crate::scanner::identity(&f.metadata()?);
    let mut hash = blake3::Hasher::new();
    let mut buffer = [0u8; 65536];
    if sample {
        for offset in [
            0,
            before.size.saturating_sub(65536) / 2,
            before.size.saturating_sub(65536),
        ] {
            if cancelled() {
                return Err(Error::new("scan_cancelled", "Hashing cancelled"));
            }
            f.seek(SeekFrom::Start(offset))?;
            let count = f.read(&mut buffer)?;
            hash.update(&buffer[..count]);
        }
    } else {
        loop {
            if cancelled() {
                return Err(Error::new("scan_cancelled", "Hashing cancelled"));
            }
            let count = f.read(&mut buffer)?;
            if count == 0 {
                break;
            }
            hash.update(&buffer[..count]);
        }
    }
    if crate::scanner::identity(&f.metadata()?) != before
        || crate::scanner::identity(&std::fs::symlink_metadata(path)?) != before
    {
        return Err(Error::new(
            "filesystem_changed",
            "File changed while hashing",
        ));
    }
    Ok((hash.finalize().to_hex().to_string(), before))
}
