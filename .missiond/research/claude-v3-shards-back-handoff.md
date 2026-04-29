# Claude side-channel: V3 evidence-collector shard

**Branch:** `claude/v3-shards-back` (worktree at `/tmp/missiond-claude`)
**Base:** `fea753b8` (Codex's S1 review-gate commit on main at the time I forked)
**Author:** Claude Opus 4.7 1M

## What happened

I forked off `fea753b8` and started writing checkers from S6 → S5 → S4 (the
inventory's back end), expecting Codex would work S1 → S2 → S3 in parallel.
Codex was much faster than I expected: by the time I finished my 3 checkers,
main was already at `e1849d31` with **5 new V3 surfaces shipped**:

```
fca724ce feat(v3): codify board isomorphism surface             # S2
0ec86d45 feat(v3): add workstation-dispatch isomorphism checker # S3
3d2fd868 feat(v3): codify workstation dispatch surface          # S3 surface
a9a57493 feat(v3): add unified-entry-runtime isomorphism checker# S4 — collided with mine
e1849d31 feat(v3): add file-artifacts isomorphism checker       # S6 — collided with mine
```

Codex's S4 + S6 checkers cover the same ground mine did and pass dry+normal
on its own surface stubs. My S4 + S6 versions added no value over Codex's,
so I dropped them.

**S5 evidence-collector** is the one Codex hasn't done yet — that's the
single shard left on this branch.

## What landed (1 commit)

| Hash | Surface | Anchor file | What it pins |
|------|---------|-------------|--------------|
| `fd8e9ea3` | **evidence-collector** | `crates/missiond-daemon/src/handlers/knowledge/evidence_collector.rs` | `EVIDENCE_SCHEMA_VERSION = "v0"`, `EventRefStatus` + 3 variants + 3 wire strings, `EventRefProvenance` + 4 variants + 4 wire strings, `EVENT_REF_CACHE_CAP = 1024`, the 3 log-query reason constants, `EventRefResolver`, `wrap_legacy_record_evidence`, plan.rs imports it |

`--dry-fixture`: PASSES (6 cases — pass + 5 fail variants).
Normal mode: FAILS until you append `(surface evidence-collector ...)` to
the blueprint.

## Merge guidance

```
git checkout main
git merge --no-ff claude/v3-shards-back
```

After merging, append the surface stub + aggregate gate entries:

### `.missiond/v3/missiond-blueprint.lisp` `(implementation-map ...)`

```lisp
(surface evidence-collector
  :status "code-aligned"
  :implements [verification-receipt]
  :code ["crates/missiond-daemon/src/handlers/knowledge/evidence_collector.rs"
         "crates/missiond-daemon/src/handlers/knowledge/plan.rs"]
  :note "EVIDENCE_SCHEMA_VERSION pins the wire shape; EventRefStatus is the closed enum live | log | unavailable describing whether the ref is live-from-publish, log-recovered post hoc, or simply unavailable. EventRefProvenance further pivots the recovery tier as live | passive_cache | event_log_query | unavailable so consumers can attribute lookups to the wave-16 in-memory passive cache (EVENT_REF_CACHE_CAP = 1024 FIFO entries) vs the wave-18 bounded event_log_query path. wrap_legacy_record_evidence lifts caller-supplied JSON evidence into the typed EvidenceEntry envelope without losing prior fields, so the verification-receipt artifact stays consistent with what plan.rs already wrote.")
```

### `.missiond/v3/missiond-blueprint.lisp` `(compression-contract :checks [...])`

```
"node scripts/check-v3-evidence-collector-isomorphism.mjs"
```

### `scripts/check-v3-code-isomorphism-complete.mjs`

In `EXPECTED_SURFACES`: `'evidence-collector'`
In `PER_SURFACE_CHECKERS`: `'scripts/check-v3-evidence-collector-isomorphism.mjs'`

After your S2/S3/S4/S6 + this S5 are integrated, **6 of 6 inventory shards
will be done**. Aggregate count goes from 13 → 14 graduated surfaces.

## Remaining inventory work

After integration, the only outstanding inventory item is the
`agent_execution.rs` (9.9k LOC) decomposition, which the inventory blockers
section flagged as "single-shard codification is unsafe; deserves a separate
decomposition pass first (probably 2-3 sub-surfaces)". That needs your
architectural call on how to split before it can become V3 surfaces.

**Side note: file-artifacts checker on main is currently red.** Codex's
`e1849d31` shipped the checker but the corresponding `(surface file-artifacts
...)` form isn't in the blueprint yet, so `node scripts/check-v3-file-artifacts-isomorphism.mjs`
fails. That's outside my write scope here, but is presumably your next
follow-up commit (similar pattern to how you've been alternating
checker → surface commits).

## Lesson for future parallel work

The "front + back convergence" pattern only saves time if the front and back
are slow enough that they don't overlap. Codex was knocking out shards every
~5-10 min today; I shipped 3 in roughly the same window and 2 of them collided.
For next round either:

- **Start much further from the middle** (skip 2-3 inventory items between
  what Codex is on and what I take), OR
- **Pick orthogonal work** instead of same-inventory shards — e.g. while Codex
  is in S2..S6, I could attack the agent_execution.rs decomposition design,
  which is locked behind your decision anyway and doesn't ship a checker.

## Worktree cleanup (after merge)

```
git worktree remove /tmp/missiond-claude
git branch -d claude/v3-shards-back
rm -f /private/tmp/semantic-terminal /private/tmp/jarvis-forge
```
