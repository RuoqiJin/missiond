#!/usr/bin/env node
// Live smoke for MissionD -> xjpcode portable worker dispatch.
//
// Default mode is dry/no-side-effect. Pass --live to create a real read-only
// BoardTask through mission_task_delegate and wait for the canonical
// task-result-artifact.

import { spawn } from "node:child_process";
import { existsSync } from "node:fs";
import net from "node:net";
import { join } from "node:path";

const MISSIOND_ROOT = process.cwd();
const XJPCODE_ROOT =
  process.env.MISSIOND_XJPCODE_ROOT || "/Users/jinchen/Projects/xjpcode";
const args = process.argv.slice(2);
const flags = new Set(args);
const JSON_MODE = flags.has("--json");
const LIVE = flags.has("--live");
const NO_START_WORKER = flags.has("--no-start-worker");
const WAIT_SECS = numberArg("--wait-secs", Number(process.env.MISSIOND_XJPCODE_SMOKE_WAIT_SECS || 45));
const PREFLIGHT_RUNTIME = flags.has("--preflight-runtime");

let workerProcess = null;

function numberArg(name, fallback) {
  const index = args.indexOf(name);
  if (index >= 0 && args[index + 1]) {
    const parsed = Number(args[index + 1]);
    if (Number.isFinite(parsed)) return parsed;
  }
  return fallback;
}

function emit(result) {
  if (JSON_MODE) {
    console.log(JSON.stringify(result, null, 2));
  } else {
    console.log(`${result.ok ? "OK" : "FAIL"} ${result.summary}`);
    for (const step of result.steps || []) {
      console.log(`${step.ok ? "PASS" : "FAIL"} ${step.name}${step.detail ? ` — ${step.detail}` : ""}`);
    }
  }
}

function fail(summary, extra = {}) {
  const result = { ok: false, summary, ...extra };
  emit(result);
  cleanup();
  process.exit(1);
}

function cleanup() {
  if (workerProcess && !workerProcess.killed) {
    workerProcess.kill("SIGTERM");
  }
}

process.on("SIGINT", () => {
  cleanup();
  process.exit(130);
});
process.on("SIGTERM", () => {
  cleanup();
  process.exit(143);
});

async function findFreePort() {
  return await new Promise((resolve, reject) => {
    const server = net.createServer();
    server.on("error", reject);
    server.listen(0, "127.0.0.1", () => {
      const address = server.address();
      const port = typeof address === "object" && address ? address.port : null;
      server.close(() => {
        if (port) resolve(port);
        else reject(new Error("failed to allocate a local port"));
      });
    });
  });
}

async function waitForHealth(url, timeoutMs) {
  const deadline = Date.now() + timeoutMs;
  let lastError = null;
  while (Date.now() < deadline) {
    try {
      const response = await fetch(`${url.replace(/\/$/, "")}/worker/v1/health`);
      const body = await response.text();
      if (response.ok && body.includes("xjpcode-worker-runtime")) return body;
      lastError = new Error(`health HTTP ${response.status}: ${body.slice(0, 200)}`);
    } catch (error) {
      lastError = error;
    }
    await sleep(500);
  }
  throw lastError || new Error("worker health timeout");
}

async function startWorker() {
  if (!existsSync(join(XJPCODE_ROOT, "Cargo.toml"))) {
    throw new Error(`xjpcode root not found or missing Cargo.toml: ${XJPCODE_ROOT}`);
  }
  const port = Number(process.env.MISSIOND_XJPCODE_SMOKE_PORT || 0) || await findFreePort();
  const url = `http://127.0.0.1:${port}`;
  workerProcess = spawn("cargo", ["run", "--quiet", "--", "--serve", "--port", String(port)], {
    cwd: XJPCODE_ROOT,
    env: { ...process.env },
    stdio: ["ignore", "pipe", "pipe"],
  });
  let stderr = "";
  workerProcess.stderr.on("data", (chunk) => {
    stderr += chunk.toString("utf8");
  });
  workerProcess.on("exit", (code, signal) => {
    if (code !== null && code !== 0) {
      console.error(`xjpcode worker exited code=${code} signal=${signal ?? ""}`);
      if (stderr.trim()) console.error(stderr.trim().slice(-2000));
    }
  });
  await waitForHealth(url, 60000);
  return { url, spawned: true };
}

function sleep(ms) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

async function callTool(tool, payload, extraEnv = {}) {
  const raw = await new Promise((resolve, reject) => {
    const child = spawn(process.execPath, [
      "scripts/mission-mcp-call.mjs",
      tool,
      JSON.stringify(payload),
    ], {
      cwd: MISSIOND_ROOT,
      env: {
        ...process.env,
        ...extraEnv,
        MISSION_MCP_CALL_TIMEOUT_MS: process.env.MISSION_MCP_CALL_TIMEOUT_MS || "120000",
      },
      stdio: ["ignore", "pipe", "pipe"],
    });
    let stdout = "";
    let stderr = "";
    child.stdout.on("data", (chunk) => {
      stdout += chunk.toString("utf8");
    });
    child.stderr.on("data", (chunk) => {
      stderr += chunk.toString("utf8");
    });
    child.on("close", (code) => {
      if (code !== 0) {
        reject(new Error(`mission-mcp-call ${tool} failed with code ${code}: ${stderr || stdout}`));
      } else {
        resolve(stdout);
      }
    });
  });
  const response = JSON.parse(raw);
  return extractToolPayload(response);
}

function extractToolPayload(response) {
  if (response?.error) return { __mcp_error: response.error };
  const content = response?.result?.content;
  if (!Array.isArray(content)) return response;
  const texts = content
    .map((item) => item?.text)
    .filter((text) => typeof text === "string" && text.trim());
  for (const text of texts) {
    try {
      return JSON.parse(text);
    } catch {
      // Keep scanning. Some tools may emit non-JSON text before JSON.
    }
  }
  return { text: texts.join("\n") };
}

async function pollTaskResult(taskId, timeoutMs, extraEnv) {
  const deadline = Date.now() + timeoutMs;
  let lastPayload = null;
  while (Date.now() < deadline) {
    lastPayload = await callTool(
      "mission_shared_memory",
      { action: "task_result_get", task_id: taskId, limit: 1 },
      extraEnv,
    );
    const results = Array.isArray(lastPayload?.results) ? lastPayload.results : [];
    if (results.length > 0) return { payload: lastPayload, artifact: results[0] };
    await sleep(1000);
  }
  return { payload: lastPayload, artifact: null };
}

async function main() {
  const steps = [];
  const plannedArgs = {
    objective:
      "Read-only smoke: use xjpcode portable worker to inspect MissionD AGENTS.md and return a concise task-result artifact. Do not modify files.",
    intent: "research",
    cwd: MISSIOND_ROOT,
    engine_hint: "xjpcode",
    xjpcode_worker_url: null,
    task_class: "review",
    read_scope: [join(MISSIOND_ROOT, "AGENTS.md")],
    write_scope: [],
    must_not_touch: [MISSIOND_ROOT],
    acceptance: ["canonical task-result-artifact exists for this BoardTask"],
    timeout_secs: 120,
  };

  if (!LIVE) {
    emit({
      ok: true,
      summary: "dry run only; pass --live to create a real read-only xjpcode BoardTask",
      live: false,
      planned_tool: "mission_task_delegate",
      planned_args: plannedArgs,
      steps: [
        { name: "xjpcode root present", ok: existsSync(join(XJPCODE_ROOT, "Cargo.toml")), detail: XJPCODE_ROOT },
        { name: "mission-mcp-call present", ok: existsSync(join(MISSIOND_ROOT, "scripts/mission-mcp-call.mjs")) },
      ],
    });
    return;
  }

  if (PREFLIGHT_RUNTIME) {
    const masterStatus = await callTool("mission_master_status", {});
    const compiledRuntime = masterStatus?.service?.compiledRuntime;
    const runtimeOk = compiledRuntime?.runtimeConfig?.ok === true
      && Array.isArray(compiledRuntime?.runtimeDomains)
      && compiledRuntime.runtimeDomains.every((domain) => domain?.ok === true);
    if (!runtimeOk) {
      fail("MissionD compiled runtime is not ready; refusing to start xjpcode worker", {
        code: "MISSIOND_COMPILED_RUNTIME_NOT_READY",
        runtime_config: compiledRuntime?.runtimeConfig ?? null,
        runtime_domain_failures: (compiledRuntime?.runtimeDomains ?? [])
          .filter((domain) => domain?.ok !== true)
          .map((domain) => ({
            domain: domain.domain,
            diagnostics: domain.diagnostics,
            requiredAction: domain.requiredAction,
          })),
        hint: "run node scripts/project-v3-contracts.mjs --write, node scripts/compile-v3-runtime.mjs --json, then restart/deploy the MissionD daemon so its generated contract and compiled runtime agree",
      });
    }
  }

  let workerUrl = process.env.MISSIOND_XJPCODE_WORKER_URL || "";
  let workerSpawned = false;
  if (!workerUrl && !NO_START_WORKER) {
    const worker = await startWorker();
    workerUrl = worker.url;
    workerSpawned = worker.spawned;
  }
  if (!workerUrl) {
    fail("MISSIOND_XJPCODE_WORKER_URL is required when --no-start-worker is used", {
      code: "XJPCODE_WORKER_URL_REQUIRED",
    });
  }
  steps.push({ name: "xjpcode worker health", ok: true, detail: workerUrl });
  plannedArgs.xjpcode_worker_url = workerUrl;

  const extraEnv = { MISSIOND_XJPCODE_WORKER_URL: workerUrl };
  const delegate = await callTool("mission_task_delegate", plannedArgs, extraEnv);
  if (delegate?.error || delegate?.__mcp_error || delegate?.code || delegate?.error_code) {
    const code = delegate?.error?.code || delegate?.__mcp_error?.code || delegate?.code || delegate?.error_code;
    fail("mission_task_delegate refused xjpcode dispatch", {
      code,
      delegate,
      worker_url: workerUrl,
      worker_spawned: workerSpawned,
      hint: code === "XJPCODE_WORKER_NOT_CONFIGURED"
        ? "The running MissionD daemon did not inherit MISSIOND_XJPCODE_WORKER_URL; restart/deploy daemon with that env or use the production worker URL."
        : undefined,
      steps,
    });
  }
  const taskId = delegate.task_id;
  if (!taskId) {
    fail("mission_task_delegate returned no task_id", { delegate, steps });
  }
  steps.push({ name: "mission_task_delegate accepted", ok: true, detail: taskId });

  const { payload, artifact } = await pollTaskResult(taskId, WAIT_SECS * 1000, extraEnv);
  if (!artifact) {
    fail("xjpcode delegated BoardTask did not produce task-result-artifact before timeout", {
      code: "XJPCODE_ARTIFACT_TIMEOUT",
      task_id: taskId,
      delegate,
      last_task_result_payload: payload,
      steps,
    });
  }
  steps.push({ name: "canonical task-result-artifact exists", ok: true, detail: artifact.artifactHash || artifact.artifact_hash || "" });

  emit({
    ok: true,
    summary: "MissionD delegated a read-only BoardTask to xjpcode and recorded canonical task-result-artifact",
    live: true,
    worker_url: workerUrl,
    worker_spawned: workerSpawned,
    task_id: taskId,
    delegate,
    artifact,
    steps,
  });
  cleanup();
}

main().catch((error) => {
  fail(error.message, {
    code: "XJPCODE_DISPATCH_SMOKE_ERROR",
    stack: process.env.MISSIOND_DEBUG_SMOKE ? error.stack : undefined,
  });
});
