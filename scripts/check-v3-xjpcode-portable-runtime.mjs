#!/usr/bin/env node
// MissionD V3 — xjpcode portable worker runtime registry checker.
//
// This is intentionally a cross-repo structural gate. MissionD owns the
// provider/worker registry; xjpcode owns the worker API implementation.

import { existsSync, readFileSync } from "node:fs";
import { join } from "node:path";

const MISSIOND_ROOT = process.cwd();
const XJPCODE_ROOT =
  process.env.MISSIOND_XJPCODE_ROOT || "/Users/jinchen/Projects/xjpcode";
const JSON_MODE = process.argv.includes("--json");

const checks = [];

function read(root, rel) {
  const path = join(root, rel);
  if (!existsSync(path)) return null;
  return readFileSync(path, "utf8");
}

function push(name, ok, detail = "") {
  checks.push({ name, ok, detail });
}

function has(body, needle) {
  return Boolean(body && body.includes(needle));
}

const projectRegistry = read(MISSIOND_ROOT, ".missiond/v3/shards/universe/project-registry.lisp");
const workstationRuntime = read(MISSIOND_ROOT, ".missiond/v3/shards/workstation-runtime.lisp");
const blueprint = read(XJPCODE_ROOT, ".missiond/xjpcode-app-blueprint.lisp");
const intent = read(XJPCODE_ROOT, ".missiond/intent.lisp");
const manifest = read(XJPCODE_ROOT, ".missiond/intent-manifest.lisp");
const serverMod = read(XJPCODE_ROOT, "src/server/mod.rs");
const worker = read(XJPCODE_ROOT, "src/server/worker.rs");
const textProvider = read(XJPCODE_ROOT, "src/server/text_provider.rs");
const workerChecker = read(XJPCODE_ROOT, "scripts/check-xjpcode-portable-worker-runtime.mjs");
const taskDelegate = read(MISSIOND_ROOT, "crates/missiond-daemon/src/handlers/compute/task_delegate.rs");
const liveSmoke = read(MISSIOND_ROOT, "scripts/smoke-xjpcode-worker-dispatch.mjs");
const deployDaemon = read(MISSIOND_ROOT, "scripts/deploy-daemon.sh");

push("missiond project registry exists", Boolean(projectRegistry));
push("missiond workstation runtime exists", Boolean(workstationRuntime));
push("xjpcode blueprint exists", Boolean(blueprint));
push("xjpcode worker implementation exists", Boolean(worker));
push("xjpcode text-only provider implementation exists", Boolean(textProvider));

push("project registry names xjpcode worker checker", has(projectRegistry, "check-xjpcode-portable-worker-runtime.mjs"));
push("project registry declares portable runtime role", has(projectRegistry, "portable agent runtime"));
push("portable worker registry exists", has(workstationRuntime, "portable-worker-runtime-registry"));
push("xjpcode read-only worker registered", has(workstationRuntime, "xjpcode-readonly-worker"));
push("xjpcode write worker active-gated", has(workstationRuntime, "xjpcode-code-worker") && has(workstationRuntime, ":status active-gated"));
push("registry forbids write bypass", has(workstationRuntime, "write_lease_id") && has(workstationRuntime, "artifact-first") && has(workstationRuntime, "git apply --check"));
push("text-only paid CLI registry exists", has(workstationRuntime, "text-only-paid-cli-provider-registry"));
push("text-only registry disabled by default", has(workstationRuntime, ":default-enabled false") && has(workstationRuntime, "XJPCODE_TEXT_ONLY_CLI_ENABLED"));
push("text-only registry forbids tools", has(workstationRuntime, "tool_use") && has(workstationRuntime, "apply_patch") && has(workstationRuntime, "approval_prompt"));
push("text-only registry pins xjpcode endpoint", has(workstationRuntime, "/provider/v1/text-only/completions") && has(workstationRuntime, "xjpcode.text-only-provider.v1"));

push("blueprint portable-worker-runtime pillar", has(blueprint, "(pillar portable-worker-runtime"));
push("blueprint text-only-provider-runtime pillar", has(blueprint, "(pillar text-only-provider-runtime"));
push("intent portable-worker-runtime pillar", has(intent, "(pillar portable-worker-runtime"));
push("intent text-only-provider-runtime pillar", has(intent, "(pillar text-only-provider-runtime"));
push("manifest portable-worker module-map", has(manifest, "(portable-worker-runtime \"src/server/worker.rs\""));
push("manifest text-only-provider module-map", has(manifest, "(text-only-provider-runtime \"src/server/text_provider.rs\""));
push("manifest portable-worker checker", has(manifest, "(checker portable-worker-runtime"));
push("server exposes health route", has(serverMod, "/worker/v1/health"));
push("server exposes work-order route", has(serverMod, "/worker/v1/work-orders"));
push("server exposes event replay route", has(serverMod, "/worker/v1/work-orders/:id/events"));
push("server exposes text-only provider list route", has(serverMod, "/provider/v1/text-only/providers"));
push("server exposes text-only completion route", has(serverMod, "/provider/v1/text-only/completions"));
push("mission_task_delegate xjpcode detector", has(taskDelegate, "engine_hint_is_xjpcode"));
push("mission_task_delegate spawns xjpcode worker", has(taskDelegate, "spawn_xjpcode_readonly_worker"));
push("mission_task_delegate xjpcode env", has(taskDelegate, "MISSIOND_XJPCODE_WORKER_URL"));
push("mission_task_delegate accepts xjpcode code worker", has(taskDelegate, "xjpcode-code-worker") && !has(taskDelegate, "XJPCODE_WRITE_MODE_NOT_ENABLED"));
push("mission_task_delegate passes xjpcode write lease", has(taskDelegate, "write_lease_id") && has(taskDelegate, "apply_patch") && has(taskDelegate, "claim_task_grant_id"));
push("mission_task_delegate loopback smoke override", has(taskDelegate, "xjpcode_worker_url") && has(taskDelegate, "XJPCODE_WORKER_URL_UNTRUSTED"));
push("mission_task_delegate parses xjpcode SSE", has(taskDelegate, "parse_xjpcode_sse_frames"));
push("mission_task_delegate stops xjpcode SSE on final frame", has(taskDelegate, "read_xjpcode_sse_until_final") && has(taskDelegate, "xjpcode_final_status_from_frames(&frames).is_some()"));
push("mission_task_delegate persists xjpcode worker events", has(taskDelegate, "record_xjpcode_worker_events") && has(taskDelegate, "worker-runtime:xjpcode"));
push("mission_task_delegate persists xjpcode worker telemetry", has(taskDelegate, "record_xjpcode_worker_telemetry") && has(taskDelegate, "xjpcode_worker.telemetry"));
push("registry pins durable xjpcode worker event stream", has(workstationRuntime, "shared_events stream_id=worker-runtime:xjpcode") && has(workstationRuntime, "xjpcode_worker.telemetry"));
push("mission_task_delegate writes task artifact", has(taskDelegate, "write_completion_artifact") && has(taskDelegate, "TaskCompletionEvidenceInput"));
push("mission_task_delegate promotes xjpcode changed-path evidence", has(taskDelegate, "\"changed_paths\": files_changed.clone()") && has(taskDelegate, "\"files_changed\": files_changed") && has(taskDelegate, "\"verification\": verification"));
push("mission_task_delegate binds xjpcode artifacts to attempts", has(taskDelegate, "accepted_shard_id: run.metadata.accepted_shard_id.clone()") && has(taskDelegate, "attempt_id: Some(run.attempt_id.clone())"));
push("mission_task_delegate reports rejected xjpcode artifacts", has(taskDelegate, "xjpcode_worker.artifact_rejected") && has(workstationRuntime, "xjpcode_worker.artifact_rejected"));
push("mission_task_delegate releases xjpcode claims on terminal outcomes", has(taskDelegate, "release_xjpcode_worker_claims") && has(taskDelegate, "artifact_rejected") && has(taskDelegate, "artifact_settled"));
push("mission_task_delegate settles xjpcode worker", has(taskDelegate, "settle_task_command") && has(taskDelegate, "SettleTaskCommand"));
push("xjpcode dispatch smoke script exists", Boolean(liveSmoke));
push("xjpcode dispatch smoke calls mission_task_delegate", has(liveSmoke, "mission_task_delegate"));
push("xjpcode dispatch smoke passes loopback worker url", has(liveSmoke, "xjpcode_worker_url"));
push("xjpcode dispatch smoke polls task_result_get", has(liveSmoke, "task_result_get"));
push("xjpcode dispatch smoke is explicit live opt-in", has(liveSmoke, "--live"));
push("xjpcode dispatch smoke rejects non-success artifacts by default", has(liveSmoke, "XJPCODE_ARTIFACT_NOT_SUCCESS") && has(liveSmoke, "artifactStatusIsTerminalSuccess"));
push("xjpcode dispatch smoke keeps diagnostic escape hatch explicit", has(liveSmoke, "--allow-blocked-artifact") && has(workstationRuntime, "--allow-blocked-artifact"));
push("deploy-daemon propagates xjpcode worker env", has(deployDaemon, "MISSIOND_XJPCODE_WORKER_URL"));

for (const token of [
  "pub struct WorkOrderRequest",
  "pub enum WorkOrderMode",
  "context_capsule_lisp",
  "read_scope",
  "write_scope",
  "accepted_shard_id",
  "write_lease_id",
  "worktree_id",
  "validate_write_contract",
  "PATCH_PATH_OUTSIDE_WRITE_SCOPE",
  "git apply",
  "TaskResultArtifact",
  "xjpcode.task-result-artifact.v1",
  "READ_SCOPE_OUTSIDE_PROJECT",
  "WorkerFrame::TaskResultArtifact",
]) {
  push(`worker token ${token}`, has(worker, token));
}

push("xjpcode project-local worker checker exists", Boolean(workerChecker));

for (const token of [
  "TextOnlyProviderId",
  "ClaudeCode",
  "CodexCli",
  "AgyCli",
  "TEXT_ONLY_CONTRACT",
  "xjpcode.text-only-provider.v1",
  "TextOnlyFrame::TextDelta",
  "TEXT_ONLY_PAID_CLI_DISABLED",
  "XJPCODE_TEXT_ONLY_CLI_ENABLED",
  "build_text_only_cli_invocation",
  "detect_text_only_violation",
  "TEXT_ONLY_PROVIDER_VIOLATION",
  "--disable-slash-commands",
  "--strict-mcp-config",
  "--sandbox",
  "read-only",
]) {
  push(`text-only provider token ${token}`, has(textProvider, token));
}

const failures = checks.filter((c) => !c.ok);
const result = {
  ok: failures.length === 0,
  missiond_root: MISSIOND_ROOT,
  xjpcode_root: XJPCODE_ROOT,
  failures: failures.length,
  checks,
};

if (JSON_MODE) {
  console.log(JSON.stringify(result, null, 2));
} else {
  for (const check of checks) {
    console.log(`${check.ok ? "PASS" : "FAIL"} ${check.name}${check.detail ? ` — ${check.detail}` : ""}`);
  }
  console.log(`Result: ${result.ok ? "PASS" : "FAIL"}`);
}

process.exit(result.ok ? 0 : 1);
