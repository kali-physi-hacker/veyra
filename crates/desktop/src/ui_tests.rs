use super::*;

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
#[test]
fn empty_overview_is_guided_and_does_not_start_scanning() {
    let temp = tempfile::tempdir().unwrap();
    let engine = Engine::open(Config {
        data_dir: temp.path().join("state"),
        ..Default::default()
    })
    .unwrap();
    let ctx = egui::Context::default();
    let mut app = App::new(&ctx, engine.clone(), Page::Overview);
    settle(&mut app);
    for width in [960.0, 1280.0] {
        frame(&mut app, &ctx, vec![], width);
        let text = frame(&mut app, &ctx, vec![], width);
        assert!(
            text.iter()
                .any(|(text, _)| text.contains("Meet your storage"))
        );
        assert!(text.iter().any(|(text, _)| text == "Choose a folder…"));
    }
    assert!(engine.scans().unwrap().is_empty());
}

#[test]
fn map_inspects_direct_files_and_navigation_stays_available_during_work() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("files");
    std::fs::create_dir(&root).unwrap();
    std::fs::write(root.join("report.bin"), "important file").unwrap();
    let engine = Engine::open(Config {
        data_dir: temp.path().join("state"),
        ..Default::default()
    })
    .unwrap();
    engine.scan_location(root.to_str().unwrap()).unwrap();
    let ctx = egui::Context::default();
    let mut app = App::new(&ctx, engine, Page::Map);
    settle(&mut app);
    frame(&mut app, &ctx, vec![], 1280.0);
    let text = frame(&mut app, &ctx, vec![], 1280.0);
    let position = text
        .iter()
        .find(|(s, _)| s.starts_with("report.bin\n"))
        .unwrap()
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
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("project");
    std::fs::create_dir_all(root.join("target")).unwrap();
    std::fs::write(root.join("Cargo.toml"), "[package]").unwrap();
    std::fs::write(root.join("target/artifact"), "recover me").unwrap();
    let engine = Engine::open(Config {
        data_dir: temp.path().join("state"),
        ..Default::default()
    })
    .unwrap();
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
