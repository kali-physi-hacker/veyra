# Desktop product research and direction

Research date: 2026-09-26. Observations are from primary product documentation, not hands-on competitor benchmarks, customer interviews, or proof of willingness to pay.

## Lessons from the category

| Reference | Observed interaction | Stratum decision |
| --- | --- | --- |
| [CleanMyMac Space Lens](https://macpaw.com/support/cleanmymac/knowledgebase/space-lens-results) | Coordinated map/list, size ordering, breadcrumbs, details and separate selection review | Pair visual exploration with a precise list and inspector; route findings to evidence and review. Do not copy its visual identity. |
| [DaisyDisk overview](https://daisydiskapp.com/guide/disks-overview) | Capacity gauges, explicit scan locations, folder selection and remembered locations | Make scope and freshness visible; offer a small first scan without implying whole-disk coverage. |
| [DaisyDisk hidden space](https://web.daisydiskapp.com/guide/hidden-space) | Explains accounting gaps, permissions and APFS behavior | Never subtract logical file totals from physical volume use and label the remainder junk. |
| [iStat Menus](https://bjango.com/mac/istatmenus/) | Glanceable measurements with drill-down/history and configurable rules | Keep the overview lightweight; collect process details on demand. No synthetic health score or memory-cleaning claims. |
| [GrandPerspective](https://grandperspectiv.sourceforge.net/) | Area-proportional exploration with background work | Bound rendering data and keep navigation responsive during background operations. |

## Product hypothesis

The initial audience is developers and technical Mac users repeatedly asking **what changed, why, and what is worth reviewing**. The purchase hypothesis is saved investigation time and trustworthy historical/developer explanations, not a prettier delete button. Validate with real users before pricing or billing work.

The journey: choose a scope → observe → understand contributors and changes → inspect evidence → review exact files → explicitly authorize a supported action → inspect or undo the result. No “fix everything” control, scare copy, fabricated savings, or automatic preselection.

## Audit of 0.1

1. First-run guidance and native folder selection are missing; technical path controls appear everywhere.
2. The map omits direct files and omitted pages. A folder full of large files can misleadingly look empty.
3. Findings contain raw byte counts, lack direct navigation, and repeat nested dependency folders.
4. Navigation is disabled during work, asynchronous results lack request identity, and progress lacks a dedicated control surface.
5. Categories aggregate every file on every request; developer-name rules repeatedly scan directory lists; overview sampling collects every process unnecessarily.
6. Cleanup is a flat list. Selection, exact review, expiry and restoration need clear separation. Quarantine is not capacity recovery.

## Implementation slice

- Guided first-run, native folder choice, stronger hierarchy, capacity/scope cards and actionable findings.
- Bounded directory breakdown including direct files and an explicit remainder; coordinated map/list, inspector and breadcrumbs.
- Independent background queries and mutation state, stale-response rejection, scan controls and wake-on-completion rendering.
- Transactional category rollups, a directory-name index and summary-only system sampling.
- Deduplicated developer findings with project-share evidence, recent-large-file observations and interval-normalized growth anomaly rules.
- Selection → review → outcome without broadening destructive permissions.

## Commercial readiness gates

Before selling: signing/notarization, accessibility and keyboard review, independent safety review, real-home/external-volume soak tests, crash recovery, measured idle budgets, usability sessions, reliable updates/support, and a separately designed disposal policy if capacity recovery is offered. Do not market quarantined bytes as recovered storage. Broader ownership evidence and historical attribution remain longer-term work.

Usability success means users can identify scan scope, locate a large direct file, explain uncertainty, distinguish indexed bytes from volume capacity, and describe exactly what approval does without coaching. Measure time-to-answer and task errors during consented research; do not add product telemetry.
