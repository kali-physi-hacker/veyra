# Storage model

SQLite uses WAL, foreign keys, a five-second busy timeout, FULL synchronous journal durability and a bounded 16 MiB page cache. Temporary sorting can spill to disk. Migrations run inside an immediate transaction and use `PRAGMA user_version`; opening a newer schema fails explicitly. Read/write transactions are not created per discovered file.

## Tables

- `scans`: lifecycle, root, counts, warning/exclusion totals, byte totals and freshness.
- `roots`: published scan generation for each non-overlapping root.
- `entries`: generation + exact UTF-8 path key, parent, kind, timestamps, sizes, identity, category, evidence and complete domain JSON.
- `current_entries`: view over published roots, hiding staged records.
- `category_totals`: per-generation file counts and byte rollups, published and incrementally updated in the same transactions as their entries.
- `history`: root and immediate directory totals per successful scan or incremental snapshot, with an observation sequence for timestamp ties.
- `fingerprints`: sampled and full BLAKE3 hashes keyed by path and complete file identity.
- `documents`: typed application records such as plans, operation journals, reports and scan policies.
- `warnings`: retained warning evidence, separate from counts.
- `audit`: append-only application action history.
- `system_history`: lightweight timestamp/CPU/memory/swap samples.

Indexes cover per-generation size, allocation, parent, category, extension, modification time, inode/device and historical path/time. Largest-directory queries read already computed aggregates; they do not recursively scan at query time. Query strings select from an allow-list of sort fields and parameterize user inputs.

Schema 3 adds category rollups with a backfill from published generations only. Schema 4 uses a small partial directory-name index for analysis rules and replaces the old parent index with a size-ordered covering parent index. It removes an experimental all-entry name index rather than burdening every file insertion with an unnecessary index. Migration tests cover earlier data preservation and rollup backfill. Migrations may take time on a large existing index; never open the upgraded database with an older binary that rejects its schema version.

Category reads now visit compact rollups, not every filesystem entry. Initial publication still computes the rollups once; incremental leaf changes apply old/new category deltas atomically. Cancelled or staged generations do not contribute. A directory breakdown uses a single read transaction for its directory, bounded child list and exact remainder, preventing page-total inconsistencies during concurrent publication.

## Generation publication

Traversal emits children before their parent with subtree totals. Inserts go into an unpublished generation. Completion atomically publishes the root, captures shallow history and discards older full entry generations for that root. A cancelled/failed scan discards its staged entries. An interrupted process is recovered under the writer lock at the next open. The last completed generation remains usable.

A scan with permission or encoding warnings publishes partial observed coverage and reports `unknown` freshness. Previously visible inaccessible children are not silently retained as current. Exclusions and mount boundaries deliberately limit the requested scope and are counted separately. A directory's total is the sum of observed included file paths, not a guarantee of complete physical occupancy.

Incremental regular-file creation/change/deletion under an indexed parent updates the leaf, all indexed ancestors and hash invalidation in one transaction. Directory changes, unindexed parents, custom ignore policies, mount changes and queue overflow require full reconciliation. The saved scan policy is reused. Native events are advisory; periodic reconciliation is the backstop. There is no claim that FSEvents captures every mutation.

History retains shallow aggregates for 90 days by default; no complete per-file historical snapshot is retained. Baseline rules only compare complete observed scans. Missing intervals are unknown. Growth attribution below the retained depth requires future aggregate dimensions. Access times are observations, not evidence that a file is unused.

Freshness values are `probably_fresh` after a successful scan, `stale` after observed changes or an elapsed reconciliation interval, and `unknown` for incomplete coverage. `fresh` is reserved for a future stronger observation contract. Incremental updates conservatively keep stale status until reconciliation, because events cannot establish completeness.

The database is not encrypted by this application. Rely on operating-system account permissions and disk encryption. Back up a live database through SQLite's backup facilities or stop all Stratum processes first; copying only the main file while WAL is active can omit committed data.
