# Architecture

Stratum is an application service with several clients. The project intentionally starts with eight functioning crates, not one empty crate per future feature.

```text
CLI / TUI / native egui desktop / Axum HTTP adapter / future MCP
                              |
                       stratum-engine
                 domain operations and policies
                    /                  \
          stratum-platform         stratum-index
        filesystem and system      SQLite persistence
                    \                  /
                      stratum-domain
                     versioned contracts
```

`domain` owns serializable models, typed errors, configuration and events. It does not depend on HTTP, SQLite, or UI libraries. `platform` owns streaming traversal, file identity, secure file access, atomic no-clobber moves, content hashing, and measured system snapshots. `index` owns migrations, transactions, query plans, generations, history and durable journals. `engine` orchestrates domain use cases, analysis rules, duplicate verification, application associations, safety policy and incremental reconciliation.

`api`, `cli`, `desktop` and `tui` only translate requests and render responses. The desktop uses native egui/eframe and the same Rust engine directly. It runs slow work on background threads. There is no second GUI backend and no JavaScript business logic. The TUI renders the same pages over the same engine, including the plan-and-approve cleanup flow with a typed phrase; it never bypasses the engine's safety checks.

## Concurrency and resource budgets

A pool of directory-listing workers (at most eight by default; `scan_threads` overrides it) shares one queue. Each worker lists a directory, stats its children and reports them. A single coordinator thread owns the open part of the tree: it emits files as they arrive, emits each directory after its last descendant with final totals, and every 400 ms republishes the running totals of directories that are still open as provisional rows. The queue is taken last-in first-out, so the set of open directories stays as small as a depth-first walk's; depth is limited to 256. Worker reports are bounded (8,192) and the coordinator feeds the index through a second bounded channel (512 by default), so a slow disk or a slow writer applies backpressure instead of consuming memory. Multiple roots are processed sequentially.

The writer commits batches of up to 4,000 entries, or whatever has accumulated after 250 ms, in path order, and keeps category totals current per batch. During a scan the writer connection runs in bulk mode: WAL commits stop forcing a disk flush and the page cache grows so index pages stay resident. Full durability returns before the root is published, and no cleanup journal record can be written meanwhile, because the scan holds the cross-process advisory lock that serializes scan, duplicate and cleanup mutations for one state directory. Entries are stored as columns under one primary key and seven secondary indexes. Readers use a separate pool of connections, so WAL lets a page query while a batch is being written; the in-process writer mutex never blocks a read. Filesystem and hashing calls hold no database lock. API blocking work runs outside the async executor.

Every root has a published generation. Interfaces that opt into the visible generation (the desktop and the terminal do) also read a root's first scan while it runs; a rescan stays invisible until it publishes, so a saved index never disappears mid-scan. Derived views such as the overview, insights and applications are cached against an index version that every write advances.

Event subscribers use a separate bounded broadcast channel: progress follows each written batch, and per-path events are samples taken every 100 ms, not a log of every entry. A slow UI does not stall indexing; SSE reports `resync_required` when events are lost. Clients query durable operation state to recover. The scanner-to-index channel is lossless and applies backpressure. Stdout is never an internal event bus.

## Evolution

HTTP lives under `/api/v1`. Rust models and `Engine` form the equivalent application API. Semver-breaking changes require a new crate version; changes to wire meanings require a new API version. Optional fields can be added. New transports call engine methods. SQL, OS commands and filesystem actions do not belong in an MCP adapter.

Analysis rules implement `AnalysisRule`; cleanup discovery rules implement `CleanupRule`. Evidence and uncertainty are model fields, not decorative explanation strings appended by a client. The deterministic engine needs no LLM. An agent may summarize its findings but cannot manufacture stronger confidence or authorize cleanup on behalf of the user.

The scanner is not a filesystem snapshot. Concurrent writers can change a machine during traversal. `probably_fresh`, partial coverage and later reconciliation communicate this limitation. Crash resume restarts an incomplete root while preserving its last published generation; resuming from an arbitrary saved traversal cursor is deliberately unsupported.
