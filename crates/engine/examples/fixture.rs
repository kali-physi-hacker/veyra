//! Build a labeled synthetic fixture in a NEW directory; never overwrite existing data.
use std::{fs, io::Write, path::PathBuf};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(
        std::env::args()
            .nth(1)
            .ok_or("usage: fixture NEW_DIRECTORY")?,
    );
    fs::create_dir(&root)?;
    for dir in [
        "Projects/atlas/target/debug",
        "Projects/atlas/src",
        "Projects/web/node_modules/example",
        "Downloads",
        "Documents",
        "Pictures",
        "Applications/Example.app/Contents",
        "Library/Caches/local.example.app",
        ".npm/_cacache/content",
    ] {
        fs::create_dir_all(root.join(dir))?;
    }
    fs::write(
        root.join("README.txt"),
        "STRATUM SYNTHETIC TEST FIXTURE. Generated content only. Sparse files have large logical but small allocated sizes.\n",
    )?;
    fs::write(
        root.join("Projects/atlas/Cargo.toml"),
        "[package]\nname='atlas-fixture'\nversion='0.1.0'\n",
    )?;
    fs::write(
        root.join("Projects/atlas/src/main.rs"),
        "fn main() { println!(\"fixture\"); }\n",
    )?;
    for i in 0..24 {
        let mut file =
            fs::File::create(root.join(format!("Projects/atlas/target/debug/artifact-{i:02}")))?;
        file.write_all(&vec![i as u8; 65536])?;
        file.set_len((i + 1) * 1024 * 1024)?;
    }
    for (path, size, byte) in [
        ("Downloads/archive.zip", 12 * 1024 * 1024, 11),
        ("Downloads/archive-copy.zip", 12 * 1024 * 1024, 11),
        ("Documents/research.pdf", 1024 * 1024, 17),
        ("Pictures/landscape.jpg", 2 * 1024 * 1024, 19),
        (
            "Projects/web/node_modules/example/module.js",
            4 * 1024 * 1024,
            23,
        ),
        (
            "Library/Caches/local.example.app/cache",
            3 * 1024 * 1024,
            29,
        ),
        (".npm/_cacache/content/package", 1024 * 1024, 31),
    ] {
        fs::write(root.join(path), vec![byte; size])?;
    }
    fs::write(
        root.join("Applications/Example.app/Contents/Info.plist"),
        r#"<?xml version="1.0"?><plist version="1.0"><dict><key>CFBundleIdentifier</key><string>local.example.app</string><key>CFBundleName</key><string>Example</string></dict></plist>"#,
    )?;
    fs::write(
        root.join("Applications/Example.app/Contents/executable"),
        vec![42; 2 * 1024 * 1024],
    )?;
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink("Projects", root.join("projects-link"))?;
        fs::hard_link(
            root.join("Documents/research.pdf"),
            root.join("Documents/research-hardlink.pdf"),
        )?;
    }
    println!("{}", fs::canonicalize(root)?.display());
    Ok(())
}
