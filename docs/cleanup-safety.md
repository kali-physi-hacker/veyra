# Cleanup safety contract

Inspection does not move or delete data. Candidate discovery produces evidence. Planning hashes an explicit selection and stores an immutable, expiring plan. Execution is a separate operation that requires an exact approval phrase containing the plan ID. No command accepts destructive wildcards. There is no generic delete-path, run-command or execute-SQL API.

Supported actions in 0.1 are quarantine and no-clobber undo for individually selected regular files. Discovery requires either a Cargo target ancestor beside a Cargo.toml file, an npm `_cacache` subtree, or Cargo registry cache. Names such as Downloads, node_modules, caches or target without a Cargo manifest are insufficient for execution. All current candidates are moderate risk: caches may require network access, and generated folders may contain manual changes. A candidate is not a declaration that a file is safe to delete.

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

Only the underlying OS abstraction crate uses platform-specific low-level operations; this project forbids unsafe Rust. The platform abstraction is covered by symlink and no-clobber tests.

## Recovery and boundaries

Multi-file execution is not a filesystem transaction. A failure can leave a partial operation; completed moves remain journaled and reversible. A crash between rename and database completion leaves `moving` intent. Inspect the operation and use undo; the recorded quarantine path allows recovery without guessing. Never rerun a claimed plan. Retain the state database alongside quarantine contents.

Concurrent hostile changes by another process with the same OS user authority cannot be eliminated by a local application. Descriptor anchoring, metadata checks, no-clobber rename and post-move verification reduce race exposure, but there is a remaining check-to-rename interval for the leaf. A racing replacement can be moved, detected and restored or left in review if restoration conflicts. Active writers may retain open descriptors after a move. Stop the producing application/build first. Stratum does not promise an atomic snapshot or immunity to a malicious same-user process.

Quarantine stays under the private state directory. It must be on the same filesystem as a source. It preserves bytes and **does not immediately reclaim capacity**. There is intentionally no automatic purge or retention deletion. A future purge requires its own high-risk plan, explicit authorization and irreversible-action model. Root deletion, bundle uninstallation, recursive cache deletion, duplicate deletion and cross-device quarantine return unsupported/not-candidate errors.

Changing configured allowed roots or protected paths can make an old operation ineligible for automatic undo. Resolve the policy deliberately; never bypass it by passing a raw source/destination to an API. API tokens authorize use but do not prove informed human approval. MCP adapters must present the plan and obtain user authorization before sending the approval phrase.
