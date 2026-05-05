# M6 Maturity Audit: Dev-Tool Remainder SSOT Batch

- BoardTask: `e556ac09-b330-4902-955e-a417ef97959d`
- Audit date: 2026-05-05
- Scope: `/Users/jinchen/Projects/neural-codegen`, `/Users/jinchen/Projects/semantic-terminal`
- Read posture: project roots, `.missiond/**` intent/manifest/checker/evidence, repo-local checker scripts, and MissionD project-universe checker evidence.
- Excluded: KB, historical conversations, provider durable logs, Board backlog, nightly/self-evolution, production probes, Cloudflare, Kubernetes/live ops.

## Verdict

`neural-codegen` is M6 for the dev-tool SSOT maturity contract.

`semantic-terminal` initially audited as M5 / M6-blocked by checker self-consistency: it had an M6-shaped SSOT, manifest, code mapping, runtime projection, package map, and test anchors, but its own static `ssot-write-scope` gate rejected the existing `.missiond/evidence/m6-convergence-report.md` file.

Post-audit remediation is complete. Child BoardTask `dd7870f0-bbbc-4f85-a38e-046925d2f43e` delegated implementation to ClaudeCode task `3dafcf87-f2e4-44e9-8000-95139213d98a`, which landed semantic-terminal commit `b45f712` (`fix(semantic-terminal): allow .missiond/evidence/ in M6 SSOT write-scope gate`). After that commit, `semantic-terminal` also classifies as M6.

## Classification Matrix

| Project | Maturity | Intent/index | Backend/frontend blueprint | Entry/core/egress | Surfaces | Runtime projection | Current-code mapping | Local checker/build evidence | Missing M6 items |
|---|---:|---|---|---|---|---|---|---|---|
| `neural-codegen` | M6 | `.missiond/intent.lisp` plus `.missiond/intent-manifest.lisp`; overlay+manifest mode | Not applicable: Rust codegen pipeline, not a service/UI split | Present per pillar in `intent.lisp` | Parser, IR, deterministic codegen, examples, verification harness, docs, workspace, source hygiene | Present per pillar, including dependency, failure, purity, network, and dirty-baseline projections | Manifest shard index maps pillars to `crates/**`, examples, docs, root scripts, Cargo files | `bash .missiond/check.sh` exited 0 in default static mode; cargo-test gate exists but is skipped unless `--with-rust` | None blocking |
| `semantic-terminal` | M6 | `.missiond/intent.lisp` plus `.missiond/intent-manifest.lisp`; `:ssot-version "M6.0"` | Not applicable: Rust core plus N-API/npm package, no frontend | Present per parser/package/test/hygiene function | Rust traits/classes, N-API classes/functions, npm loader/packages, test fixtures | Present for PTY inputs, provider patterns, N-API build, npm platform resolution, source hygiene | Manifest evidence map anchors every function to `crates/**` and `packages/**`; LOC and test markers match on disk | After `b45f712`, full `bash .missiond/check.sh` passes: 110 Rust tests, Node smoke, static gates, and evidence allowlist | None blocking |

## Evidence

### `neural-codegen`

Reviewed files:
- `.missiond/intent.lisp`
- `.missiond/intent-manifest.lisp`
- `.missiond/evidence/m6-convergence-report.md`
- `.missiond/check.sh`
- `README.md`, `Cargo.toml`, `verify.sh`
- `crates/nc-parser/src/lib.rs`
- `crates/nc-ir/src/lib.rs`
- `crates/nc-codegen/src/lib.rs`

M6 basis:
- Intent declares seven implementation/meta pillars plus source hygiene, each with entry/core/egress/surfaces/runtime-projection sections.
- Manifest provides shard index, maturity matrix, evidence map, checker plan, dirty baseline, and next gaps.
- Current code matches the core claim: `nc-parser` exposes S-expression parsing, `nc-ir` exposes whitelist enums and `ApiEndpoint::from_sexp`, and `nc-codegen` exposes pure deterministic `generate(endpoints)`.
- The verification harness is explicit: `verify.sh` builds the example, runs workspace tests, emits generated Rust, and `cargo check`s it inside a temporary project. The audit did not run this heavier build path; the default checker explicitly skips `cargo-test` unless `--with-rust`.

Checker result:

```sh
cd /Users/jinchen/Projects/neural-codegen
bash .missiond/check.sh
```

Result: exit 0. Gates passed: `scope.write-set`, `baseline.untouched`, `no-format`, `no-global-add`, `whitespace.clean`, `commit.message`, `rustfmt-touched` by vacuity, and `cargo-test` skipped by policy.

Non-blocking gaps already recorded by the project include template expansion, LLM retry loop wiring, benchmark corpus broadening, CI workflow, reality snapshot, docs shard decomposition, and stronger policy enforcement. These are not missing M6 structure/checker items for the current maturity contract.

### `semantic-terminal`

Reviewed files:
- `.missiond/intent.lisp`
- `.missiond/intent-manifest.lisp`
- `.missiond/evidence/m6-convergence-report.md`
- `.missiond/check.sh`
- `README.md`, `Cargo.toml`
- `crates/semantic-terminal/src/{lib.rs,types.rs,state.rs,gemini_state.rs,confirm.rs,status.rs,title.rs,tool.rs,fingerprint.rs,patterns.rs}`
- `crates/semantic-terminal-napi/src/lib.rs`
- `packages/semantic-terminal/{index.js,index.mjs,index.d.ts,package.json,test.js}`
- platform package `package.json` files under `packages/semantic-terminal-*`

Initial M5/M6-blocked basis:
- Intent carries the full M6 function shape for state detection, confirmation parsing, status/title parsing, tool output parsing, fingerprint registry, provider patterns, N-API binding, npm packaging, tests, and source hygiene.
- Manifest provides shard index, maturity matrix, evidence map, checker plan, package map, next gaps, and closure rules.
- Code anchors are real. Spot checks matched manifest LOC claims: `state.rs` 1002 lines, `gemini_state.rs` 277, `confirm.rs` 512, `status.rs` 406, `title.rs` 277, `tool.rs` 558, `fingerprint.rs` 667, `patterns.rs` 801, N-API `lib.rs` 572.
- Test-marker count matches the reported 110 Rust tests across the eight test-bearing modules.
- Package surface is real: main package declares five optional platform triples, `index.js` resolves `.node` artifacts by platform/arch, and only `semantic-terminal.darwin-arm64.node` is checked into the main package, matching the manifest gap.

Checker results:

```sh
cd /Users/jinchen/Projects/semantic-terminal
bash .missiond/check.sh --dry-run
```

Result: exit 0. This confirms the checker is discoverable and describes six gates.

```sh
cd /Users/jinchen/Projects/semantic-terminal
bash .missiond/check.sh --skip-rust --skip-node
```

Result: exit 1. Static gates `ssot-clean-diff`, `forbid-source-mutation`, and `forbid-prebuilt-touch` passed, but `ssot-write-scope` failed:

```text
unexpected path under .missiond/: .missiond/evidence/m6-convergence-report.md
FAIL gates: ssot-write-scope
```

This is a real M6 blocker because the evidence report is part of the project-local M6 evidence, while the checker allowlist only accepts `intent.lisp`, `intent-manifest.lisp`, and `check.sh`.

Remediation:

```sh
cd /Users/jinchen/Projects/semantic-terminal
git show --stat --oneline b45f712
bash .missiond/check.sh --skip-rust --skip-node
bash .missiond/check.sh --dry-run
bash .missiond/check.sh
git diff --check
```

Result after remediation: all commands exited 0. Full checker output included `110 passed; 0 failed` for `cargo test -p semantic-terminal` and successful `node packages/semantic-terminal/test.js`. The only remaining worktree entry is pre-existing untracked `.claude/`, outside this task's declared write scope.

## MissionD Universe Checker

Reviewed `scripts/check-project-ssot-universe.mjs` and the V3 project registry entries. The registry points to:
- `neural-codegen`: `/Users/jinchen/Projects/neural-codegen`, check `bash .missiond/check.sh --dry-run`
- `semantic-terminal`: `/Users/jinchen/Projects/semantic-terminal`, check `bash .missiond/check.sh --dry-run`

Live command:

```sh
cd /Users/jinchen/Projects/missiond
node scripts/check-project-ssot-universe.mjs --json
```

Result: exit 0, `ok=true`, `diagnostics=[]`. Both target entries were green at the universe level. This is weaker than the target-local static run because the universe checker intentionally invokes `--dry-run` for both projects.

## Required Backfill

Created child BoardTask `dd7870f0-bbbc-4f85-a38e-046925d2f43e` for `semantic-terminal` to reconcile `.missiond/check.sh` with the committed M6 evidence layout. Acceptance required:
- `bash .missiond/check.sh --skip-rust --skip-node` exits 0.
- `bash .missiond/check.sh --dry-run` still documents the same effective gates.
- No source/package/prebuilt artifacts under `crates/**`, `packages/**`, `Cargo.toml`, or `Cargo.lock` are touched.
- The fix is limited to `.missiond/check.sh` and, only if needed, `.missiond/intent-manifest.lisp` / `.missiond/evidence/m6-convergence-report.md`.

Status: complete via ClaudeCode implementation task `3dafcf87-f2e4-44e9-8000-95139213d98a`, commit `b45f712`.

## Decision

Final decision: both `neural-codegen` and `semantic-terminal` are M6 after remediation. The parent audit can be closed once BoardTask notes record the child commit and acceptance evidence.
