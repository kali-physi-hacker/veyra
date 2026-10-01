# Cleanup safety contract

Inspection does not move or delete data. Candidate discovery produces evidence. Planning hashes an explicit selection and stores an immutable, expiring plan. Execution is a separate operation that requires an exact approval phrase containing the plan ID. Purge, the only permanent deletion, is a further operation with its own phrase. No command accepts destructive wildcards. There is no generic delete-path, run-command or execute-SQL API.

Supported actions in 0.1 are quarantine, no-clobber undo, and purge of quarantined files, all for individually selected regular files. Discovery requires either a Cargo target ancestor beside a Cargo.toml file, an npm `_cacache` subtree, or Cargo registry cache. Names such as Downloads, node_modules, caches or target without a Cargo manifest are insufficient for execution. All current candidates are moderate risk: caches may require network access, and generated folders may contain manual changes. A candidate is not a declaration that a file is safe to delete.

## Checks

1. Absolute, non-traversing, indexed paths inside allowed roots.
2. Built-in protected OS locations, configured protections, private Stratum state, `.ssh`, `.gnupg`, `.git`, and Keychains are denied.
3. Only regular files with exactly one hard link; no directory recursion, symlink traversal, special files, or cross-device copies.
4. Metadata identity includes device, inode, size, mtime, ctime and link count. A full BLAKE3 content hash is captured at planning.
5. All items are checked again before the first move. Each item is checked immediately before its move.
6. Ancestor directories are opened one component at a time with no-follow flags. Atomic descriptor-relative rename uses NOREPLACE, so existing destinations are never overwritten.
7. The moved file's identity and bytes are checked. A mismatch triggers an immediate no-clobber restoration attempt and an explicit failed/review state.
8. A durable journal records each `moving` intent before the filesystem action. Parent directories are synchronized following rename, and the database uses FULL synchronous durability.
9. Undo verifies the quarantined object's identity/content and never overwrites an occupied original path.
10. Purge deletes only what an operation holds in quarantine, at destinations directly inside that operation's quarantine folder; it never touches a source location. It requires `PURGE <operation-id>`, which differs from the quarantine phrase, and, when `purge_after_hours` is set, a minimum time in quarantine. Each file is hashed again and must match the content hash, device, inode, size, mtime and single link recorded after its move. The unlink is anchored to the no-follow parent directory and re-checks identity immediately before; the parent is synchronized afterwards. A durable `purging` intent is journaled before the first deletion, and the start, every file and the completion are audited.

Only the underlying OS abstraction crate uses platform-specific low-level operations; this project forbids unsafe Rust. The platform abstraction is covered by symlink and no-clobber tests.

## Recovery and boundaries

Multi-file execution is not a filesystem transaction. A failure can leave a partial operation; completed moves remain journaled and reversible. A crash between rename and database completion leaves `moving` intent. Inspect the operation and use undo; the recorded quarantine path allows recovery without guessing. Never rerun a claimed plan. A crash during a purge leaves `purging` items: run the purge again. An item whose quarantined copy is already gone is then recorded as purged, and the rest are verified and deleted as usual. A copy removed outside Stratum is reported `missing`, and a copy that changed is left in place as `purge_failed`. Retain the state database alongside quarantine contents.

Concurrent hostile changes by another process with the same OS user authority cannot be eliminated by a local application. Descriptor anchoring, metadata checks, no-clobber rename and post-move verification reduce race exposure, but there is a remaining check-to-rename interval for the leaf. A racing replacement can be moved, detected and restored or left in review if restoration conflicts. Active writers may retain open descriptors after a move. Stop the producing application/build first. Stratum does not promise an atomic snapshot or immunity to a malicious same-user process.

Quarantine stays under the private state directory. It must be on the same filesystem as a source. It preserves bytes and **reclaims no capacity until the operation is purged**. There is intentionally no automatic purge or retention deletion: a purge is always an explicit request with its own phrase, and it cannot be undone. The operation itself is the purge's plan: an immutable list of files whose hashes and identities were recorded when they moved, which a purge can only shrink, never extend. Restore skips purged files, and an operation with nothing left in quarantine refuses restore. Root deletion, bundle uninstallation, recursive cache deletion, duplicate deletion and cross-device quarantine return unsupported/not-candidate errors.

Changing configured allowed roots or protected paths can make an old operation ineligible for automatic undo. Resolve the policy deliberately; never bypass it by passing a raw source/destination to an API. API tokens authorize use but do not prove informed human approval. MCP adapters must present the plan and obtain user authorization before sending the approval phrase, and obtain it again, separately, before sending a purge phrase.
