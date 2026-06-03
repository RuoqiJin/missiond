#!/usr/bin/env node

const args = new Set(process.argv.slice(2));
const json = args.has('--json');
const allowBusy = args.has('--allow-busy');
const baseUrl = stripTrailingSlash(
  process.env.JARVIS_BASE_URL || 'https://jarvis.xiaojinpro.top',
);
const authReadyUrl =
  process.env.AUTH_READY_URL || 'https://auth.xiaojinpro.com/health/ready';
const tunnelHealthUrl = process.env.JARVIS_TUNNEL_HEALTH_URL || '';

function stripTrailingSlash(value) {
  return String(value).replace(/\/+$/, '');
}

async function fetchText(url, options = {}) {
  const controller = new AbortController();
  const timeout = setTimeout(() => controller.abort(), Number(process.env.JARVIS_SMOKE_TIMEOUT_MS || 10000));
  try {
    const response = await fetch(url, { ...options, signal: controller.signal });
    const text = await response.text();
    return { status: response.status, ok: response.ok, text, headers: Object.fromEntries(response.headers.entries()) };
  } finally {
    clearTimeout(timeout);
  }
}

async function main() {
  const diagnostics = [];
  const auth = await fetchText(authReadyUrl).catch((error) => ({
    status: 0,
    ok: false,
    text: error.message,
    headers: {},
  }));
  if (auth.status !== 200) {
    diagnostics.push({
      code: 'AUTH_READY_FAILED',
      message: `Auth ready endpoint returned ${auth.status}`,
      url: authReadyUrl,
    });
  }

  const monitorUrl = `${baseUrl}/api/monitor/jarvis`;
  const monitorRaw = await fetchText(monitorUrl).catch((error) => ({
    status: 0,
    ok: false,
    text: error.message,
    headers: {},
  }));
  let monitor = null;
  try {
    monitor = JSON.parse(monitorRaw.text);
  } catch {
    diagnostics.push({
      code: 'JARVIS_MONITOR_NON_JSON',
      message: `Jarvis monitor returned non-JSON HTTP ${monitorRaw.status}`,
      body_sample: monitorRaw.text.slice(0, 300),
    });
  }

  if (monitor) {
    const acceptedSchemas = new Set([
      'missiond.jarvis-chain-monitor.v1',
      'missiond.jarvis-chain-monitor.v2',
    ]);
    if (!acceptedSchemas.has(monitor.schema)) {
      diagnostics.push({
        code: 'JARVIS_MONITOR_SCHEMA_DRIFT',
        message: `Unexpected monitor schema: ${monitor.schema || '<missing>'}`,
      });
    }
    if (monitor.schema === 'missiond.jarvis-chain-monitor.v2') {
      if (!monitor.route_graph || !monitor.runtime_topology) {
        diagnostics.push({
          code: 'JARVIS_MONITOR_TOPOLOGY_MISSING',
          message: 'Jarvis monitor v2 must include route_graph and runtime_topology.',
        });
      }
      if (!monitor.provider_box_slots || !Array.isArray(monitor.provider_box_slots.slots)) {
        diagnostics.push({
          code: 'JARVIS_MONITOR_PROVIDER_SLOTS_MISSING',
          message: 'Jarvis monitor v2 must include provider_box_slots.slots.',
        });
      } else {
        const phases = new Set(monitor.provider_box_slots.slots.map((slot) => slot.phase));
        for (const required of ['intent', 'grounding', 'key_judgment', 'plan', 'communicator', 'direct_answer']) {
          if (!phases.has(required)) {
            diagnostics.push({
              code: 'JARVIS_MONITOR_PROVIDER_SLOT_PHASE_MISSING',
              message: `Jarvis monitor provider_box_slots missing phase ${required}`,
            });
          }
        }
      }
    }
    const overall = String(monitor.overall || 'unknown');
    if (!['ready', 'busy'].includes(overall) || (overall === 'busy' && !allowBusy)) {
      diagnostics.push({
        code: 'JARVIS_OVERALL_NOT_READY',
        message: `Jarvis overall=${overall}`,
        hint: 'Use --allow-busy only when validating route/daemon health rather than worker availability.',
      });
    }
    const checkIds = new Set((monitor.checks || []).map((check) => check.id));
    for (const required of ['public-entry', 'default-slot-readiness']) {
      if (!checkIds.has(required)) {
        diagnostics.push({
          code: 'JARVIS_MONITOR_MISSING_CHECK',
          message: `Jarvis monitor missing check ${required}`,
        });
      }
    }
  }

  let tunnel = null;
  if (tunnelHealthUrl) {
    tunnel = await fetchText(tunnelHealthUrl).catch((error) => ({
      status: 0,
      ok: false,
      text: error.message,
      headers: {},
    }));
    if (tunnel.status >= 500 || tunnel.status === 0) {
      diagnostics.push({
        code: 'JARVIS_TUNNEL_HEALTH_FAILED',
        message: `Tunnel health returned ${tunnel.status}`,
        url: tunnelHealthUrl,
      });
    }
  }

  const result = {
    ok: diagnostics.length === 0,
    schema: 'missiond.jarvis-chain-smoke.v1',
    base_url: baseUrl,
    auth_ready: { url: authReadyUrl, status: auth.status },
    monitor: monitor
      ? {
          url: monitorUrl,
          http_status: monitorRaw.status,
          schema: monitor.schema,
          legacy_schema: monitor.legacy_schema,
          overall: monitor.overall,
          route_graph: monitor.route_graph
            ? {
                edge_node: monitor.route_graph.edge_node,
                origin_node: monitor.route_graph.origin_node,
                tunnel_client_id: monitor.route_graph.tunnel_client_id,
                route_generation: monitor.route_graph.route_generation,
              }
            : null,
          provider_box_slots: monitor.provider_box_slots
            ? {
                summary: monitor.provider_box_slots.summary,
                phases: (monitor.provider_box_slots.slots || []).map((slot) => ({
                  phase: slot.phase,
                  slot_id: slot.slot_id,
                  status: slot.status,
                  ok: slot.ok,
                  blocked_kind: slot.recognition?.blocked_kind,
                })),
              }
            : null,
          checks: (monitor.checks || []).map((check) => ({
            id: check.id,
            ok: check.ok,
            status: check.status,
          })),
        }
      : { url: monitorUrl, http_status: monitorRaw.status },
    tunnel_health: tunnelHealthUrl ? { url: tunnelHealthUrl, status: tunnel?.status } : null,
    diagnostics,
  };

  if (json) {
    console.log(JSON.stringify(result, null, 2));
  } else if (result.ok) {
    console.log(`Jarvis chain smoke OK: ${monitor?.overall || 'unknown'} at ${baseUrl}`);
  } else {
    console.error(JSON.stringify(result, null, 2));
  }
  process.exit(result.ok ? 0 : 1);
}

main().catch((error) => {
  const result = {
    ok: false,
    schema: 'missiond.jarvis-chain-smoke.v1',
    diagnostics: [{ code: 'JARVIS_CHAIN_SMOKE_EXCEPTION', message: error.message }],
  };
  console.error(json ? JSON.stringify(result, null, 2) : error.stack || error.message);
  process.exit(1);
});
