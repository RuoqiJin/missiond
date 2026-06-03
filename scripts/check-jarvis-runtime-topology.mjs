#!/usr/bin/env node

import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';

const json = process.argv.includes('--json');
const diagnostics = [];

const SERVICE_RUNTIME = '.missiond/v3/shards/universe/service-runtime.lisp';
const COMPILED_UNIVERSE_REL = 'compiled-project-universe.json';
const REPO_COMPILED_UNIVERSE = path.join('.missiond/v3/runtime/compiled', COMPILED_UNIVERSE_REL);

const expected = {
  service_id: 'missiond-jarvis-edge',
  schema: 'missiond.jarvis-runtime-topology.v1',
  edge_node: 'gcp-caddy-edge',
  edge_domain: 'jarvis.xiaojinpro.top',
  edge_public_ip: '34.104.147.118',
  origin_node: 'bwg-tunnel',
  origin: '104.194.81.38:9876',
  tunnel_server_url: 'ws://104.194.81.38:9876/tunnel/ws',
  tunnel_client_id: 'rickyhqmac-mini-jarvis',
  target_service: 'missiond',
  expected_deploy_agent_version: '10.7.15',
  launchd_unit: 'com.xiaojinpro.jarvis-tunnel',
  route_generation: 'jarvis-gcp-bwg-macmini-20260603',
  proxy_no_buffer: true,
  proxy_flush_interval: '-1',
  proxy_read_timeout: '75s',
  proxy_write_timeout: '75s',
  proxy_stream_timeout: '0',
};

function read(path) {
  try {
    return fs.readFileSync(path, 'utf8');
  } catch (error) {
    diagnostics.push({
      code: 'READ_FAILED',
      path,
      message: error.message,
    });
    return '';
  }
}

function compiledUniverseCandidates() {
  const candidates = [];
  const compiledRuntimeDir = String(process.env.MISSIOND_COMPILED_RUNTIME_DIR || '').trim();
  if (compiledRuntimeDir) {
    candidates.push(path.join(compiledRuntimeDir, COMPILED_UNIVERSE_REL));
  }
  const runtimeDir = String(process.env.MISSIOND_RUNTIME_DIR || '').trim();
  if (runtimeDir) {
    candidates.push(path.join(runtimeDir, 'compiled', COMPILED_UNIVERSE_REL));
  }
  candidates.push(path.join(os.homedir(), '.xjp-mission/active/compiled-runtime', COMPILED_UNIVERSE_REL));
  candidates.push(path.join(os.homedir(), '.missiond/runtime/missiond/compiled', COMPILED_UNIVERSE_REL));
  candidates.push(REPO_COMPILED_UNIVERSE);
  return [...new Set(candidates)];
}

function resolveCompiledUniversePath() {
  const candidates = compiledUniverseCandidates();
  const found = candidates.find((candidate) => fs.existsSync(candidate));
  if (found) return { path: found, candidates };
  diagnostics.push({
    code: 'COMPILED_UNIVERSE_NOT_FOUND',
    path: candidates[0] ?? REPO_COMPILED_UNIVERSE,
    candidates,
    message: 'compiled-project-universe.json was not found in MISSIOND_COMPILED_RUNTIME_DIR, MISSIOND_RUNTIME_DIR/compiled, active release, runtime cache, or repo dev fallback.',
  });
  return { path: '', candidates };
}

function requireSource(fragment, code) {
  if (!source.includes(fragment)) {
    diagnostics.push({
      code,
      path: SERVICE_RUNTIME,
      message: `Missing source fragment: ${fragment}`,
    });
  }
}

const source = read(SERVICE_RUNTIME);
requireSource('(service :id missiond-jarvis-edge', 'JARVIS_EDGE_SERVICE_MISSING');
requireSource(':tunnel-client "rickyhqmac-mini-jarvis"', 'JARVIS_TUNNEL_CLIENT_NOT_DEDICATED');
requireSource(':jarvis-runtime-topology (:schema "missiond.jarvis-runtime-topology.v1"', 'JARVIS_TOPOLOGY_FORM_MISSING');
requireSource(':tunnel-server-url "ws://104.194.81.38:9876/tunnel/ws"', 'JARVIS_TUNNEL_SERVER_URL_MISSING');
requireSource(':expected-deploy-agent-version "10.7.15"', 'JARVIS_DEPLOY_AGENT_VERSION_MISSING');
requireSource(':launchd-unit "com.xiaojinpro.jarvis-tunnel"', 'JARVIS_LAUNCHD_UNIT_MISSING');
requireSource(':route-generation "jarvis-gcp-bwg-macmini-20260603"', 'JARVIS_ROUTE_GENERATION_MISSING');
requireSource(':proxy-no-buffer true', 'JARVIS_PROXY_NO_BUFFER_MISSING');
requireSource(':proxy-flush-interval "-1"', 'JARVIS_PROXY_FLUSH_INTERVAL_MISSING');
requireSource(':proxy-read-timeout "75s"', 'JARVIS_PROXY_READ_TIMEOUT_MISSING');
requireSource(':proxy-write-timeout "75s"', 'JARVIS_PROXY_WRITE_TIMEOUT_MISSING');
requireSource(':proxy-stream-timeout "0"', 'JARVIS_PROXY_STREAM_TIMEOUT_MISSING');

const compiledResolution = resolveCompiledUniversePath();
const compiledRaw = compiledResolution.path ? read(compiledResolution.path) : '';
if (compiledRaw) {
  let compiled = null;
  try {
    compiled = JSON.parse(compiledRaw);
  } catch (error) {
      diagnostics.push({
        code: 'COMPILED_UNIVERSE_INVALID_JSON',
        path: compiledResolution.path,
        message: error.message,
      });
  }
  if (compiled) {
    const topologies = compiled?.payload?.jarvis_runtime_topologies ?? [];
    const topology = topologies.find((item) => item?.service_id === expected.service_id);
    if (!topology) {
      diagnostics.push({
        code: 'COMPILED_JARVIS_TOPOLOGY_MISSING',
        path: compiledResolution.path,
        message: 'compiled-project-universe must project missiond-jarvis-edge jarvis_runtime_topology.',
      });
    } else {
      for (const [key, value] of Object.entries(expected)) {
        if (topology[key] !== value) {
          diagnostics.push({
            code: 'COMPILED_JARVIS_TOPOLOGY_DRIFT',
            path: compiledResolution.path,
            field: key,
            expected: value,
            actual: topology[key] ?? null,
          });
        }
      }
    }
    const service = (compiled?.payload?.services ?? []).find((item) => item?.id === expected.service_id);
    if (!service?.jarvis_runtime_topology) {
      diagnostics.push({
        code: 'COMPILED_SERVICE_TOPOLOGY_MISSING',
        path: compiledResolution.path,
        message: 'missiond-jarvis-edge service must carry jarvis_runtime_topology for runtime monitor lookup.',
      });
    }
  }
}

const result = {
  ok: diagnostics.length === 0,
  schema: 'missiond.jarvis-runtime-topology-check.v1',
  checked: {
    source: SERVICE_RUNTIME,
    compiled: compiledResolution.path || null,
    compiled_candidates: compiledResolution.candidates,
  },
  diagnostics,
};

if (json) {
  console.log(JSON.stringify(result, null, 2));
} else if (result.ok) {
  console.log('Jarvis runtime topology check OK');
} else {
  console.error(JSON.stringify(result, null, 2));
}
process.exit(result.ok ? 0 : 1);
