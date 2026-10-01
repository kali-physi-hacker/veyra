# Application API v1

Canonical machine-readable specification: `stratum api openapi --json` or authenticated `GET /api/v1/openapi.json`. The specification is generated from the handler and domain types; `docs/openapi.json` is a release snapshot verified in CI. Times are Unix seconds, sizes are bytes. Inode/device fields are unsigned identifiers. Non-UTF-8 paths are reported as scan warnings and omitted; they are never lossily converted into actionable identities.

All requests use `Authorization: Bearer <local token>`. Querying is read-only unless the HTTP method is POST. A token grants the capabilities of this user account; there is no remote service or multi-user authority model.

| Resource | Operations |
| --- | --- |
| `/health`, `/system`, `/volumes` | GET health and measured machine data |
| `/processes`, `/processes/{pid}` | GET process snapshots |
| `/scans` | GET scan records; POST `ScanRequest` starts a background job |
| `/scans/{id}/{action}` | POST `pause`, `resume` or `cancel` for an in-process scan |
| `/scans/{id}/warnings` | GET retained warning details |
| `/jobs/{id}` | GET durable background job status/result |
| `/index/reconcile` | POST explicit changed paths for incremental leaf updates |
| `/files`, `/directories`, `/storage/largest` | GET paginated index queries |
| `/storage/categories`, `/storage/history`, `/storage/explain` | GET aggregates and evidence |
| `/storage/breakdown?path=...&limit=60` | GET a directory, largest immediate children and exact omitted-child totals |
| `/files/inspect?path=...` | GET one indexed entry and classification evidence |
| `/duplicates` | GET most recently computed duplicate report |
| `/duplicates/scan` | POST starts staged content verification |
| `/duplicates/groups` | GET duplicate group pagination using `limit`/`offset` |
| `/apps`, `/apps/{id}` | GET estimated application footprints |
| `/apps/{id}/uninstall-plan` | POST review proposal; bundle execution is unsupported |
| `/insights`, `/insights/{id}` | GET deterministic findings with evidence |
| `/cleanup/candidates` | GET conservative candidate discovery |
| `/cleanup/plans` | POST explicit `paths`; creates immutable, non-destructive plan |
| `/cleanup/plans/{id}` | GET exact plan preview |
| `/cleanup/plans/{id}/execute` | POST exact approval phrase; separate action capability |
| `/cleanup/operations/{id}` | GET durable per-file action journal |
| `/cleanup/operations/{id}/undo` | POST no-clobber restoration |
| `/cleanup/operations/{id}/purge` | POST exact purge phrase; permanently deletes what the operation still holds in quarantine |
| `/audit`, `/system/history` | GET action and resource history |
| `/events` | GET authenticated SSE operation stream |

Every path above is prefixed by `/api/v1`.

## Query semantics

`FileQuery` supports `path` (subtree, component-boundary aware), `parent` (immediate children), `name` (SQLite glob), `extension`, `kind`, `category`, minimum/maximum logical bytes, modified-before/after and created-before/after timestamps. Combined filters are ANDed. Sort options: `logical_bytes`, `allocated_bytes`, `modified_at`, `path`. Results have `items`, `limit`, `offset`, `has_more`; limits are 1–1,000. Equal-size ties use path order. Filters and values are parameterized; callers cannot supply SQL.

Candidate pagination traverses the corresponding file page and applies discovery rules. An empty candidate page can still have `has_more=true`. Continue using the returned offset/limit until `has_more=false`. It is not a total candidate count.

Duplicate reports retain at most 100 group summaries and expose `group_count`, `truncated`, and `analyzed_at`. All verified memberships are grouped in SQLite; `/duplicates/groups` pages over them. Each group includes its full `file_count` and at most 1,000 representative paths. Reports describe files at analysis time, not a continuing assertion that content remains unchanged. No duplicate report is an executable cleanup authorization.

Directory totals include descendants. Do not sum an ancestor and its child to estimate total storage. Category totals count file paths; hard links and copy-on-write clones can share actual physical storage. Allocated bytes come from OS block metadata and are not exclusive ownership estimates.

`DirectoryBreakdown` includes both files and directories, with `child_count`, `children_logical_bytes`, `children_allocated_bytes`, and explicit `omitted_*` fields. Its limit is 1–200 (default 60); the displayed children plus omitted logical/allocated bytes exactly cover recorded immediate children. Zero-size children still contribute to counts. This is a consistent SQLite read snapshot, not a live filesystem snapshot.

`StorageExplanation.coverage` lists only published root generations rather than every historical scan. Insights now include additive typed `measurements` for logical bytes, parent share, observation window, growth bytes and daily growth rate where meaningful; unavailable measurements are null. Parent/child insights may overlap and must not be summed into reclaimable storage. Growth anomaly comparisons normalize by elapsed time, skip incomplete scans and collapse same-second observations. Developer findings suppress nested matches beneath an already reported developer directory. Recent-large-file observations are not cleanup candidates merely because of size or age.

Scans and expensive duplicate discovery return `{id,status,result,error}` jobs. Poll the job or subscribe to events. `POST /scans` does not imply the scan has completed. Engine failures appear in the job's stable `error.code`. If the process crashes, reconcile job status with scan/operation records. SSE is not a durable event log.

## Action protocol

```json
{"paths":["/absolute/project/target/debug/example"]}
```

Submit to `POST /cleanup/plans`. Review returned items, evidence, risk, expiry and bytes. The plan's `approval_phrase` is `QUARANTINE <plan-id>`. After explicit approval:

```json
{"approval":"QUARANTINE <plan-id>"}
```

Submit to `POST /cleanup/plans/<plan-id>/execute`. Store the returned operation ID for status and undo. A plan is claimed only once. Repeating execution returns `conflict`; it never expands the selection. A multi-file operation may end `partial`; inspect every item before retrying or undoing. Permission errors, file replacement or expiry require another plan, not a forced override.

Quarantine reclaims no space. To delete what an operation holds, permanently, obtain a second, explicit approval and send

```json
{"approval":"PURGE <operation-id>"}
```

to `POST /cleanup/operations/<operation-id>/purge`. Only files still in quarantine are deleted, each only while it matches the hash and identity recorded when it moved; a file that changed stays as `purge_failed`, and one already removed outside Stratum is reported `missing`. The operation ends `purged`, or `purge_partial` when something is left. A wrong phrase returns `approval_required`; nothing left in quarantine, or a configured `purge_after_hours` not yet elapsed, returns `conflict`. A purge cannot be undone, and restore then skips purged files.

## Errors and bounds

Domain errors return `{code,message}`. Codes include `invalid_request`, `path_not_found`, `permission_denied`, `database_error`, `unsupported_platform_feature`, `scan_cancelled`, `filesystem_changed`, `protected_path`, `invalid_cleanup_plan`, `approval_required`, `busy`, `conflict`, `not_found`, `overlapping_root`. Authentication returns 401, protected/approval failures 403, missing resources 404, conflicts 409, unsupported execution 501. Messages are explanatory; code is the automation contract.

Request bodies are capped at 64 KiB. Plan selection is 1–1,000 paths. Scan request roots are capped at 16 through HTTP. The API initially caps scan lists at 1,000, warning details at 10,000 per scan, application results at 1,000 matching bundles, history at 10,000 observations and system history at 1,000 samples. Underlying warning counts still report omitted details. These are documented bounded views, not completeness guarantees.
