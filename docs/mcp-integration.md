# Future MCP adapter

MCP is a transport, not the engine. An adapter can link `stratum-engine` in Rust or call authenticated HTTP v1 from another language. It does not need to traverse files, parse human CLI output, query SQLite, run shell commands or reimplement any safety rules.

| Future MCP tool/resource | Existing application operation |
| --- | --- |
| `system_summary`, `list_volumes`, `inspect_process` | `Engine::system`, `/system`, `/volumes`, `/processes/{pid}` |
| `analyze_disk` | `Engine::scan`, POST `/scans`, `/jobs/{id}`, `/events` |
| `largest_files`, `largest_directories` | `Engine::files(FileQuery)` with kind/sort/filters |
| `find_duplicates` | `Engine::discover_duplicates`, POST `/duplicates/scan` |
| `list_apps`, `inspect_app` | `Engine::applications`, `inspect_application` |
| `storage_history` | `Engine::history`, GET `/storage/history` |
| `list_insights`, `explain_storage_usage` | `Engine::insights`, `explain_storage` |
| `create_cleanup_plan`, `preview_cleanup_plan` | `Engine::create_cleanup_plan`, `cleanup_plan` |
| `execute_cleanup_plan`, `undo_cleanup` | `Engine::execute_cleanup_plan`, `undo_cleanup` |

## Seven-day question and approved action

1. Call `/storage/explain`, `/storage/history?since=<seven-days-ago>` and `/insights`. These return indexed contributors, observation timestamps, evidence, confidence, risk and coverage. If the oldest observation is newer than seven days, say so. An agent must not invent a week-long baseline.
2. Inspect `/cleanup/candidates` and paginate the underlying file pages. The engine's categories and rules determine eligibility. The agent must not convert a large file or weak app association into a deletion recommendation.
3. Submit a bounded, explicit path list to `/cleanup/plans`. This only creates a plan and performs read-only verification.
4. Present exact selected items, reasons, byte estimates, risk, expiry, and quarantine limitations to the user. In particular, quarantine does not yet reclaim physical disk capacity and irreversible purge is unsupported.
5. Only after explicit user authorization, send the separate execution request with the exact approval phrase. Neither an inspection request nor plan creation authorizes execution.
6. Retain the operation ID. Inspect per-item statuses for partial failures; surface them. Use `/cleanup/operations/{id}/undo` only when restoration is requested. Expose audit records for accountability.

MCP tool schemas can be derived from OpenAPI/domain types. Advertise inspection tools as read-only, planning tools as non-destructive local writes, execution as destructive/reversible where supported, and undo as a state-changing action. The adapter should enforce its own user-consent workflow in addition to engine protections. Possession of the token or approval phrase is not evidence of user intent.

Unsupported operations should return the engine's machine code; an MCP server must not bypass them by issuing filesystem commands. A future richer cleanup executor, platform adapter or history dimension can be added behind the existing service boundaries without moving business logic into MCP.
