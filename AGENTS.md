# Agent Instructions

## MissionD Architecture Read Order

For MissionD architecture, design, implementation, or review work, treat the V3
SSOT Lisp as the primary architecture entrypoint. Read in this order:

1. `.missiond/v3/missiond-blueprint.lisp`
2. `.missiond/v3/shards/index.lisp`
3. Relevant active-authoring shards under `.missiond/v3/shards/**`
4. Relevant compiled runtime projections and ABI under `.missiond/v3/runtime/compiled/**`
5. Rust/TypeScript implementation
6. README, operator manual, and module catalog as supporting references

Do not infer MissionD design authority from Rust/TypeScript implementation alone
when a V3 contract exists. Compare implementation against the V3 SSOT and its
checkers before making architecture recommendations.

Runtime hot paths should consume compiled JSON/runtime ABI rather than raw Lisp
unless an explicit dev/fallback path is being inspected.

## Codex App Context Pack Bootstrap

When working in the Codex App without MissionD-controlled prompt/tool injection,
start every non-trivial MissionD design, debug, frontend, provider, Codex,
conversation, or architecture task by pulling a compact context pack:

```bash
node scripts/mission-context-pack.mjs --json --message "$CODEX_USER_REQUEST"
```

If the latest user message is not available in `CODEX_USER_REQUEST`, pass it
directly with `--message`. Use the returned `intent_candidates`,
`suggested_first_reads`, `evidence_lanes`, and `avoid_first_reads` before broad
repository search. This bootstrap is deterministic and intentionally compact; it
does not preload raw conversation history, provider logs, cold runtime archives,
or unreviewed KB dumps.

When MissionD MCP/runtime tools are available, the bootstrap may point to
`mission_context_boot` or `mission_context_gather` for live evidence. Treat those
as scoped follow-up retrieval surfaces, not as a replacement for the V3 SSOT read
order above.
