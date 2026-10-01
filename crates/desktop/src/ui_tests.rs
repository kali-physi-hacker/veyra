//! Headless rendering tests. Each frame is driven through `egui::Context::run`, and the text
//! shapes it emits are collected so pointer interactions can target real widgets.
use super::*;
use eframe::egui::Vec2;

fn settle(app: &mut App) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while app.busy || app.queries.loading() {
        app.receive();
        assert!(
            std::time::Instant::now() < deadline,
            "UI work did not complete: {:?}",
            app.error
        );
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}
fn frame(
    app: &mut App,
    ctx: &egui::Context,
    events: Vec<egui::Event>,
    width: f32,
) -> Vec<(String, egui::Pos2)> {
    let output = ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                Vec2::new(width, 850.0),
            )),
            events,
            ..Default::default()
        },
        |ctx| app.render(ctx),
    );
    fn collect(shape: &egui::Shape, out: &mut Vec<(String, egui::Pos2)>) {
        match shape {
            egui::Shape::Text(text) => out.push((
                text.galley.job.text.clone(),
                text.pos + text.galley.rect.center().to_vec2(),
            )),
            egui::Shape::Vec(shapes) => {
                for shape in shapes {
                    collect(shape, out);
                }
            }
            _ => {}
        }
    }
    let mut text = Vec::new();
    for shape in output.shapes {
        collect(&shape.shape, &mut text);
    }
    text
}
fn click(app: &mut App, ctx: &egui::Context, position: egui::Pos2) {
    for pressed in [true, false] {
        frame(
            app,
            ctx,
            vec![
                egui::Event::PointerMoved(position),
                egui::Event::PointerButton {
                    pos: position,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
            1280.0,
        );
    }
}
fn fixture_engine() -> (tempfile::TempDir, Arc<Engine>) {
    let temp = tempfile::tempdir().unwrap();
    let engine = Engine::open(Config {
        data_dir: temp.path().join("state"),
        ..Default::default()
    })
    .unwrap();
    (temp, engine)
}

#[test]
fn empty_overview_is_guided_and_does_not_start_scanning() {
    let (_temp, engine) = fixture_engine();
    let ctx = egui::Context::default();
    let mut app = App::new(&ctx, engine.clone(), Page::Overview);
    settle(&mut app);
    for width in [980.0, 1280.0] {
        frame(&mut app, &ctx, vec![], width);
        let text = frame(&mut app, &ctx, vec![], width);
        assert!(
            text.iter()
                .any(|(text, _)| text.contains("Meet your storage"))
        );
        assert!(text.iter().any(|(text, _)| text == "Choose a folder…"));
        assert!(text.iter().any(|(text, _)| text == "Stratum"));
    }
    assert!(engine.scans().unwrap().is_empty());
}

#[test]
fn map_inspects_direct_files_and_navigation_stays_available_during_work() {
    let (temp, engine) = fixture_engine();
    let root = temp.path().join("files");
    std::fs::create_dir(&root).unwrap();
    std::fs::write(root.join("report.bin"), "important file").unwrap();
    engine.scan_location(root.to_str().unwrap()).unwrap();
    let ctx = egui::Context::default();
    let mut app = App::new(&ctx, engine, Page::Map);
    settle(&mut app);
    frame(&mut app, &ctx, vec![], 1280.0);
    let text = frame(&mut app, &ctx, vec![], 1280.0);
    let position = text
        .iter()
        .find(|(s, _)| s == "report.bin")
        .expect("the map must label its direct file")
        .1;
    click(&mut app, &ctx, position);
    assert_eq!(app.selected_entry.as_ref().unwrap().name, "report.bin");
    app.busy = true;
    let text = frame(&mut app, &ctx, vec![], 1280.0);
    let position = text
        .iter()
        .find(|(s, _)| s == "Storage explorer")
        .unwrap()
        .1;
    click(&mut app, &ctx, position);
    assert!(app.page == Page::Explorer);
    app.busy = false;
    settle(&mut app);
    assert_eq!(app.files[0].name, "report.bin");
}

#[test]
fn approval_button_requires_exact_phrase_and_undo_restores_bytes() {
    let (temp, engine) = fixture_engine();
    let root = temp.path().join("project");
    std::fs::create_dir_all(root.join("target")).unwrap();
    std::fs::write(root.join("Cargo.toml"), "[package]").unwrap();
    std::fs::write(root.join("target/artifact"), "recover me").unwrap();
    engine.scan_location(root.to_str().unwrap()).unwrap();
    let path = std::fs::canonicalize(root.join("target/artifact")).unwrap();
    let path = path.display().to_string();
    let ctx = egui::Context::default();
    let mut app = App::new(&ctx, engine.clone(), Page::Cleanup);
    settle(&mut app);
    assert!(app.selected.is_empty());
    app.plan = Some(
        engine
            .create_cleanup_plan(PlanRequest {
                paths: vec![path.clone()],
            })
            .unwrap(),
    );
    let phrase = app.plan.as_ref().unwrap().approval_phrase.clone();
    frame(&mut app, &ctx, vec![], 1280.0);
    let text = frame(&mut app, &ctx, vec![], 1280.0);
    let position = text
        .iter()
        .find(|(t, _)| t == "Authorize quarantine")
        .expect("Approval control must be visible")
        .1;
    click(&mut app, &ctx, position);
    assert!(!app.busy);
    assert!(std::path::Path::new(&path).exists());
    app.approval = phrase;
    let text = frame(&mut app, &ctx, vec![], 1280.0);
    let position = text
        .iter()
        .find(|(t, _)| t == "Authorize quarantine")
        .unwrap()
        .1;
    click(&mut app, &ctx, position);
    settle(&mut app);
    assert!(app.error.is_none(), "{:?}", app.error);
    assert!(!std::path::Path::new(&path).exists());
    assert!(app.operation.is_some());
    frame(&mut app, &ctx, vec![], 1280.0);
    let text = frame(&mut app, &ctx, vec![], 1280.0);
    let position = text
        .iter()
        .find(|(t, _)| t == "Restore quarantined files")
        .expect("Restore control must be visible")
        .1;
    click(&mut app, &ctx, position);
    settle(&mut app);
    assert_eq!(std::fs::read_to_string(path).unwrap(), "recover me");
}

#[test]
fn every_page_renders_in_both_appearances_with_indexed_data() {
    let (temp, engine) = fixture_engine();
    let root = temp.path().join("workspace");
    std::fs::create_dir_all(root.join("Projects/atlas/target/debug")).unwrap();
    std::fs::create_dir_all(root.join("Downloads")).unwrap();
    std::fs::write(root.join("Projects/atlas/Cargo.toml"), "[package]").unwrap();
    std::fs::write(
        root.join("Projects/atlas/target/debug/artifact"),
        vec![7u8; 4096],
    )
    .unwrap();
    std::fs::write(root.join("Downloads/archive.zip"), vec![1u8; 2048]).unwrap();
    std::fs::write(root.join("Downloads/archive-copy.zip"), vec![1u8; 2048]).unwrap();
    engine.scan_location(root.to_str().unwrap()).unwrap();
    let ctx = egui::Context::default();
    let mut app = App::new(&ctx, engine, Page::Overview);
    settle(&mut app);
    for dark in [true, false] {
        app.set_dark(dark);
        for (section, pages) in Page::sections() {
            for &page in pages {
                app.choose_page(page);
                settle(&mut app);
                frame(&mut app, &ctx, vec![], 1280.0);
                let text = frame(&mut app, &ctx, vec![], 1280.0);
                assert!(
                    text.iter().any(|(t, _)| t == page.title()),
                    "{section}/{:?} must show its title in {} mode",
                    page,
                    if dark { "dark" } else { "light" }
                );
                assert!(app.error.is_none(), "{:?}: {:?}", page, app.error);
            }
        }
    }
    assert!(!app.overview.as_ref().unwrap().coverage.is_empty());
}

#[test]
fn scan_modal_opens_with_shortcut_and_closes_without_scanning() {
    let (_temp, engine) = fixture_engine();
    let ctx = egui::Context::default();
    let mut app = App::new(&ctx, engine.clone(), Page::Overview);
    settle(&mut app);
    frame(
        &mut app,
        &ctx,
        vec![egui::Event::Key {
            key: egui::Key::O,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::COMMAND,
        }],
        1280.0,
    );
    assert!(app.show_scan_dialog);
    let text = frame(&mut app, &ctx, vec![], 1280.0);
    assert!(text.iter().any(|(t, _)| t == "Choose a scan location"));
    let position = text.iter().find(|(t, _)| t == "Cancel").unwrap().1;
    click(&mut app, &ctx, position);
    assert!(!app.show_scan_dialog);
    assert!(engine.scans().unwrap().is_empty());
}

#[test]
fn saved_index_opens_from_the_scan_dialog_without_scanning_again() {
    let (temp, engine) = fixture_engine();
    let root = temp.path().join("fixture");
    std::fs::create_dir_all(root.join("docs")).unwrap();
    std::fs::write(root.join("docs/notes.txt"), "saved").unwrap();
    engine
        .scan(ScanRequest {
            roots: vec![root.display().to_string()],
            ..Default::default()
        })
        .unwrap();
    let scans_before = engine.scans().unwrap().len();
    let ctx = egui::Context::default();
    let mut app = App::new(&ctx, engine.clone(), Page::Overview);
    settle(&mut app);
    app.show_scan_dialog = true;
    // The modal lays itself out on one frame and paints on the next.
    frame(&mut app, &ctx, vec![], 1280.0);
    let text = frame(&mut app, &ctx, vec![], 1280.0);
    let root_text = std::fs::canonicalize(&root).unwrap().display().to_string();
    // The modal paints after the page, so its chip is the last text with this label.
    let label = truncate_middle(&short_path(&root_text), 50);
    let chip = text
        .iter()
        .rfind(|(t, _)| *t == label)
        .map(|(_, pos)| *pos)
        .unwrap_or_else(|| panic!("the saved index {label:?} is listed in the dialog: {text:?}"));
    click(&mut app, &ctx, chip);
    settle(&mut app);
    assert!(
        !app.show_scan_dialog,
        "opening a saved index closes the dialog"
    );
    assert_eq!(app.page, Page::Map);
    assert_eq!(app.path, root_text);
    assert_eq!(
        engine.scans().unwrap().len(),
        scans_before,
        "no scan started"
    );
    assert!(
        app.breakdown.is_some(),
        "the saved index is browsed immediately"
    );
}

#[test]
fn cleanup_opens_on_the_recognised_folders_and_a_click_lists_their_files() {
    let (temp, engine) = fixture_engine();
    let root = temp.path().join("project");
    std::fs::create_dir_all(root.join("target/debug")).unwrap();
    std::fs::write(root.join("Cargo.toml"), "[package]").unwrap();
    std::fs::write(root.join("target/debug/artifact"), "rebuild me").unwrap();
    // A far larger ordinary file, which used to fill the only page the rules looked at.
    std::fs::write(root.join("video.mov"), vec![0u8; 512 * 1024]).unwrap();
    engine.scan_location(root.to_str().unwrap()).unwrap();
    let ctx = egui::Context::default();
    let mut app = App::new(&ctx, engine.clone(), Page::Cleanup);
    settle(&mut app);
    assert_eq!(app.locations.len(), 1, "{:?}", app.locations);
    assert!(
        app.candidates.is_empty(),
        "nothing is listed or selected before a folder is opened"
    );
    frame(&mut app, &ctx, vec![], 1280.0);
    let text = frame(&mut app, &ctx, vec![], 1280.0);
    assert!(
        text.iter().any(|(t, _)| t.contains("Cargo build output")),
        "{text:?}"
    );
    let position = text
        .iter()
        .find(|(t, _)| t.ends_with("target"))
        .expect("the folder row must be visible")
        .1;
    click(&mut app, &ctx, position);
    settle(&mut app);
    assert!(
        app.cleanup_scope
            .as_deref()
            .is_some_and(|s| s.ends_with("project/target")),
        "{:?}",
        app.cleanup_scope
    );
    assert!(
        app.candidates
            .iter()
            .any(|c| c.path.ends_with("target/debug/artifact")),
        "{:?}",
        app.candidates
    );
    assert!(app.selected.is_empty(), "nothing may be preselected");
}

#[test]
fn largest_selection_then_a_separate_phrase_deletes_quarantined_files() {
    let (temp, engine) = fixture_engine();
    let root = temp.path().join("project");
    std::fs::create_dir_all(root.join("target/debug")).unwrap();
    std::fs::write(root.join("Cargo.toml"), "[package]").unwrap();
    for i in 0..3 {
        std::fs::write(
            root.join(format!("target/debug/artifact-{i}")),
            vec![i as u8; 4096],
        )
        .unwrap();
    }
    engine.scan_location(root.to_str().unwrap()).unwrap();
    let target = std::fs::canonicalize(root.join("target")).unwrap();
    let ctx = egui::Context::default();
    let mut app = App::new(&ctx, engine.clone(), Page::Cleanup);
    settle(&mut app);
    app.cleanup_scope = Some(target.display().to_string());
    app.refresh();
    settle(&mut app);
    frame(&mut app, &ctx, vec![], 1280.0);
    let text = frame(&mut app, &ctx, vec![], 1280.0);
    let position = text
        .iter()
        .find(|(t, _)| t == "Select largest 1,000")
        .expect("an opened folder offers the largest-files selection")
        .1;
    click(&mut app, &ctx, position);
    settle(&mut app);
    assert_eq!(app.selected.len(), 3, "{:?}", app.error);
    let plan = engine
        .create_cleanup_plan(PlanRequest {
            paths: app.selected.keys().cloned().collect(),
        })
        .unwrap();
    let operation = engine
        .execute_cleanup_plan(&plan.id, &plan.approval_phrase)
        .unwrap();
    app.operation = Some(operation.clone());
    frame(&mut app, &ctx, vec![], 1280.0);
    let text = frame(&mut app, &ctx, vec![], 1280.0);
    let position = text
        .iter()
        .find(|(t, _)| t == "Delete permanently…")
        .expect("a quarantine offers permanent deletion")
        .1;
    click(&mut app, &ctx, position);
    assert!(app.purge_open);
    let text = frame(&mut app, &ctx, vec![], 1280.0);
    let phrase = purge_phrase(&operation.id);
    assert!(
        text.iter().any(|(t, _)| t == &phrase),
        "the phrase must be shown"
    );
    let position = text
        .iter()
        .find(|(t, _)| t == "Delete permanently")
        .expect("the confirmation has its own button")
        .1;
    click(&mut app, &ctx, position);
    assert!(!app.busy, "nothing runs without the phrase");
    app.purge_typed = format!("QUARANTINE {}", plan.id);
    click(&mut app, &ctx, position);
    assert!(
        !app.busy,
        "the quarantine phrase does not authorize a purge"
    );
    assert!(
        operation
            .items
            .iter()
            .all(|i| std::path::Path::new(&i.destination).is_file())
    );
    app.purge_typed = phrase;
    let text = frame(&mut app, &ctx, vec![], 1280.0);
    let position = text
        .iter()
        .find(|(t, _)| t == "Delete permanently")
        .unwrap()
        .1;
    click(&mut app, &ctx, position);
    settle(&mut app);
    assert!(app.error.is_none(), "{:?}", app.error);
    let purged = app.operation.as_ref().unwrap();
    assert_eq!(purged.status, "purged");
    for item in &operation.items {
        assert!(!std::path::Path::new(&item.destination).exists());
        assert!(!std::path::Path::new(&item.source).exists());
    }
    frame(&mut app, &ctx, vec![], 1280.0);
    let text = frame(&mut app, &ctx, vec![], 1280.0);
    assert!(
        text.iter().any(|(t, _)| t.contains("deleted permanently")),
        "the outcome reports what was deleted"
    );
    assert!(!text.iter().any(|(t, _)| t == "Delete permanently…"));
}
