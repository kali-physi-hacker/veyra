# Synthetic fixtures

`cargo run -p stratum-engine --example fixture -- NEW_DIRECTORY` creates a clearly labeled, self-contained workload with Cargo artifacts, source, dependencies, a valid macOS bundle, associated cache, package cache, duplicate archives, hard links, a symlink, documents and sparse build files. Existing directories are rejected; nothing is overwritten.

The benchmark generates scalable pairs at 10k, 100k, 1m or 3m files via `STRATUM_BENCH_FILES`. Integration tests cover deep traversal, permission failures, sparse files and concurrent changes in private temporary roots. Generated content is never represented as observations of the user's real machine.
