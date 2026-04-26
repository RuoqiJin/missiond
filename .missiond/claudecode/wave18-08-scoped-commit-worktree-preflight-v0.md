# Wave 18 Task 08 — Scoped Commit Worktree Preflight v0

## Goal

Add a worktree-level preflight helper for scoped commit enforcement.

Wave16/17 enforce caller-reported scoped commit metadata and make workstation briefs require it. This task adds an explicit preflight action/helper that compares actual git status against claimed scope before the worker commits.

The daemon may inspect git status, but must not stage, commit, reset, or checkout.

## Ownership

Expected files:

- `crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs`
- `crates/missiond-mcp/src/tools/knowledge/agent_execution.rs`

Optional new helper module if cleaner:

- `crates/missiond-daemon/src/handlers/knowledge/scoped_commit.rs`

Do not modify workstation dispatch unless only schema wording references the new preflight.

## Requirements

1. Add an action or sub-action:

   - preferred: `mission_execution(action="preflight_commit")`
   - inputs: `execution_id`, `claim_id` optional, `project`, `cwd`, `expected_files` optional

2. Behavior:

   - run read-only git status/diff name-only under resolved project root
   - compare changed/staged files against active/released claim scope
   - return structured result:
     - `ok`
     - `changed_files`
     - `staged_files`
     - `out_of_scope_files`
     - `next_step`

3. Safety:

   - no git add/commit/reset/checkout
   - reject unresolved project root
   - reject paths outside project root

4. Compatibility:

   - existing mission_execution actions unchanged

5. Workstation brief can mention the preflight action if already convenient, but do not require editing it.

## Tests

Add pure tests for:

- git porcelain parser
- scope comparison
- out-of-scope detection
- no-claim rejection
- clean worktree ok

Add integration-ish temp repo test only if existing test utilities make it cheap.

## Acceptance Commands

```bash
cargo test -p missiond-daemon handlers::knowledge::agent_execution::tests
cargo test -p missiond-daemon
cargo test -p missiond-mcp --lib
cargo build --workspace
node scripts/check-architecture-lisp.mjs --all-v2
git diff --check
```

## Commit

```bash
git add crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs \
        crates/missiond-daemon/src/handlers/knowledge/scoped_commit.rs \
        crates/missiond-mcp/src/tools/knowledge/agent_execution.rs
git commit -m "feat(execution): preflight scoped commit worktree state"
```

Only stage files actually modified.

## Report

Return:

- Commit hash.
- Preflight action contract.
- Proof no mutating git commands are used.
- Tests and acceptance results.
