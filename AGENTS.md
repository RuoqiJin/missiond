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
