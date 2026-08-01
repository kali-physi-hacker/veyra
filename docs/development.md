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

The benchmark also reports category-query and intelligence-rule latency. Desktop tests render egui headlessly and exercise pointer interactions: first-run must not start a scan, map inspection includes direct files, navigation remains available during work, and quarantine cannot execute before the exact phrase is entered. A synthetic file is then quarantined and restored through the UI handlers. These tests supplement, not replace, native dialogs, accessibility and signed-package testing.

Desktop shortcuts: Command/Ctrl+O chooses a scan location, Command/Ctrl+R refreshes, and Command/Ctrl+1/2/3/4 opens Overview/Map/Insights/Cleanup. Dragging a folder opens scope review and never starts an automatic scan. Native folder selection uses rfd; AccessKit semantics are enabled, with the map list providing a standard-widget alternative to the custom-painted map. Screen-reader quality still requires manual verification.

Library API references used for the platform/UI boundary: [eframe](https://docs.rs/eframe/0.33.3/eframe/), [rustix no-clobber rename](https://docs.rs/rustix/latest/rustix/fs/fn.renameat_with.html), and [sysinfo](https://docs.rs/sysinfo/0.38.4/sysinfo/).

## Packaging

`scripts/package-macos.sh` builds release binaries and creates `dist/Stratum.app` plus a tar archive containing CLI, TUI, desktop and documentation. The initial local artifact is unsigned; notarization and distribution signing require a developer identity and are not configured. The script does not install a daemon or copy applications into system directories.

Use `scripts/package-macos.sh dist-preview` for a separate preview artifact without replacing `dist/Stratum.app`. Close older Stratum processes before opening the new version against the same state directory: schema 4 is intentionally rejected by older binaries. For side-by-side testing, use a separate `--data-dir`; never run mixed schema versions against the same database.

CI generates a fresh OpenAPI document and checks it against the committed release snapshot. When changing handlers or domain contracts, run `scripts/update-openapi.sh` and commit the result with the implementation. Local release artifacts are build outputs and are not committed.
