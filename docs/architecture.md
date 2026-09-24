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

A scanner producer and SQLite consumer run concurrently through a bounded channel (512 entries by default). Traversal is depth-first and retains only active directory iterators and totals, with depth limited to 256. One producer is intentional: opening many threads on one disk increases metadata pressure and makes cancellation and ordering harder to reason about. Multiple roots are processed sequentially. This is bounded concurrency, not a claim of maximally parallel traversal.

The writer uses batches of 1,000 entries and publishes each completed root in a transaction. A cross-process advisory lock serializes scan, duplicate and cleanup mutations for one state directory. SQLite WAL supports readers between batches; the in-process connection mutex is held only for database work. Filesystem and hashing calls do not hold it. API blocking work runs outside the async executor.

Event subscribers use a separate bounded broadcast channel. A slow UI does not stall indexing; SSE reports `resync_required` when events are lost. Clients query durable operation state to recover. The scanner-to-index channel is lossless and applies backpressure. Stdout is never an internal event bus.

## Evolution

HTTP lives under `/api/v1`. Rust models and `Engine` form the equivalent application API. Semver-breaking changes require a new crate version; changes to wire meanings require a new API version. Optional fields can be added. New transports call engine methods. SQL, OS commands and filesystem actions do not belong in an MCP adapter.

Analysis rules implement `AnalysisRule`; cleanup discovery rules implement `CleanupRule`. Evidence and uncertainty are model fields, not decorative explanation strings appended by a client. The deterministic engine needs no LLM. An agent may summarize its findings but cannot manufacture stronger confidence or authorize cleanup on behalf of the user.

The scanner is not a filesystem snapshot. Concurrent writers can change a machine during traversal. `probably_fresh`, partial coverage and later reconciliation communicate this limitation. Crash resume restarts an incomplete root while preserving its last published generation; resuming from an arbitrary saved traversal cursor is deliberately unsupported.
