# Platform support

| Capability | macOS | Linux | Windows / other |
| --- | --- | --- | --- |
| Streaming metadata scan + SQLite | implemented | implemented | model/adapters compile in principle; unverified |
| Inode, hard links, allocated blocks | Unix metadata | Unix metadata | identity/allocation precision unavailable |
| Native filesystem watcher | notify FSEvents adapter | notify inotify adapter | notify abstraction available; unverified |
| Secure hashing / quarantine / undo / purge | no-follow + rename exclusive; descriptor-relative unlink | no-follow + rename no-replace; descriptor-relative unlink | explicitly unsupported |
| Application footprint | valid .app Info.plist + indexed exact bundle-id associations | indexed macOS bundle fixtures only | native app inventory unsupported |
| CPU/memory/process/volume snapshots | sysinfo | sysinfo | dependency support exists; unverified |
| Memory pressure, APFS exclusive blocks | unavailable | unavailable | unavailable |
| Desktop | native egui/eframe | native egui/eframe | build and package unverified |

macOS privacy permissions and SIP are respected. Stratum does not request root, disable SIP, remove protected OS paths or bypass TCC. Grant Full Disk Access to the invoking terminal/application only if desired. Inaccessible locations remain scan warnings.

APFS clone sharing, compression, snapshots and purgeable storage are not completely attributable through ordinary file metadata. Allocated block counts must not be presented as exclusive physical consumption or promised reclaimable bytes. Finder, `du`, volume statistics and indexed totals can therefore differ legitimately.

Application footprints combine a validated bundle and exact bundle identifier names in scanned Application Support, Caches, Preferences, Logs, Containers, Group Containers and Saved Application State parents. Associations expose their evidence. No launch-history collection exists. Shared resources, extensions, plugins, helper apps and unrelated naming conventions may be missed. No claim of orphaned data is made from these incomplete observations.

Rust target, node_modules, npm, Cargo registry, Gradle, Maven, DerivedData, virtual environment and Git directory patterns produce classifications or observations. Docker/Podman engine accounting, simulator inventories, package-manager ownership databases and network monitoring are not implemented. Do not confuse a folder-name heuristic with a tool's authoritative storage report.
