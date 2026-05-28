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
const workerChecker = read(XJPCODE_ROOT, "scripts/check-xjpcode-portable-worker-runtime.mjs");
const taskDelegate = read(MISSIOND_ROOT, "crates/missiond-daemon/src/handlers/compute/task_delegate.rs");
const liveSmoke = read(MISSIOND_ROOT, "scripts/smoke-xjpcode-worker-dispatch.mjs");

push("missiond project registry exists", Boolean(projectRegistry));
push("missiond workstation runtime exists", Boolean(workstationRuntime));
push("xjpcode blueprint exists", Boolean(blueprint));
push("xjpcode worker implementation exists", Boolean(worker));

push("project registry names xjpcode worker checker", has(projectRegistry, "check-xjpcode-portable-worker-runtime.mjs"));
push("project registry declares portable runtime role", has(projectRegistry, "portable agent runtime"));
push("portable worker registry exists", has(workstationRuntime, "portable-worker-runtime-registry"));
push("xjpcode read-only worker registered", has(workstationRuntime, "xjpcode-readonly-worker"));
push("xjpcode write worker gated", has(workstationRuntime, "xjpcode-code-worker") && has(workstationRuntime, ":status gated"));
push("registry forbids write bypass", has(workstationRuntime, "write_lease") && has(workstationRuntime, "artifact-first"));

push("blueprint portable-worker-runtime pillar", has(blueprint, "(pillar portable-worker-runtime"));
push("intent portable-worker-runtime pillar", has(intent, "(pillar portable-worker-runtime"));
push("manifest portable-worker module-map", has(manifest, "(portable-worker-runtime \"src/server/worker.rs\""));
push("manifest portable-worker checker", has(manifest, "(checker portable-worker-runtime"));
push("server exposes health route", has(serverMod, "/worker/v1/health"));
push("server exposes work-order route", has(serverMod, "/worker/v1/work-orders"));
push("server exposes event replay route", has(serverMod, "/worker/v1/work-orders/:id/events"));
push("mission_task_delegate xjpcode detector", has(taskDelegate, "engine_hint_is_xjpcode"));
push("mission_task_delegate spawns xjpcode worker", has(taskDelegate, "spawn_xjpcode_readonly_worker"));
push("mission_task_delegate xjpcode env", has(taskDelegate, "MISSIOND_XJPCODE_WORKER_URL"));
push("mission_task_delegate parses xjpcode SSE", has(taskDelegate, "parse_xjpcode_sse_frames"));
push("mission_task_delegate writes task artifact", has(taskDelegate, "task_result_put_typed"));
push("mission_task_delegate settles xjpcode worker", has(taskDelegate, "settle_worker_command") && has(taskDelegate, "WorkerSettleRequest"));
push("xjpcode dispatch smoke script exists", Boolean(liveSmoke));
push("xjpcode dispatch smoke calls mission_task_delegate", has(liveSmoke, "mission_task_delegate"));
push("xjpcode dispatch smoke polls task_result_get", has(liveSmoke, "task_result_get"));
push("xjpcode dispatch smoke is explicit live opt-in", has(liveSmoke, "--live"));

for (const token of [
  "pub struct WorkOrderRequest",
  "pub enum WorkOrderMode",
  "context_capsule_lisp",
  "read_scope",
  "write_scope",
  "accepted_shard_id",
  "TaskResultArtifact",
  "xjpcode.task-result-artifact.v1",
  "READ_SCOPE_OUTSIDE_PROJECT",
  "WRITE_MODE_NOT_ENABLED",
  "WorkerFrame::TaskResultArtifact",
]) {
  push(`worker token ${token}`, has(worker, token));
}

push("xjpcode project-local worker checker exists", Boolean(workerChecker));

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
