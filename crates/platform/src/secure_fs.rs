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
