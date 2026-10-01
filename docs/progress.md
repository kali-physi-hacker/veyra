# Implementation progress

## Completed implementation

- New eight-crate Cargo workspace: domain, platform, index, engine, HTTP API, CLI, native desktop and optional TUI.
- Streaming, bounded traversal with cancellation/pause, directory totals, metadata, allocation, inode/device, exclusions, hidden-file handling, mount boundaries, permission/encoding warnings and atomic index publication.
- SQLite migrations, WAL, batched insertion, query indexes, generation reconciliation, historical directory totals and durable audit.
- Largest files/directories, file filters and pagination, categories with confidence, staged duplicate hashing and cache invalidation, hard-link-aware duplicate groups.
- Deterministic developer-storage and growth/anomaly rules; evidence-based macOS application footprint estimates and non-executable uninstall review proposals.
- Conservative candidate discovery, immutable expiring plans, explicit approval, protected paths, same-filesystem quarantine, content/identity validation, per-item journal, no-clobber undo, and a separately authorized purge of quarantined files.
- Native filesystem watching, leaf-level incremental updates, hash invalidation, periodic full reconciliation and low-detail system history.
- Loopback bearer-authenticated API v1, background jobs, SSE events, typed domain errors and generated OpenAPI; no shell/SQL execution endpoint.
- CLI JSON output, native desktop dashboard/explorer/map/cleanup/apps/duplicates/system/insights/history/audit, and a read-only TUI.

## Decisions

The desktop is native Rust/egui; all clients use the same service layer. Initial traversal uses one bounded producer and one batched writer. A whole root generation remains invisible until publication. Filesystem events are advisory and never alone establish a perfectly fresh index. Cleanup intentionally handles individual verified files; broad recursive deletion is not implemented, and permanent deletion exists only as a separately authorized purge of files already in quarantine.

## Validation

Validated locally on Apple M2 Max, macOS 26.5.1, Rust 1.94:

- `cargo fmt --all --check`, strict workspace Clippy with all targets/features, and `cargo test --workspace` pass. The suite contains 35 integration/safety/API tests, including a real CLI workflow across separate processes.
- Release binaries build for CLI, desktop and TUI. An unsigned macOS `.app` and compressed release archive are produced under `dist/`.
- Live loopback HTTP checks confirmed bearer authentication, unauthorized 401, paginated file queries, explicit cleanup plan execution and byte-preserving undo on a synthetic fixture.
- The native macOS watcher indexed a newly created fixture file through an incremental update, without a full scan. Its shutdown was exercised.
- The packaged native desktop was launched against generated fixture data. Dashboard, treemap and explorer layouts were inspected; clipped cards and missing glyphs were corrected. Screenshots are in `docs/screenshots/`. Full manual interaction coverage on every desktop page remains unverified.
- The release TUI opened the persistent fixture index, switched tabs and exited cleanly.
- Version 1 to 2 SQLite migration preserves existing audit data. Recovery tests cover interrupted move/restore journals, modified originals and destination conflicts.
- CI is configured for macOS and Ubuntu, including release build and OpenAPI consistency checks. There is no remote configured, so hosted CI has not run and Linux execution has not been locally verified.

Tests and visual inspection exposed and fixed an application-inventory false positive for a cache directory ending in `.app`, a mismatched APFS non-UTF-8 fixture assumption, and an index query plan that scanned a full generation for directory results. Inventory requires valid bundle metadata; unsupported path encoding remains explicit; domain queries now select the relevant SQLite index.

## Measured performance

These are exploratory measurements on the development machine, with APFS near capacity and other build activity. They are not isolated performance guarantees. Filesystem fixture generation is excluded from the times; index-only workloads synthesize metadata instead of creating millions of files.

| Workload | Observed result |
| --- | --- |
| 100,000 real fixture files: traversal only | 0.403 s |
| 100,000 files: traversal + SQLite publication | 12.226 s |
| 100,000 files: largest-directory query | 1.461 ms |
| 100,000 files: staged duplicates (50,000 groups) | 88.204 s |
| 100,000-file index: one incremental creation + ancestor/history updates | 27.893 ms |
| 100,000 files: full reconciliation | 18.129 s |
| 1,000,000 synthetic file records: insert + publish | 82.636 s |
| 1,000,000 records: largest-directory query | 23.954 ms |
| 3,000,000 synthetic file records: insert + publish | 263.883 s |
| 3,000,000 records: largest-directory query | 24.187 ms |

The million-record directory query initially took 6.448 s; the query-index correction reduced it to 23.954 ms. The 3-million-record database occupied 4,006,371,328 bytes, including entry JSON and indexes. Large scans require capacity for both the previous and staged generation. Millions of *real filesystem files* have not been traversed in this validation; that larger workload remains available through the benchmark harness for a dedicated test disk.

## Known limits

- This is a functioning developer release, not completion of every long-term feature in the original vision.
- Quarantine retains disk usage until it is purged. No automatic purge or quarantine expiry, cross-volume move or executable bundle uninstall exists.
- Index history is shallow directory aggregates, not full historical per-file attribution. Seven-day answers require an actual observation baseline.
- Memory-pressure metrics, persistent per-process anomaly baselines, Docker/Podman internal ownership, authoritative unused/orphaned app detection and APFS exclusive block accounting are unavailable.
- Windows platform execution, app signing/notarization and release distribution remain unverified.
- Non-UTF-8 paths are reported and excluded from API v1; maximum traversal depth is 256. Deep permission failures remain visible as incomplete coverage.
- Some list APIs are bounded rather than fully cursor-paginated; exact bounds are documented. Duplicate memberships and grouping live in SQLite; reports show a bounded first page with explicit truncation and a paginated group API.
- Interrupted scans restart from the root. Durable traversal-cursor resume is not implemented.
- Same-user adversarial filesystem mutation has residual TOCTOU limits described in cleanup safety. No claim of transactional multi-file filesystem operations is made.

## Upcoming work

Broaden native platform adapters and ownership evidence, add per-group file pagination, retain richer historical aggregate dimensions, validate large filesystem workloads on dedicated storage, and complete desktop accessibility/interaction coverage. MCP remains a thin future adapter over existing APIs.

## Desktop, intelligence and optimization iteration — 2026-09-26

Research and product hypotheses are recorded in [product-research.md](product-research.md), with primary sources from CleanMyMac, DaisyDisk, iStat Menus and GrandPerspective. The focus is faster, trustworthy investigation for developers and technical Mac users, not a claim that the product is commercially ready.

### Implemented

- Guided empty state, native folder selection, drag-and-drop scope review, keyboard shortcuts, grouped sidebar, capacity overview, immediately visible scan scope/freshness, and links from findings into exploration or candidate review.
- File-inclusive treemap with linked list, metadata/evidence inspector, breadcrumbs, backward/forward navigation, largest/recent-file views and explicit accounting for entries beyond the displayed map limit. The map's standard-widget list is also its keyboard/accessibility alternative.
- Independent, coalesced queries while a mutation runs; stale responses cannot overwrite current navigation. Worker completion wakes the UI instead of frequent idle polling. Actual scan pause/resume/cancel and duplicate cancellation remain separate from cleanup authorization.
- A persistent cleanup selection bar, compact grouped file review, exact immutable plan/expiry display, typed approval and a distinct restore/result view. No preselection or broadened cleanup permissions.
- Clearer application footprint cards, association evidence and non-executable uninstall reports; duplicate observations explicitly retain their analysis-time limitations.
- Transactionally maintained category rollups; a partial directory-name index and replacement covering parent index. The overview samples summary resources without collecting every process.
- Developer findings suppress nested dependency matches and carry typed parent-share measurements. Recent large files are observations, not deletion recommendations. Growth anomalies normalize by elapsed time and exclude partial/same-time comparisons.
- Additive API/CLI directory breakdown and entry inspection, generated OpenAPI, and schema 1/2→4 migration coverage.

### Measurements

Same M2 Max development machine; background apps, builds and APFS capacity pressure were present. These are individual exploratory runs, not isolated distribution statistics or performance guarantees. Sizes refer to synthetic indexed metadata, not real million-file scans.

| Measurement | Before iteration | Final implementation |
| --- | --- | --- |
| 100k records: category query | 359.514 ms | 0.032 ms |
| 100k records: insight rules (no findings in this fixture) | 3.009 ms | 0.318 ms |
| 100k records: insert + publish | 5.628 s | 7.091 s |
| 100k records: database size | 131,526,656 bytes | 132,403,200 bytes |
| 1m records: category query | Not measured in the old implementation | 0.070 ms |
| 1m records: insight rules (no findings) | Not measured | 1.717 ms |
| 1m records: largest directory query | Earlier release: 23.954 ms | 19.536 ms |
| 1m records: insert + publish | Earlier release: 82.636 s | 89.888 s |
| 1m records: database size | Earlier release: 1,328,574,464 bytes | 1,337,376,768 bytes |

Read latency improved substantially; publication still performs the one-time aggregation and carries some write cost. An intermediate full-entry name index increased storage and insertion cost; the final partial index and removal of the superseded parent index avoided that duplication. The earlier three-million-record results above belong to the initial release, not a rerun of this iteration.

### Validation boundaries

All 50 tests pass, along with formatting and strict all-target/all-feature Clippy. The suite includes headless desktop pointer interactions that verify no automatic first-run scan, direct-file inspection, navigation during work, disabled execution without an exact approval phrase, actual fixture quarantine after approval, and byte-preserving restore. Additional tests cover rate-normalized anomalies, nested developer findings, recent-large-file safety, rollup reconciliation and API remainder accounting.

Native screenshots were inspected for onboarding, overview, map and cleanup. Native UI automation was unavailable and macOS Accessibility automation permission was absent; native file-dialog interaction and screen-reader quality remain unverified. AccessKit support is enabled, but that is not an accessibility certification. Signing/notarization, hosted CI, independent safety review and customer willingness to pay remain outstanding. Preview packaging is separate from the already-open original build; older binaries must not share an upgraded state database.

## Desktop and terminal interface redesign — 2026-09-27

### Implemented

- Desktop: bundled Inter, JetBrains Mono and Phosphor typography and icons; dark and light palettes that follow the system appearance with a runtime toggle; a module sidebar with coloured icon tiles; gradient primary actions; a component kit of cards, badges, rows, fields, segmented controls, animated checkboxes, a step indicator, skeleton and empty states; an animated capacity ring, category stacked bars, a treemap with hover and entrance motion, system gauges and a gradient history chart; a modal scan dialog with quick-pick locations; a floating activity card with pause, resume and cancel during scans and cancel during hashing; page-entry transitions; a rasterised dock icon; and a macOS full-size content view.
- Terminal: nine keyboard-driven pages (overview, insights, storage browser with inspector, files, applications, duplicates with verification, cleanup with plan, approval and restore, live system monitor, audit), a responsive sidebar that collapses to a tab strip, overlays for scanning, progress, roots, operations and help, a braille spinner and toasts, and truecolor and ANSI palettes.
- Behaviour is unchanged: nothing is preselected, quarantine only happens through immutable plans with the exact typed phrase, and no scan starts automatically on either surface.

### Validation

- Workspace formatting, strict Clippy across all targets and features, and the test suite pass. The desktop crate has 16 tests, including headless interaction tests that render every page in both appearances; the terminal crate has 22 tests that render each page into a test backend and exercise the full cleanup flow.
- Native window captures of every desktop page in both appearances against the synthetic fixture were inspected. Layout defects found this way, such as a non-wrapping banner that widened the page, badge collisions with long paths, and a hard edge on treemap tiles, were fixed before this record was written.

### Limits

- Screen-reader quality, native dialogs, signing and notarization remain unverified. The activity card during long scans and the terminal's live scan progress were exercised only through tests and short synthetic scans. Terminal glyph widths depend on the terminal's font, and ambiguous-width symbols assume single-width rendering.

## Scanner throughput, live results and the saved index — 2026-09-27

### Implemented

- Traversal: a pool of directory-listing workers (up to eight by default, `scan_threads` overrides) feeds one coordinator that owns the open part of the tree, emits every directory after its descendants with final totals, and republishes the running totals of still-open directories every 400 ms as provisional rows. Worker reports and the index queue are bounded, and pause and cancel still act per directory.
- Index: entries are stored as columns; the JSON copy of every row, which repeated the path three times, is gone. A pool of reader connections sits beside the single writer, so a page never waits for a batch. Secondary indexes are partial over published rows, so a running scan touches only the primary key; publication drops and rebuilds the five indexes in sorted passes, removes the previous generation with a range delete and packs the pages. Bulk mode (no flush per commit, a larger page cache, rare checkpoints) applies only while the scan holds the writer lock. New databases use 16 KB pages; batches commit at 20,000 rows or after one second.
- Visible generation: `Engine::set_live_view` makes a root's first scan readable while it runs; published generations answer from their indexes and the running one from primary-key ranges, merged into one ordering. A rescan keeps the published generation visible until it publishes. The desktop and terminal enable it, refresh the open page as batches land, show a live badge, and open saved indexes from the scan dialog without scanning; rescanning is a separate action.
- Derived views (overview, insights, applications) are cached against an index version that every write advances, and the developer-storage rule became one query. Progress events follow each batch, per-path events are 100 ms samples, and completion is announced before the checkpoint that ends bulk mode.

### Measurements

Same M2 Max, APFS at 99 % capacity, other applications running. Single runs, not distributions. The real tree is `~/development/personal`: about 530,000 entries and 21 GB logical.

| Measurement | Before | After |
| --- | --- | --- |
| First scan, wall time | 202.05 s | 14.25 s (streaming 8.4 s, publication 5.2 s) |
| Rescan of the same root | 123.77 s (measured midway, with columns and bulk mode already in) | 17.98 s (10.7 s, 7.1 s) |
| Index file after the first scan | 3,304,284,160 bytes, 1.5 GB of it empty page space | 750,616,576 bytes |
| Traversal alone, one thread | 12.55 s (`find -type f`: 11.98 s) | 3.32 s with eight workers; 4.52 s with four |
| Largest-directory query | 1.5 ms | unchanged class, under 10 ms |

Steps along the way, first scan: columns, reader pool and bulk mode 94.08 s; partial indexes updated row by row 28.70 s but 82.07 s for a rescan; rebuilding the indexes at publication 22.95 s and 23.18 s; 16 KB pages and five indexes 14.25 s and 17.98 s.

### Limits

- What remains is roughly half streaming inserts, about 60,000 rows per second into the primary key, and half the index rebuild; the traversal is three seconds of it. Metadata is one `lstat` per entry; macOS bulk attribute calls would need foreign calls the workspace forbids.
- A rescan holds two generations for a moment. Freed pages are reused rather than returned, so the file settles near one generation plus slack and is never shrunk.
- Sorting by allocated bytes and filtering by category now sort or filter through the size index; both are rare and slower on large indexes.
- Live results cover a root's first scan; a rescan stays behind the saved index until it publishes. The API and CLI keep the published view unless a client opts in.
- Migration 5 drops the entries of earlier releases rather than converting them: an in-place conversion of a multi-gigabyte index needs more free space than the index itself and hours of rewriting. Scan records, history, plans, operations and audit records survive; each location must be rescanned once, and the store vacuums the file so the space returns to the disk.

## The write-ahead log after a scan — 2026-10-01

### Found

The Home scan of 27 September left a 13.1 GB `index.sqlite3-wal` beside a 5.4 GB index. Bulk mode ended with a passive checkpoint, which copies frames into the database but never shrinks the file, and the pages reading the running scan kept SQLite from restarting the log. That checkpoint had also stopped short: 1.8 GB of the scan existed only in the WAL until it was folded in on 1 October, so deleting the file would have lost it.

### Implemented

- Every write connection sets `journal_size_limit` to 64 MiB.
- `insert_batch` checkpoints and truncates the WAL between batches once it passes 512 MiB, waiting up to 100 ms for readers, or two seconds past 2 GiB.
- Publication starts from an empty WAL, and the end of bulk mode truncates it with up to three two-second waits for readers, falling back to a passive checkpoint and a warning.
- `Store::open` truncates any WAL left behind; `Store::wal_bytes` reports its size. Two index tests cover a finished scan and a WAL left by another connection.

### Measurements

`~/development/personal`, 546,891 entries, scanned with a reader querying the table every 0.2 s throughout: 16 s (streaming 10.0 s, publication 5.4 s); the WAL peaked at 739 MiB during publication against a 738 MiB index, and no WAL remained afterwards. The Home index: 13.1 GB of WAL folded in and truncated; its files went from 18.5 GB to 7.6 GB with no free pages, and 29 GiB of the disk is free again.

### Limits

- Publication is one transaction, so the WAL briefly reaches about the size of the generation it publishes: roughly 6 GB for a 5.5-million-entry home folder. A disk with less free space than that fails the publication, which rolls back and leaves the previous generation in place.
- A truncation waits for readers; a page holding a long read can defer it to the next batch or the next open.

## Cleanup from the whole index — 2026-10-01

### Found

The Cleanup page in both interfaces asked the index for its hundred largest files and ran the candidate rule over those alone. Cargo build output and package caches are rarely among a home folder's largest files, so the first page of the Home index held no candidate and the rest of 5.5 million entries was reachable only through Next. While a scan ran, the page refreshed every 900 ms and the desktop and terminal discarded any answer that arrived after a newer request, so a query slower than 900 ms never showed and the page stayed on its loading placeholder.

### Implemented

- `Engine::cleanup_locations` and `stratum cleanup locations`: every recognised folder, largest first, from the directory-name index and the folders' indexed totals, with a manifest check per Cargo `target` and nested folders folded into their parent. Unscoped calls are cached against the index version.
- Desktop and terminal Cleanup open on that list; a folder opens its candidate files by size; the selection carries across folders into one plan, still at most a thousand regular files with their identity and content checks.
- `LatestRequest` separates navigation from same-view refreshes in both interfaces, starts queued queries as soon as an answer arrives, and reports `waiting` until a view's first answer, which placeholders now use.
- Tests: the engine finds all three kinds of folder past a larger ordinary file and leaves out a Maven `target` and a nested one; the desktop opens on the folder and a click lists its artifact; the terminal walks folder, files, plan, quarantine and restore; both request trackers keep an answer across refreshes and drop it across navigation.

### Measurements

The Home index (5,515,645 entries): `stratum cleanup locations` lists 11 folders holding 12.31 GB (11.72 GB in one project's `target`) in 0.74 s including process start; the first page of candidates in that folder takes 0.05 s. The old first page, the hundred largest files, held no candidates.


## Permanent deletion from quarantine — 2026-10-02

### Found

Cleanup could only quarantine. Files moved into Stratum's private storage on the same filesystem, so the 12.31 GB of recognised build output and caches stayed on disk unless restored, and the safety contract listed purge as future work. Plans hold at most a thousand files, and the only bulk selection was the visible page of a hundred. The desktop outcome also drew successfully quarantined files with the failure icon.

### Implemented

- `Engine::purge_quarantine`, `stratum cleanup purge` and `POST /cleanup/operations/{id}/purge` permanently delete what one quarantine operation still holds. The phrase is `PURGE <operation-id>`, separate from `QUARANTINE <plan-id>`, and `purge_after_hours` (default 0) can require a minimum time in quarantine.
- Each file must still match the hash and identity recorded after its move and sit directly inside its operation's quarantine folder. `secure_fs::remove_regular` re-checks identity and unlinks through the no-follow parent directory.
- A `purging` intent is journaled before the first deletion. Files end `purged`, `purge_failed` or `missing`, the operation `purged` or `purge_partial`, and the start, every file and the completion are audited. Restore skips purged files and refuses an operation with nothing left in quarantine.
- `stratum cleanup operations` lists every operation with what it still holds and its purge phrase.
- Desktop: *Delete permanently…* on an operation's outcome opens a confirmation with the phrase, a typed field and, when one applies, the remaining wait. Quarantined files now show as held. *Select largest 1,000* replaces the selection with the thousand largest candidates in the opened folder.
- Terminal: `D` opens the same confirmation and `A` selects the largest thousand.
- Tests: the engine covers phrase separation, deletion, changed and missing copies, an interrupted purge, the waiting period and the folder boundary. The platform test covers replaced, symlinked and hard-linked files. The command line, desktop and terminal each walk a purge end to end, and the OpenAPI snapshot carries the route.

### Measurements

Release build on the development machine, with a synthetic Cargo `target` of 999 one-megabyte files (1.05 GB). Times include process start.

| Step | Time |
| --- | --- |
| Plan, hashing every file | 1.73 s |
| Quarantine | 3.58 s |
| Purge | 1.07 s |

The quarantine folder was empty afterwards, and the audit held one record per deleted file.

### Limits

- A purge only reaches files a quarantine has moved. Quarantine still takes a thousand files per plan, so a large `target` needs several rounds.
- A same-user process that swaps a quarantined file between the final identity check and the unlink is outside what a local application can prevent. The quarantine folder is private to the user.
- On APFS, local snapshots keep deleted blocks until they expire, so free space can lag a purge.
