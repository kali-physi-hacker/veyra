# Privacy and security

Stratum is local only. It contains no analytics SDK, external API client, cloud synchronization, account system, LLM dependency or upload feature. Filenames, paths, contents, process observations and application inventory stay on the machine. Dependency download/build tools may use the network during installation; the running product does not contact external services.

The dedicated state directory is mode 0700 on Unix. The HTTP token is a random 256+ bit capability written with create-new semantics and mode 0600. All HTTP routes require it. Bearer comparisons use constant-time equality. Only loopback binds are permitted. Browser-origin requests are denied, CORS is not enabled, bodies are bounded and no cookie authentication exists. Do not expose the loopback server through a reverse proxy or share its token.

This is a single-user application. Processes running under that same account can generally read that account's state and may bypass application policies outside Stratum. Local bearer authentication protects against unauthenticated callers, not against a fully compromised user session. The service grants broad inspection capabilities; token holders can read indexed private paths and resource snapshots.

Path queries use prepared parameters. File names are never interpolated into shell commands. No product operation spawns a shell. Cleanup uses explicit immutable selections, no-follow opens, atomic no-clobber rename, hashes, deny policies, bounded plan size, expiry, and durable per-item audit. Permanent deletion is limited to quarantined copies, needs a phrase separate from the quarantine approval, and re-verifies each file before a descriptor-anchored unlink. Detailed destructive-operation limitations are in [cleanup safety](cleanup-safety.md).

Partial scans, errors, unsupported platforms and absent baselines remain visible. Heuristic application relationships are labeled as estimates. The engine never infers unused software from mtime, and never infers disposable files from size or age alone. Memory pressure and unavailable platform measurements remain null with explanatory limitations.

State is not encrypted by Stratum. Use FileVault or equivalent disk encryption for at-rest protection. API traffic is plaintext over loopback. Diagnostic logging goes to stderr; normal CLI JSON goes to stdout. Logs and audit records may themselves contain private paths, so protect them as machine inventory.

Report vulnerabilities privately to the project's eventual maintainer before publishing reproducers involving user data. There is no configured remote support/telemetry endpoint in this initial project.
