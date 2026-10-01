# Development

Rust 1.94 is the minimum compiler for this lockfile. The project forbids unsafe code in its own crates. Dependency versions are locked in Cargo.lock. `sysinfo` is intentionally on 0.38 because the newest 0.39 patch requires a newer compiler.

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace
cargo build --release --workspace
```

Linux desktop dependencies (Debian/Ubuntu): `pkg-config libx11-dev libxi-dev libxrandr-dev libxcursor-dev libxinerama-dev libxkbcommon-dev libwayland-dev libgl1-mesa-dev`. CI is configured for macOS and Ubuntu; hosted runs have not yet been exercised. The standalone CLI/API do not need a windowing system. Windows execution is not a release target yet.

## Fixtures and verification

Integration tests create private temporary directories and only remove their own fixtures via `tempfile`. Coverage includes idempotent migrations, newer-schema rejection, persistent index reopening, generation visibility, reconciliation, permission warnings, non-UTF-8 names, sparse files, symlinks, hard links, duplicate invalidation, cancellation, protected paths, changed plans, no-clobber moves, undo conflicts and application evidence. API tests exercise authentication, browser-origin rejection, JSON contracts, typed errors and OpenAPI.

No test scans or cleans arbitrary home directories. Filesystem permission tests require a non-root account. OS permission prompts and truly hostile same-user races need separate manual validation. Unit/integration tests do not prove APFS clone accounting, operating-system signing or successful installation on another machine.

## Benchmarks

```sh
cargo bench -p stratum-engine --bench workloads
STRATUM_BENCH_FILES=100000 cargo bench -p stratum-engine --bench workloads
STRATUM_BENCH_FILES=1000000 cargo bench -p stratum-engine --bench workloads
STRATUM_BENCH_FILES=3000000 cargo bench -p stratum-engine --bench workloads
STRATUM_BENCH_FILES=1000000 cargo bench -p stratum-engine --bench index_scale
STRATUM_BENCH_FILES=3000000 cargo bench -p stratum-engine --bench index_scale
```

The harness creates deterministic file pairs in private temporary directories, scans, queries directory aggregates, verifies duplicate candidates and reconciles a change. Fixture creation is excluded from timing. Each run emits JSON for comparison. Large profiles create many files and a substantial SQLite index; run them on a dedicated test disk with adequate free capacity. Synthetic results are not a guarantee for TCC-constrained home scans, network filesystems or cold disks.

The `index_scale` benchmark streams synthetic metadata in 1,000-entry batches to measure SQLite insertion and directory queries independently of filesystem creation. The fixture generator supplies sparse files, duplicates, application and developer storage; integration tests supply permission and link scenarios. Keep baseline measurements in progress documentation with hardware/environment caveats. Benchmark-driven changes must preserve structured coverage, cancellation, bounded queues and safety.

The benchmark also reports category-query and intelligence-rule latency.

## Scanning

`scan_threads` (0 chooses from the core count, capped at eight; up to 64) sets the directory-listing workers; `batch_size` (20,000) caps a write batch, and a batch also flushes after a second so progress reaches pages steadily. `cargo run --release -p stratum-platform --example walk -- DIR` times the traversal alone and prints entry, directory and provisional counts; `STRATUM_SCAN_THREADS=n` overrides the workers for that run.

Readers never wait for the writer: `Store` keeps a pool of read connections beside the single writer. The desktop and terminal call `Engine::set_live_view(true)`, so a root's first scan is readable while it runs and their pages refresh as batches land; a rescan keeps the published generation visible until it publishes. The API and CLI keep the published view. `Engine::cached` memoises derived views against `Store::version`, which every write bumps.

The write-ahead log never outlives a scan. Pages reading the running scan can keep SQLite from ever restarting the WAL, so `insert_batch` checkpoints and truncates it between batches once it passes 512 MiB, waiting briefly for readers (up to two seconds past 2 GiB). Publication starts from an empty WAL and rewrites the generation in one transaction, so the file peaks near the index's own size while it publishes, and the end of bulk mode truncates it to nothing. `journal_size_limit` cuts the file back to 64 MiB whenever SQLite restarts it, and opening the index truncates a WAL left by a crash or an older release (a process still reading it defers that). To fold a WAL in by hand, run `sqlite3 ~/.local/share/stratum/index.sqlite3 'PRAGMA wal_checkpoint(TRUNCATE)'`; never delete the file, because frames not yet checkpointed exist nowhere else.

Opening a state directory written by a release before schema version 5 drops its indexed entries (not its scan records, history, plans, operations or audit log), vacuums the file and records `index_format_changed` in the audit log; scan each location again.

Cleanup starts from the index, not from a page of large files. `Engine::cleanup_locations` finds every folder the candidate rules recognise (Cargo `target` directories beside a `Cargo.toml`, `~/.npm/_cacache` and `~/.cargo/registry/cache`) through the directory-name index, with the totals the index already holds for them, largest first; a folder inside another is left out. Both interfaces open on that list; opening a folder pages through its candidate files by size, and the selection carries across folders into one plan of at most a thousand files. `stratum cleanup locations` prints the same list.

Pages ask for data in two ways. Navigation (another page, folder, sort or page of results) makes any answer still on its way stale; a refresh of the open view, as while a scan fills it or the System page samples, keeps that answer and runs after it. Before this distinction, refreshes every 900 ms during a scan discarded any query slower than that, and a page could stay on its placeholder for the whole scan. Placeholders show only until the first answer for the open view arrives (`LatestRequest::waiting`).

## Desktop interface

The desktop crate is a native egui application. `theme.rs` defines the dark and light palettes and restyles egui's built-in widgets; `fonts.rs` embeds Inter at four weights, JetBrains Mono and the Phosphor icon fonts from `crates/desktop/assets/fonts` (the SIL OFL and MIT licences sit beside the files); `icons.rs` lists the Phosphor codepoints in use; `kit.rs` holds the component kit (cards, gradient buttons, badges, rings, rows, fields, segmented controls, animated checkboxes, steps, skeletons, banners, disclosure headers) plus the entrance and hover motion helpers; `shell.rs` renders the sidebar, header, scan modal and floating activity card; `brand.rs` paints the mark and rasterises the dock icon; each page lives in its own module. Widgets read the active palette from egui memory, so pages never pass colours around.

The appearance follows the system by default. `--appearance dark|light` forces one, and the sun/moon control in the sidebar switches at runtime. `--page` opens a specific page, and the hidden `--open-scan-dialog` flag opens the scan dialog immediately for screenshots; nothing scans until the dialog is confirmed. On macOS the window uses a full-size content view with a hidden title, so the sidebar sits under the traffic lights and its brand row doubles as a drag region.

Desktop shortcuts: Command/Ctrl+O chooses a scan location, Command/Ctrl+R refreshes, and Command/Ctrl+1/2/3/4 opens Overview/Map/Insights/Cleanup. Dragging a folder opens scope review and never starts an automatic scan. Native folder selection uses rfd; AccessKit semantics are enabled, with the map list providing a standard-widget alternative to the custom-painted map. Screen-reader quality still requires manual verification.

Desktop tests render frames headlessly through `egui::Context::run` and click real widgets by their text: first run must not start a scan, map inspection includes direct files and navigation stays available during work, quarantine cannot execute before the exact phrase is entered and a synthetic file is then quarantined and restored, the scan dialog opens with the shortcut and closes without scanning, and every page renders in both appearances with indexed data. Unit tests cover palette contrast, icon rasterisation, treemap layout, formatting and the request-coalescing guard. These tests supplement, not replace, native dialogs, accessibility and signed-package testing.

To reproduce the documentation screenshots, generate a fixture with `cargo run -p stratum-engine --example fixture -- DIR`, index it with `stratum --data-dir STATE scan DIR`, and launch `stratum-desktop --data-dir STATE --page map`. On macOS, `screencapture -l WINDOW_ID` captures the window even when another application is frontmost.

## Terminal interface

`stratum-tui` is a ratatui application over the same engine. `app.rs` owns the state, the background query threads with stale-response guards and every key binding; `ui/` renders the shell and one module per page; `widgets.rs` and `theme.rs` provide proportional bars, gauges, keycaps, spinners and the palette (`--palette ansi` restricts it to sixteen colours). Below 100 columns the sidebar collapses into a tab strip.

Keys: `1`–`9`, `Tab` and `Shift+Tab` switch pages; `j`/`k` or the arrows move; `Enter` opens or investigates; `Backspace` or `h` goes to the parent; `/` filters; `s` opens the scan dialog (`Space` pauses or resumes and `c` cancels a running scan); `v` verifies duplicates; on Cleanup, `Enter` opens one of the recognised folders and `0`, `h` or `Backspace` returns to the list; `x` or `Space` toggles a cleanup candidate, `a` selects the page, `n` clears, `p` creates the immutable plan from the files chosen in any folder, typing the approval phrase then `Enter` authorises quarantine and `u` restores; `o` lists previous operations or re-sorts processes; `r` refreshes; `?` shows help; `Esc` closes overlays; `q` quits.

`cargo test -p stratum-tui` renders every page into a `TestBackend` against a labelled temporary fixture, including the scan modal, the full cleanup flow and the ANSI palette. `cargo test -p stratum-tui dump_screens -- --ignored --nocapture` prints each page for manual inspection.

Library API references used for the platform/UI boundary: [eframe](https://docs.rs/eframe/0.33.3/eframe/), [rustix no-clobber rename](https://docs.rs/rustix/latest/rustix/fs/fn.renameat_with.html), and [sysinfo](https://docs.rs/sysinfo/0.38.4/sysinfo/).

## Packaging

`scripts/package-macos.sh` builds release binaries and creates `dist/Stratum.app` plus a tar archive containing CLI, TUI, desktop and documentation. The initial local artifact is unsigned; notarization and distribution signing require a developer identity and are not configured. The script does not install a daemon or copy applications into system directories.

Use `scripts/package-macos.sh dist-preview` for a separate preview artifact without replacing `dist/Stratum.app`. Close older Stratum processes before opening the new version against the same state directory: schema 4 is intentionally rejected by older binaries. For side-by-side testing, use a separate `--data-dir`; never run mixed schema versions against the same database.

CI generates a fresh OpenAPI document and checks it against the committed release snapshot. When changing handlers or domain contracts, run `scripts/update-openapi.sh` and commit the result with the implementation. Local release artifacts are build outputs and are not committed.
