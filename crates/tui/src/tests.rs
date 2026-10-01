//! Headless rendering tests: every page draws into a test backend against a
//! labeled synthetic fixture, and the cleanup flow proves that nothing moves
//! without the exact approval phrase.
use crate::{
    app::{App, Overlay, Page},
    theme::Theme,
    ui,
};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{Terminal, backend::TestBackend};
use std::{
    io::Write,
    sync::Arc,
    time::{Duration, Instant},
};
use stratum_engine::{Engine, domain::Config};

struct Harness {
    _temp: tempfile::TempDir,
    root: std::path::PathBuf,
    engine: Arc<Engine>,
}
fn open_engine(temp: &tempfile::TempDir) -> Arc<Engine> {
    Engine::open(Config {
        data_dir: temp.path().join("state"),
        ..Default::default()
    })
    .unwrap()
}
/// A small labeled fixture: Cargo artifacts, duplicate archives, a document
/// and a valid application bundle. Nothing outside the temporary directory.
fn fixture() -> Harness {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("fixture");
    for dir in [
        "Projects/atlas/target/debug",
        "Projects/atlas/src",
        "Downloads",
        "Documents",
        "Applications/Example.app/Contents",
    ] {
        std::fs::create_dir_all(root.join(dir)).unwrap();
    }
    std::fs::write(
        root.join("Projects/atlas/Cargo.toml"),
        "[package]\nname='atlas-fixture'\nversion='0.1.0'\n",
    )
    .unwrap();
    std::fs::write(
        root.join("Projects/atlas/src/main.rs"),
        "fn main() { println!(\"fixture\"); }\n",
    )
    .unwrap();
    for i in 0..4u8 {
        let mut file = std::fs::File::create(
            root.join(format!("Projects/atlas/target/debug/artifact-{i:02}")),
        )
        .unwrap();
        file.write_all(&vec![i; 2 * 1024 * 1024]).unwrap();
    }
    std::fs::write(
        root.join("Downloads/archive.zip"),
        vec![7u8; 2 * 1024 * 1024],
    )
    .unwrap();
    std::fs::write(
        root.join("Downloads/archive-copy.zip"),
        vec![7u8; 2 * 1024 * 1024],
    )
    .unwrap();
    std::fs::write(root.join("Documents/research.pdf"), vec![9u8; 512 * 1024]).unwrap();
    std::fs::write(
        root.join("Applications/Example.app/Contents/Info.plist"),
        r#"<?xml version="1.0"?><plist version="1.0"><dict><key>CFBundleIdentifier</key><string>local.example.app</string><key>CFBundleName</key><string>Example</string></dict></plist>"#,
    )
    .unwrap();
    std::fs::write(
        root.join("Applications/Example.app/Contents/executable"),
        vec![42u8; 256 * 1024],
    )
    .unwrap();
    let engine = open_engine(&temp);
    engine.scan_location(root.to_str().unwrap()).unwrap();
    let root = std::fs::canonicalize(root).unwrap();
    Harness {
        _temp: temp,
        root,
        engine,
    }
}
fn settle(app: &mut App) {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        app.receive();
        if !app.busy && !app.loading() {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "background work did not complete: {}",
            app.status
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}
fn screen(app: &mut App, width: u16, height: u16) -> String {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal.draw(|frame| ui::render(frame, app)).unwrap();
    let buffer = terminal.backend().buffer().clone();
    let mut text = String::new();
    for y in 0..height {
        for x in 0..width {
            text.push_str(buffer.cell((x, y)).map_or(" ", |c| c.symbol()));
        }
        text.push('\n');
    }
    text
}
fn key(app: &mut App, code: KeyCode) {
    app.handle_key(KeyEvent::new(code, KeyModifiers::NONE));
}
fn press(app: &mut App, c: char) {
    key(app, KeyCode::Char(c));
}
fn go(app: &mut App, page: Page) {
    app.choose_page(page);
    settle(app);
}

#[test]
fn empty_workspace_is_guided_and_never_scans_on_its_own() {
    let temp = tempfile::tempdir().unwrap();
    let engine = open_engine(&temp);
    let mut app = App::new(engine.clone(), Theme::truecolor());
    settle(&mut app);
    let text = screen(&mut app, 120, 36);
    assert!(text.contains("Meet your storage."), "{text}");
    assert!(text.contains("Choose a folder to scan"), "{text}");
    assert!(text.contains("STRATUM"), "{text}");
    assert!(text.contains("WORKSPACE"), "{text}");
    assert!(engine.scans().unwrap().is_empty());
    press(&mut app, 's');
    assert_eq!(app.overlay, Overlay::Scan);
    let text = screen(&mut app, 120, 36);
    assert!(text.contains("Scan a location"), "{text}");
    key(&mut app, KeyCode::Esc);
    assert_eq!(app.overlay, Overlay::None);
    assert!(engine.scans().unwrap().is_empty());
    press(&mut app, '?');
    let text = screen(&mut app, 120, 36);
    assert!(text.contains("Keyboard"), "{text}");
    press(&mut app, 'j');
    assert_eq!(app.overlay, Overlay::None);
}

#[test]
fn every_page_renders_fixture_data_at_wide_and_compact_widths() {
    let harness = fixture();
    let mut app = App::new(harness.engine.clone(), Theme::truecolor());
    settle(&mut app);
    let expectations: [(Page, &[&str]); 9] = [
        (
            Page::Overview,
            &[
                "Capacity",
                "Indexed storage",
                "Indexed locations",
                "Rust build artifacts",
            ],
        ),
        (
            Page::Insights,
            &["Findings", "Why this finding?", "Rust build artifacts"],
        ),
        (Page::Storage, &["Contents", "Projects/", "Inspector"]),
        (Page::Files, &["Entries", "NAME", "artifact-03"]),
        (
            Page::Apps,
            &["Example", "local.example.app", "Estimated footprint"],
        ),
        (Page::Duplicates, &["Verify before deciding"]),
        (Page::System, &["CPU", "Memory", "Processes"]),
        (Page::Cleanup, &["Choose files", "Cleanup folders", "Cargo build output"]),
        (Page::Audit, &["Timeline", "scan"]),
    ];
    for (page, needles) in expectations {
        go(&mut app, page);
        for (width, height) in [(140, 42), (80, 24)] {
            let text = screen(&mut app, width, height);
            assert!(
                text.contains(page.title()),
                "{page:?} {width}x{height}\n{text}"
            );
            for needle in needles {
                assert!(
                    text.contains(needle),
                    "{page:?} {width}x{height} missing {needle:?}\n{text}"
                );
            }
            if width >= 100 {
                assert!(text.contains("WORKSPACE"), "sidebar expected\n{text}");
            } else {
                assert!(!text.contains("WORKSPACE"), "tab strip expected\n{text}");
            }
        }
    }
    let tiny = screen(&mut app, 30, 8);
    assert!(tiny.contains("at least 40"), "{tiny}");
}

#[test]
fn storage_browser_opens_folders_and_stops_at_the_indexed_root() {
    let harness = fixture();
    let mut app = App::new(harness.engine.clone(), Theme::truecolor());
    settle(&mut app);
    go(&mut app, Page::Storage);
    assert_eq!(app.path, harness.root.display().to_string());
    let text = screen(&mut app, 140, 40);
    assert!(text.contains("Projects/"), "{text}");
    key(&mut app, KeyCode::Enter);
    settle(&mut app);
    assert!(app.path.ends_with("Projects"), "{}", app.path);
    let text = screen(&mut app, 140, 40);
    assert!(text.contains("atlas/"), "{text}");
    assert!(text.contains("▸ Projects"), "{text}");
    key(&mut app, KeyCode::Backspace);
    settle(&mut app);
    assert_eq!(app.path, harness.root.display().to_string());
    key(&mut app, KeyCode::Backspace);
    settle(&mut app);
    assert_eq!(app.path, harness.root.display().to_string());
    assert!(app.toast.is_some());
    press(&mut app, 'L');
    assert_eq!(app.overlay, Overlay::Roots);
    let text = screen(&mut app, 140, 40);
    assert!(text.contains("Indexed locations"), "{text}");
    key(&mut app, KeyCode::Enter);
    assert_eq!(app.overlay, Overlay::None);
}

#[test]
fn files_filter_and_mode_switch_requery_the_index() {
    let harness = fixture();
    let mut app = App::new(harness.engine.clone(), Theme::truecolor());
    settle(&mut app);
    go(&mut app, Page::Files);
    assert!(app.files.iter().any(|f| f.name == "archive.zip"));
    press(&mut app, '/');
    for c in "*.zip".chars() {
        press(&mut app, c);
    }
    let text = screen(&mut app, 120, 30);
    assert!(text.contains("*.zip"), "{text}");
    key(&mut app, KeyCode::Enter);
    settle(&mut app);
    assert!(!app.files.is_empty());
    assert!(
        app.files.iter().all(|f| f.name.ends_with(".zip")),
        "{:?}",
        app.files.iter().map(|f| &f.name).collect::<Vec<_>>()
    );
    press(&mut app, 'm');
    settle(&mut app);
    let text = screen(&mut app, 120, 30);
    assert!(text.contains("Largest folders"), "{text}");
}

#[test]
fn cleanup_requires_the_exact_phrase_then_quarantines_and_restores() {
    let harness = fixture();
    let mut app = App::new(harness.engine.clone(), Theme::truecolor());
    settle(&mut app);
    go(&mut app, Page::Cleanup);
    // The page opens on the folders the rules recognise, found in the index without paging.
    assert_eq!(app.locations.len(), 1, "{:?}", app.locations);
    assert!(app.locations[0].path.ends_with("Projects/atlas/target"), "{:?}", app.locations);
    assert!(app.candidates.is_empty());
    key(&mut app, KeyCode::Enter);
    settle(&mut app);
    assert!(
        !app.candidates.is_empty(),
        "the opened folder must list its Cargo artifacts"
    );
    let text = screen(&mut app, 120, 36);
    assert!(text.contains("all folders"), "the way back to every folder must be shown\n{text}");
    assert!(app.selected.is_empty(), "nothing may be preselected");
    press(&mut app, 'x');
    assert_eq!(app.selected.len(), 1);
    let path = app.selected.keys().next().unwrap().clone();
    assert!(path.contains("target/debug/artifact-"), "{path}");
    let original = std::fs::read(&path).unwrap();
    press(&mut app, 'p');
    settle(&mut app);
    let plan = app.plan.clone().expect("plan should be created");
    assert_eq!(plan.items.len(), 1);
    let text = screen(&mut app, 120, 36);
    assert!(text.contains("Immutable plan"), "{text}");
    assert!(text.contains(&plan.approval_phrase), "{text}");
    key(&mut app, KeyCode::Enter);
    settle(&mut app);
    assert!(app.plan.is_some(), "an empty approval must not execute");
    assert!(app.operation.is_none());
    assert!(std::path::Path::new(&path).exists());
    for c in "QUARANTINE wrong".chars() {
        press(&mut app, c);
    }
    key(&mut app, KeyCode::Enter);
    settle(&mut app);
    assert!(app.operation.is_none(), "a wrong phrase must not execute");
    assert!(std::path::Path::new(&path).exists());
    app.handle_key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL));
    assert!(app.approval.is_empty());
    for c in plan.approval_phrase.chars() {
        press(&mut app, c);
    }
    assert!(app.approval_matches());
    key(&mut app, KeyCode::Enter);
    settle(&mut app);
    let operation = app
        .operation
        .clone()
        .expect("operation should run after approval");
    assert_eq!(operation.items.len(), 1);
    assert!(
        !std::path::Path::new(&path).exists(),
        "file should be quarantined"
    );
    let text = screen(&mut app, 120, 36);
    assert!(text.contains("Operation"), "{text}");
    assert!(text.contains("restore quarantined files"), "{text}");
    press(&mut app, 'u');
    settle(&mut app);
    assert_eq!(
        std::fs::read(&path).unwrap(),
        original,
        "restore must preserve bytes"
    );
    let text = screen(&mut app, 120, 36);
    assert!(text.contains("restored"), "{text}");
    key(&mut app, KeyCode::Esc);
    settle(&mut app);
    assert!(app.operation.is_none());
    press(&mut app, 'o');
    assert_eq!(app.overlay, Overlay::Operations);
    let text = screen(&mut app, 120, 36);
    assert!(text.contains("Previous operations"), "{text}");
}

#[test]
fn scan_dialog_indexes_a_typed_folder_and_reports_progress() {
    let temp = tempfile::tempdir().unwrap();
    let folder = temp.path().join("workspace");
    std::fs::create_dir_all(folder.join("nested")).unwrap();
    std::fs::write(folder.join("nested/data.bin"), vec![1u8; 128 * 1024]).unwrap();
    let engine = open_engine(&temp);
    let mut app = App::new(engine.clone(), Theme::truecolor());
    settle(&mut app);
    press(&mut app, 's');
    app.handle_key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL));
    assert!(app.scan_input.is_empty());
    for c in folder.display().to_string().chars() {
        press(&mut app, c);
    }
    key(&mut app, KeyCode::Enter);
    assert!(app.busy);
    assert_eq!(app.overlay, Overlay::Progress);
    let text = screen(&mut app, 120, 36);
    assert!(text.contains("Scanning"), "{text}");
    settle(&mut app);
    assert_eq!(app.overlay, Overlay::None);
    assert_eq!(engine.scans().unwrap().len(), 1);
    assert!(
        app.toast
            .as_ref()
            .is_some_and(|t| t.text.contains("completed")),
        "{:?}",
        app.toast_history
    );
    let text = screen(&mut app, 120, 36);
    assert!(text.contains("Indexed storage"), "{text}");
    assert!(text.contains("workspace"), "{text}");
    press(&mut app, 's');
    assert_eq!(app.overlay, Overlay::Scan);
    let text = screen(&mut app, 120, 36);
    assert!(text.contains("Indexed · rescan"), "{text}");
}

#[test]
fn ansi_palette_renders_without_truecolor() {
    let harness = fixture();
    let mut app = App::new(harness.engine.clone(), Theme::ansi());
    settle(&mut app);
    let text = screen(&mut app, 100, 30);
    assert!(text.contains("Capacity"), "{text}");
}

/// Visual smoke check for humans: `cargo test -p stratum-tui -- --ignored --nocapture dump_screens`.
#[test]
#[ignore = "prints every page for manual inspection"]
fn dump_screens() {
    let harness = fixture();
    let mut app = App::new(harness.engine.clone(), Theme::truecolor());
    settle(&mut app);
    for page in Page::ALL {
        go(&mut app, page);
        if page == Page::Cleanup {
            press(&mut app, 'x');
        }
        println!("===== {} · 132x40 =====", page.title());
        println!("{}", screen(&mut app, 132, 40));
    }
    press(&mut app, 's');
    println!("===== scan dialog =====\n{}", screen(&mut app, 132, 40));
    key(&mut app, KeyCode::Esc);
    press(&mut app, '?');
    println!("===== help =====\n{}", screen(&mut app, 132, 40));
    key(&mut app, KeyCode::Esc);
    app.choose_page(Page::Overview);
    settle(&mut app);
    println!(
        "===== overview · 90x28 compact =====\n{}",
        screen(&mut app, 90, 28)
    );
    let temp = tempfile::tempdir().unwrap();
    let mut empty = App::new(open_engine(&temp), Theme::truecolor());
    settle(&mut empty);
    println!(
        "===== first run · 120x34 =====\n{}",
        screen(&mut empty, 120, 34)
    );
}
