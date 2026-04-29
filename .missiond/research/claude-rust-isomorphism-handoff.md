# Claude side-channel: Rust isomorphism + infra debt

**Branch:** `claude/rust-isomorphism` (worktree at `/tmp/missiond-claude`)
**Base:** `7f462d17` (the wave51 implementation commit, which is on main)
**Author:** Claude Opus 4.7 1M, side-channel parallel to your wave51 run
**Goal stated by 指挥官:** "先把 Rust 代码同构到 Lisp 蓝图已经设计好的状态,再去调整和把 JS 收回 rust"

I worked in a separate `git worktree` so we never touched the same files
on disk. Wave51's slot-dyn-83abe1da kept writing to
`/Users/jinchen/Projects/missiond` on `main` without interference.

## What landed (3 commits)

| Hash | Subject | Risk |
|------|---------|------|
| `3a2aff21` | fix(db): clear dangling slot-dyn assignees on daemon startup | low — additive trait method + new `Phase 6.8` startup hook |
| `2ea92732` | infra(scripts): add deploy-daemon.sh | zero — new file, no callers |
| `927ecb69` | infra(scripts): add cargo-fmt-touched.sh | zero — new file, no callers |

All three pass `cargo check --workspace`, `node scripts/check-v3-code-isomorphism-complete.mjs`,
`node scripts/check-task-contract.mjs --all`, and `git diff --check`.

## Merge guidance

The branch was based on the same HEAD wave51 landed at, so a `git merge
claude/rust-isomorphism` from `main` should fast-forward or trivially
merge. Recommended order:

```
git checkout main
git pull   # if remote moved
git merge --no-ff claude/rust-isomorphism
```

There are no overlaps with wave51's write-scope (autopilot.rs, V3
blueprint, workstation-config checker, wave51 ledgers). Files I touched:

- `crates/missiond-core/src/db/traits.rs` (+9 lines, additive trait method)
- `crates/missiond-core/src/db/pg/board.rs` (+33 lines, additive impl)
- `crates/missiond-daemon/src/main.rs` (+17 lines, new Phase 6.8 startup block)
- `scripts/deploy-daemon.sh` (new, +200 lines)
- `scripts/cargo-fmt-touched.sh` (new, +107 lines)

After merging, redeploy the daemon (Phase 6.8 only fires at startup):

```
scripts/deploy-daemon.sh
```

## Why each shard

### 1. `clear_dangling_dynamic_slot_assignees` (Rust isomorphism gap)

The blueprint says daemon restart should leave a clean slate for autopilot.
Reality before this commit:

- `Phase 6.4 recover_stale_running_tasks` resets `status='running'` →
  `'open'` and clears the claim block
- but it intentionally **leaves `assignee` populated**
- `Phase 6.7` then terminates active dynamic_slots (sets
  `dynamic_slots.status='terminated'`)
- result: `board_tasks` rows now have `assignee='slot-dyn-XXX'` pointing
  at a slot that no longer exists in any active row
- on next tick, autopilot's per-slot dispatch path keeps trying to send
  to that ghost

You hand-patched this several times across waves (wave47, wave48). The
Lisp blueprint already implies this should be a clean-slate guarantee
but no checker enforced it. The fix is one `NOT EXISTS` UPDATE, gated
behind `slot-dyn-%` so static slot pins are never touched.

**Suggested next-wave backfill** (please pick up): teach
`scripts/check-v3-workstation-config-isomorphism.mjs` to require
`clear_dangling_dynamic_slot_assignees` is invoked from
`main::startup` between Phase 6.7 and the PTY manager init. I did not
touch the V3 blueprint or the workstation checker because wave51 owns
both files; backfilling them after merge is safer than fighting your
write-scope.

### 2. `scripts/deploy-daemon.sh` (infra debt)

Quoted from your own diagnoses: the daemon redeploy loop hit dyld stalls,
self-deleted sockets, `spctl` rejections, and half-active processes that
held the WS port without binding the IPC socket — at least 15 times in
the past month, each costing 5–15 minutes of CLI typing.

The script encodes the safe order with explicit pre/post-conditions:

1. Build (release by default; `--debug` for fast iteration)
2. Hash-compare to skip no-op redeploys
3. Backup current binary with timestamped suffix
4. Atomic `mv` of new binary into place
5. Ad-hoc codesign (`codesign --force --sign -`) to satisfy LaunchAgent
6. Strip `com.apple.quarantine` xattr if present
7. `launchctl kickstart -k gui/$UID/<label>` to restart the service
8. Poll the IPC socket with `lsof` until a `missiond` process owns it
9. Optional smoke: minimal MCP `initialize` round-trip; on failure,
   restore backup and re-kickstart

Env-overridable for non-default installs:

- `MISSIOND_BIN_PATH` (default `~/.xjp-mission/missiond`)
- `MISSIOND_SOCKET_PATH` (default `~/.missiond/missiond.sock`)
- `MISSIOND_LAUNCHCTL_LABEL` (default `com.missiond.daemon`)
- `MISSIOND_DEPLOY_TIMEOUT` (default 30s)

### 3. `scripts/cargo-fmt-touched.sh` (infra debt)

`cargo fmt -p <crate>` and `cargo fmt --all --check` reformat every .rs
under the package, including 100+ historically un-rustfmt'd files. You
hit this rake at least three times (each costing a manual rollback
sweep). This script formats only the .rs files in the current diff.

```
scripts/cargo-fmt-touched.sh                # staged + unstaged
scripts/cargo-fmt-touched.sh --check        # dry-run, exit 1 if dirty
scripts/cargo-fmt-touched.sh --staged       # staged only
scripts/cargo-fmt-touched.sh --branch main  # diff against branch base
```

Auto-detects edition from workspace Cargo.toml.

## What I deliberately did NOT do

- **Did not edit `.missiond/v3/missiond-blueprint.lisp`** — wave51 owns
  it. The blueprint should learn the new Phase 6.8 invariant; please
  add it after merge in your normal Lisp-backfill phase.
- **Did not edit `scripts/check-v3-workstation-config-isomorphism.mjs`** —
  wave51 owns it. Same backfill suggestion as above.
- **Did not write a unit test for `clear_dangling_dynamic_slot_assignees`** —
  the existing pg/board.rs functions don't have unit tests in the file
  (they live behind `cargo test -p missiond-daemon` integration paths
  that need a live PG). I left this for you to integrate however your
  test convention prefers.
- **Did not redeploy the daemon** — wave51 was still running. The Phase
  6.8 cleanup only fires on next daemon startup, so it's safe to ship
  the merged commit at any time; the next `deploy-daemon.sh` run picks
  it up.
- **Did not touch `.mjs` task-runner code** — per 指挥官's guidance:
  Rust first, JS consolidation later.

## Worktree cleanup (when you're done with the merge)

```
# Remove the worktree (does not delete commits — they're on the branch)
git worktree remove /tmp/missiond-claude

# After merge, delete the branch (locally; never pushed to remote):
git branch -d claude/rust-isomorphism

# Optional: delete the symlinks I created so cargo could resolve external
# workspace deps from /private/tmp:
rm /private/tmp/semantic-terminal /private/tmp/jarvis-forge

# Optional: delete the separate target dir I used to avoid stomping on
# your active build cache:
rm -rf /tmp/missiond-claude-target
```

## Rationale: why this work, and why no more from me right now

The V3 aggregate gate reports all 7 surfaces are `code-aligned`. That
checker matrix only enforces what's been declared as a checker — it does
not say "Rust now matches every line of the blueprint." Walking through
the blueprint by hand, the remaining "code drift" is concentrated in:

- **Operational invariants the blueprint asserts but checkers don't yet
  enforce** — e.g. the slot-dyn assignee cleanup we just shipped, plus
  similar things you'll find as you keep extending checkers
- **Compat / legacy paths the blueprint marks for eventual removal** —
  but those are explicitly opt-in today and removing them is a behavior
  change, not a Rust-vs-Lisp drift
- **Pure operational pain the user sees** — daemon redeploy + fmt
  scoping, both shipped here as scripts

I stopped after these three because they were the highest-confidence
items I could ship without risking conflict with wave51 or wave52. The
next batch (V3 checkers gaining new invariants, hooks pre-commit going
default-on, etc.) needs your design judgment more than my
implementation throughput.

If 指挥官 tells me to take another pass, I'll wait until wave51 fully
clears (BoardTask `75852a83-...` → done, slot-dyn-83abe1da → terminated)
so the next branch can diverge from a fully settled main.
