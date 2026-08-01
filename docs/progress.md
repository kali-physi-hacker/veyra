# Implementation progress

## Completed implementation

- New eight-crate Cargo workspace: domain, platform, index, engine, HTTP API, CLI, native desktop and optional TUI.
- Streaming, bounded traversal with cancellation/pause, directory totals, metadata, allocation, inode/device, exclusions, hidden-file handling, mount boundaries, permission/encoding warnings and atomic index publication.
- SQLite migrations, WAL, batched insertion, query indexes, generation reconciliation, historical directory totals and durable audit.
- Largest files/directories, file filters and pagination, categories with confidence, staged duplicate hashing and cache invalidation, hard-link-aware duplicate groups.
- Deterministic developer-storage and growth/anomaly rules; evidence-based macOS application footprint estimates and non-executable uninstall review proposals.
- Conservative candidate discovery, immutable expiring plans, explicit approval, protected paths, same-filesystem quarantine, content/identity validation, per-item journal and no-clobber undo.
- Native filesystem watching, leaf-level incremental updates, hash invalidation, periodic full reconciliation and low-detail system history.
- Loopback bearer-authenticated API v1, background jobs, SSE events, typed domain errors and generated OpenAPI; no shell/SQL execution endpoint.
- CLI JSON output, native desktop dashboard/explorer/map/cleanup/apps/duplicates/system/insights/history/audit, and a read-only TUI.

## Decisions

The desktop is native Rust/egui; all clients use the same service layer. Initial traversal uses one bounded producer and one batched writer. A whole root generation remains invisible until publication. Filesystem events are advisory and never alone establish a perfectly fresh index. Cleanup intentionally handles individual verified files; broad recursive deletion and permanent purge are not implemented.

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
- Quarantine retains disk usage. No permanent purge, automatic quarantine expiry, cross-volume move or executable bundle uninstall exists.
- Index history is shallow directory aggregates, not full historical per-file attribution. Seven-day answers require an actual observation baseline.
- Memory-pressure metrics, persistent per-process anomaly baselines, Docker/Podman internal ownership, authoritative unused/orphaned app detection and APFS exclusive block accounting are unavailable.
- Windows platform execution, app signing/notarization and release distribution remain unverified.
- Non-UTF-8 paths are reported and excluded from API v1; maximum traversal depth is 256. Deep permission failures remain visible as incomplete coverage.
- Some list APIs are bounded rather than fully cursor-paginated; exact bounds are documented. Duplicate memberships and grouping live in SQLite; reports show a bounded first page with explicit truncation and a paginated group API.
- Interrupted scans restart from the root. Durable traversal-cursor resume is not implemented.
- Same-user adversarial filesystem mutation has residual TOCTOU limits described in cleanup safety. No claim of transactional multi-file filesystem operations is made.

## Upcoming work

Broaden native platform adapters and ownership evidence, add per-group file pagination, introduce high-risk purge as a separately authorized operation, retain richer historical aggregate dimensions, validate large filesystem workloads on dedicated storage, and complete desktop accessibility/interaction coverage. MCP remains a thin future adapter over existing APIs.

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
