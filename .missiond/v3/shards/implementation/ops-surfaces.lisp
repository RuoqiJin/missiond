(implementation-map
(surface typed-lisp-compiler
      :status "code-aligned"
      :implements [lisp-reader typed-ast semantic-validator diagnostic-json projection-json semantic-ir-json structured-runtime-config-json structured-project-universe-json structured-workflow-contract-json workflow-directory-structural-gate project-directory-structural-gate workstation-config-structural-gate project-m6-depth-gate runtime-compiled-json-loader auth-domain-sample]
      :code ["tools/missiond_lispc/dune-project"
             "tools/missiond_lispc/bin/dune"
             "tools/missiond_lispc/bin/main.ml"
             "tools/missiond_lispc/bin/ast.ml"
             "tools/missiond_lispc/bin/parser.ml"
             "tools/missiond_lispc/bin/schema_v3.ml"
             "tools/missiond_lispc/bin/workflow_schema.ml"
             "tools/missiond_lispc/bin/project_schema.ml"
             "tools/missiond_lispc/bin/workstation_schema.ml"
             "tools/missiond_lispc/bin/emit_json.ml"
             "tools/missiond_lispc/test/dune"
             "tools/missiond_lispc/test/parser_golden.ml"
             "scripts/lib/ocaml_lispc.mjs"
             "scripts/lib/v3_compiled_contract.mjs"
             "scripts/check-ocaml-toolchain.mjs"
             "scripts/check-typed-lisp-compiler.mjs"
             "scripts/compile-v3-runtime.mjs"
             "scripts/project-v3-contracts.mjs"
             "scripts/generated/v3_contracts.mjs"
             "scripts/generated/v3_contracts.d.ts"
             "scripts/check-auth-domain-ssot.mjs"
             "scripts/check-project-domain-hardening.mjs"
             "crates/missiond-daemon/src/context/v3_contracts/mod.rs"
             "crates/missiond-daemon/src/context/v3_contracts/generated.rs"
             "crates/missiond-daemon/src/context/v3_blueprint_runtime.rs"
             ".missiond/workflows/typed-lisp-compiler-convergence.lisp"]
      :note "Lisp remains the canonical authoring SSOT. OCaml is the dev-time typed compiler/checker/projection layer for diagnostics, generated runtime JSON, generated V3 contract ABI, and typed plan-contract projection. compiled-contract-abi.json plus scripts/project-v3-contracts.mjs generate Rust/JS/TS readers; compiled-runtime-config.json carries runtime policy, project universe, workflow, maturity, and M6-depth projections. OCaml is not in the daemon hot path. JS checkers remain compatibility/code-anchor validators, but live surface/function facts load through scripts/lib/v3_compiled_contract.mjs from missiond-lispc emit-v3 / emit-semantic-ir / emit-contract-abi instead of hand-maintained lists.")

(surface semantic-ir-compiler
      :status "code-aligned"
      :implements [semantic-ir-json compact-agent-slices source-map-diagnostics compiled-workflow-contracts]
      :code ["tools/missiond_lispc/bin/emit_json.ml"
             "tools/missiond_lispc/bin/main.ml"
             "scripts/compile-v3-runtime.mjs"
             "scripts/check-v3-shared-memory-isomorphism.mjs"]
      :note "The semantic IR compiler is the compact projection layer between human/agent Lisp SSOT and worker context slices. It emits typed facts with short ids and source maps into compiled-semantic-ir.json, derives compiled-agent-slices.json for agents, and keeps compiled-workflow-contracts.json aligned with workflow Lisp. Generated JSON is machine-oriented and never hand-authored.")

(surface source-hygiene
      :status "code-aligned"
      :implements [source-hygiene scoped-write-gate ssot-retrieval-scope]
      :code ["scripts/check-staged-source-hygiene.mjs"
             "scripts/task-scope-guard.mjs"
             "scripts/check-missiond-hooks.mjs"
             "scripts/install-missiond-hooks.mjs"
             ".githooks/pre-commit"
             ".ignore"
             ".missiond/.ignore"
             ".missiond/v3/.ignore"
             ".missiond/research/.ignore"
             ".missiond/tasks/.ignore"
             "scripts/verify-task-runner-batch.mjs"
             "scripts/check-v3-source-hygiene-isomorphism.mjs"
             "scripts/check-v3-runtime-path-hygiene.mjs"]
      :note "check-staged-source-hygiene.mjs is the read-only staged/source preflight: it rejects raw NUL bytes, runs git diff --cached --check, and delegates task write-scope checks to task-scope-guard.mjs. .githooks/pre-commit runs missiond-work-order verify --staged so code-like staged files require MissionD-Work-Order coverage. check-missiond-hooks.mjs is read-only; install-missiond-hooks.mjs is the only mutating hook installer. verify-task-runner-batch imports checkSuppliedFiles for fixture coverage. ssot-retrieval-scope keeps broad review/search on active Lisp and treats .missiond/v3/runtime/**, .missiond/tasks/wave*/**, memory-review, board-cleanup, and KB triage exports as cold diagnostic evidence unless include_runtime=true, --no-ignore, or a concrete trace path is requested. Repo-local .ignore sidecars mirror that boundary for searches rooted at repo, .missiond, .missiond/v3, .missiond/research, or .missiond/tasks. Production deploy writes projections under MISSIOND_RUNTIME_DIR; longer rationale lives in blueprint note evidence.")

(surface lisp-code-drift-policy
      :status "code-aligned"
      :implements [lisp-code-drift]
      :code [".missiond/v3/missiond-blueprint.lisp"
             "crates/missiond-daemon/src/engine/master_control.rs"
             "crates/missiond-daemon/src/handlers/knowledge/board/update.rs"
             "scripts/check-v3-direct-code-drift-policy.mjs"
             "scripts/check-v3-code-isomorphism-complete.mjs"]
      :note "lisp-code-drift-policy is the governance surface for code-first exceptions. Normal behavior changes must carry a same-task Lisp/checker delta or map to an already pinned surface. Emergency code-first fixes are allowed only with waiver metadata and must immediately create a visible backfill BoardTask that adds the missing blueprint, checker, and evidence. The runtime close gate in mission_board_update/mission_board_batch_update/mission_board_toggle blocks status=done while unresolved code-first drift exists, so code-first work cannot be closed without Lisp/checker/evidence convergence.")

(surface eventbridge
      :status "code-aligned"
      :implements [eventbridge-policy deployment-event-ingest deploy-agent-self-update-governance]
      :code ["crates/missiond-core/src/ws/server.rs"
             "crates/missiond-core/src/event/events/system.rs"
             "crates/missiond-daemon/src/bus/bootstrap.rs"
             "crates/missiond-daemon/src/bus/v2_subscribers.rs"
             "crates/missiond-daemon/src/handlers/comm/timeline.rs"
             "crates/missiond-mcp/src/tools/comm/timeline.rs"
             ".missiond/workflows/deployment-event-response.lisp"
             "scripts/check-v3-eventbridge-isomorphism.mjs"]
      :note "MissionD local EventBridge accepts missiond.event-envelope.v1 cloud/service events, preserves project/correlation metadata under ExternalServiceEvent payload._envelope, dedupes by service/event id, and exposes EventBus waits for deployment workflows. deploy-center remains deployment fact authority; MissionD caches, displays, and triggers Board workflows only.")

(surface board-frontend
      :status "code-aligned"
      :implements [board-frontend]
      :code [".missiond/frontend/board-blueprint.lisp"
             ".missiond/frontend/evidence/board-blueprint-notes.lisp"
             ".missiond/frontend/evidence/board-frontend-convergence-report.lisp"
             "packages/board/src/generated/board-frontend-config.ts"
             "packages/board/src/App.tsx"
             "packages/board/src/types.ts"
             "packages/board/src/api.ts"
             "packages/board/src/store.ts"
             "packages/board/src/eventStream.ts"
             "packages/board/src/lib/missiond.ts"
             "packages/board/src/components/Terminal.tsx"
             "packages/board/src/components/TaskDialog.tsx"
             "packages/board/src/components/timeline/constants.tsx"
             "packages/board/src/components/timeline/helpers.ts"
             "packages/board/src/app/api/slots/route.ts"
             "scripts/project-frontend-board-config.mjs"
             "scripts/check-frontend-board-lisp-schema.mjs"
             "scripts/check-frontend-board-code-isomorphism.mjs"
             "scripts/check-frontend-board-runtime-projection.mjs"]
      :note "Board frontend is now a project-local Lisp SSOT registered from V3: .missiond/frontend/board-blueprint.lisp owns app-shell, MissionD proxy, BoardTask UI, workstation terminal, event stream, timeline/log, knowledge/system, and design-system pillars. The frontend checkers pin the same entry/core/egress/function structure as backend V3 while keeping the backend blueprint compact for the later 20+ project pattern. Runtime workstation/PTY identity must project through mission_slots + mission_pty_status; static frontend slot pools are forbidden.")

(surface compute-primitives
      :status "code-aligned"
      :implements [pty task job flow-run process cc forge worker-control]
      :code ["crates/missiond-daemon/src/handlers/mod.rs"
             "crates/missiond-daemon/src/handlers/compute/mod.rs"
             "crates/missiond-daemon/src/handlers/compute/task.rs"
             "crates/missiond-daemon/src/handlers/compute/job.rs"
             "crates/missiond-daemon/src/handlers/compute/flow_run.rs"
             "crates/missiond-daemon/src/engine/flow/mod.rs"
             "crates/missiond-daemon/src/engine/flow/loader.rs"
             "crates/missiond-daemon/src/handlers/compute/pty.rs"
             "crates/missiond-pty/src/pty_recognition.rs"
             "crates/missiond-pty/src/session.rs"
             "crates/missiond-pty/src/manager.rs"
             "crates/missiond-daemon/src/handlers/compute/process.rs"
             "crates/missiond-daemon/src/handlers/compute/slot.rs"
             "crates/missiond-daemon/src/handlers/compute/minimax.rs"
             "crates/missiond-daemon/src/llm/minimax_client.rs"
             "crates/missiond-daemon/src/llm/minimax_gateway.rs"
             "crates/missiond-daemon/src/handlers/compute/cc_tasks.rs"
             "crates/missiond-daemon/src/handlers/compute/worker.rs"
             "crates/missiond-daemon/src/handlers/compute/forge.rs"
             "crates/missiond-daemon/src/context/v3_blueprint_runtime.rs"
             "crates/missiond-mcp/src/tools/compute/task.rs"
             "crates/missiond-mcp/src/tools/compute/job.rs"
             "crates/missiond-mcp/src/tools/compute/flow_run.rs"
             "crates/missiond-mcp/src/tools/compute/pty.rs"
             "crates/missiond-mcp/src/tools/compute/process.rs"
             "crates/missiond-mcp/src/tools/compute/slot.rs"
             "crates/missiond-mcp/src/tools/compute/minimax.rs"
             "crates/missiond-mcp/src/tools/compute/cc_tasks.rs"
             "crates/missiond-mcp/src/tools/compute/worker.rs"
             "crates/missiond-mcp/src/tools/compute/forge.rs"
             "scripts/check-v3-compute-primitives-isomorphism.mjs"
             "scripts/check-v3-pty-recognition-isomorphism.mjs"]
      :note "Code-aligned V3 destination for low-level worker runtime primitives. task.rs owns mission_task_submit/query/cancel plus async/sync/status/list/ack/track and TaskEvent::Created egress; mission_task_query bridges legacy tasks with BoardTask-backed delegated workers so running BoardTasks are visible to master/control callers; task.rs projects auto-spawn tracked PTY wait_for_idle timeout from compute-runtime-policy; job.rs owns mission_job_poll poll/list/cancel over AsyncJobStatus; flow_run.rs owns mission_flow_run BoardTask-backed flow execution and project-root resolution; engine/flow/mod.rs owns FlowDefinition shape constants, engine/flow/loader.rs loads flow-runtime-policy through context/v3_blueprint_runtime.rs and projects missing YAML node defaults while preserving explicit fields; pty.rs owns mission_pty_spawn/send/read/signal/confirm/status/screenshot plus kill/interrupt/read screen-history-logs, task requeue, and permission learning; process.rs owns mission_agent spawn/kill/restart/list and projects trac..."
      :evidence-sidecar ".missiond/v3/evidence/blueprint-notes.lisp#note-019")

(surface sysinfra-control
      :status "code-aligned"
      :implements [sysinfra permission power daemon-update global-instruction]
      :code ["crates/missiond-daemon/src/handlers/mod.rs"
             "crates/missiond-daemon/src/handlers/sysinfra/mod.rs"
             "crates/missiond-daemon/src/handlers/sysinfra/infra.rs"
             "crates/missiond-daemon/src/handlers/sysinfra/permission.rs"
             "crates/missiond-daemon/src/handlers/sysinfra/power.rs"
             "crates/missiond-daemon/src/handlers/sysinfra/system.rs"
             "crates/missiond-daemon/src/handlers/sysinfra/global_instruction.rs"
             "crates/missiond-mcp/src/tools/sysinfra/infra.rs"
             "crates/missiond-mcp/src/tools/sysinfra/permission.rs"
             "crates/missiond-mcp/src/tools/sysinfra/power.rs"
             "crates/missiond-mcp/src/tools/sysinfra/system.rs"
             "crates/missiond-mcp/src/tools/sysinfra/global_instruction.rs"
             "scripts/check-v3-sysinfra-control-isomorphism.mjs"
             "scripts/check-v3-infrastructure-universe-isomorphism.mjs"]
      :note "Code-aligned V3 sysinfra surface. infra.rs owns infra query/ops, skill evidence, credential refs, and runtime target projection; project/reconcile reports runtime and credential drift. permission, power, system, and global-instruction handlers own their MCP tools. ClaudeCode instruction and MCP discovery projections should remind ClaudeCode workstations to prefer missiond-mcp/xjp-mcp, using missiond-cli/xjp-cli only for gap-fill or diagnostics. Learned permissions are scoped, non-blanket for Bash, TTL-governed with expires_at/source_evidence/renew_policy/audit_trail, and renewed only from provider-confirmation use. Long blue-green/self-update and infra-evidence anchors live in blueprint-notes#note-021.")

(surface runtime-load-explanation
      :status "code-aligned"
      :implements [runtime-load-explanation runtimeLoadExplanation]
      :code ["crates/missiond-daemon/src/engine/master_control.rs"
             "crates/missiond-daemon/src/infra/daemon_stats.rs"
             "crates/missiond-daemon/src/engine/lisp_code_sync.rs"
             "crates/missiond-daemon/src/engine/shared_memory.rs"
             ".missiond/v3/missiond-blueprint.lisp"
             "scripts/check-v3-lisp-code-sync-isomorphism.mjs"]
      :note "runtime-load-explanation is the operator-facing explanation layer for MissionD internal load. It does not pretend to replace OS CPU sampling; it combines daemon stats, lisp-code-sync report counters, shared-memory workflow/cursor/claim counters, and nightly-evolution counters into runtimeLoadExplanation suspects so the Board and master can distinguish lisp-code-sync, EventBus backlog, workflow runner/shared-memory, context-prefetch, Autopilot/DB, or nightly-evolution activity before asking the user to decide.")

(surface ops-infra
      :status "code-aligned"
      :implements [ops-infra]
      :code ["scripts/deploy-daemon.sh"
             "scripts/bootstrap-managed-mac-node.sh"
             "scripts/rustfmt-missiond.sh"
             "scripts/cargo-fmt-touched.sh"
             "crates/missiond-daemon/src/main.rs"
             "crates/missiond-daemon/src/workers/local/ast_sync_worker.rs"
             "scripts/check-v3-ops-infra-isomorphism.mjs"
             "scripts/check-missiond-blue-green-deploy.mjs"]
      :note "ops-infra owns deploy-daemon.sh, bootstrap-managed-mac-node.sh, formatter-converged Rust hygiene, and restart-time background CPU policy. bootstrap-managed-mac-node.sh is the default managed Mac node bootstrap action: install Homebrew when absent, install/link libpq, add Homebrew/libpq to the managed shell PATH, and verify psql. deploy-daemon.sh builds paired missiond/mission-mcp releases under ~/.xjp-mission/releases/<release-id>, writes release-manifest.json, records release_owner_root for ownership checks, rejects cross-project-root active mutation unless MISSIOND_DEPLOY_ALLOW_PROJECT_ROOT_TAKEOVER=1 is explicitly set against MISSIOND_DEPLOY_EXPECTED_ACTIVE_ROOT, switches active, writes a release-local source snapshot, rewrites launchd WorkingDirectory plus MISSIOND_PROJECT_ROOT/MISSIOND_ORCHESTRATOR_ROOT to that immutable snapshot, reloads launchd through bootout/bootstrap before kickstart, runs MCP smoke, rolls back, cleans releases, discovers managed-node Node.js/OCaml/Homebrew libpq Postgres client paths, and defaults CARGO_INCREMENTAL=0 for disk-bounded deploys. Managed Mac node bootstrap must install Homebrew plus libpq/psql before the node is considered operational, because psql is the standard Postgres diagnostic and backfill client for Board runtime metadata and interaction-ledger repair. rustfmt-missiond.sh is the M6 formatter gate; cargo-fmt-touched.sh remains a scoped fallback. AST startup full sync is opt-in, and ast_sync_worker skips topology KB rewrites when no stale files were synced.")

(surface missiond-blue-green-self-update
      :status "code-aligned"
      :implements [blue-green-self-update release-manifest release-cleanup rollback]
      :code ["scripts/deploy-daemon.sh"
             "scripts/check-missiond-blue-green-deploy.mjs"
             "scripts/check-v3-ops-infra-isomorphism.mjs"
             "scripts/check-v3-sysinfra-control-isomorphism.mjs"]
      :note "MissionD self-update is owned as a blue-green release workflow. Release candidates are immutable directories under ~/.xjp-mission/releases/<release-id>; the active symlink is the only switch; daemon and MCP entrypoints both resolve through active so they share one release-manifest.json. The deploy path separates release_owner_root from launchd_project_root: owner root gates active mutation, while launchd_project_root points at the release-local source snapshot that matches compiled runtime source_units. Active mutation is rejected when the active release owner or legacy launchd root belongs to a different project root unless MISSIOND_DEPLOY_ALLOW_PROJECT_ROOT_TAKEOVER=1 is explicitly set, generated V3 contract ABI is verified with node scripts/project-v3-contracts.mjs --check --json, it may refresh only under explicit MISSIOND_DEPLOY_REFRESH_CONTRACTS=1 developer mode, compiles typed Lisp runtime projections via node scripts/compile-v3-runtime.mjs --json before building binaries, records compiled projection schema/source/file hashes in typed_lisp_runtime, supports legacy direct-binary migration, verified linker signature acceptance before force-sign fallback, pre-switch MCP smoke, launchd runtime-root rewrite and reload, post-switch daemon IPC smoke, previous-release rollback, cleanup-only dry-run/apply, removal of incomplete release dirs, retention of active/previous/newest releases, and CARGO_INCREMENTAL=0 by default to keep self-update disk-bounded.")
)
