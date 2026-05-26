#!/usr/bin/env node
import { spawnSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import crypto from 'node:crypto';

const DEFAULT_DB = process.env.MISSION_PG_DATABASE || 'missiond';
const DEFAULT_BATCH = `kb-triage-${new Date().toISOString().slice(0, 10).replaceAll('-', '')}-full-v1`;
const HIDDEN_STATES = new Set([
  'superseded-by-lisp',
  'superseded-by-code',
  'historical-evidence',
  'duplicate',
  'wrong-or-stale',
  'delete-candidate',
  'needs-human',
]);

const MANUAL_OVERRIDES = new Map(Object.entries({
  'strategic-state': ['needs-human', 'Manual pilot: broad mixed strategic snapshot; split into project constants/user preferences before active use.'],
  'missiond-user-voice-extraction-pipeline': ['superseded-by-lisp', 'Manual pilot: current memory/conversation governance lives in MissionD V3/workflows.'],
  'kb-cli-wrapper-for-non-mcp-ai': ['needs-human', 'Manual pilot: external access pattern may matter, but current MCP/runtime status must be verified before active use.'],
  'assistant-service-progressive-disclosure': ['historical-evidence', 'Manual pilot: service-specific design should live in XJP service SSOT, not global active memory.'],
  'slash-clear-is-valid-claude-code-command': ['active', 'Manual pilot: current provider behavior correction prevents recurring false diagnosis.'],
  'kb-composite-category-design': ['superseded-by-code', 'Manual pilot: composite category behavior is now schema/query implementation, not active memory.'],
  'kb-autonomous-consolidation-architecture': ['superseded-by-lisp', 'Manual pilot: entry explicitly declares V3 memory workflow as canonical.'],
  'memory-extraction-meta-circulation-resolved': ['superseded-by-lisp', 'Manual pilot: entry explicitly declares V3 conversation-memory workflow as canonical.'],
  'missiond-gemini-call-sites-architecture': ['superseded-by-lisp', 'Manual pilot: V3 workstation-pool/router-policy owns Gemini role and dispatch.'],
  'frontend-chat-dual-session': ['needs-human', 'Manual pilot: Jarvis-specific runtime fact should be verified/promoted to Jarvis SSOT.'],
  'missiond-briefing-worker-minimax-architecture': ['superseded-by-lisp', 'Manual pilot: entry explicitly declares workstation/model routing as canonical.'],
  'router-gemini-google-search-grounding': ['needs-human', 'Manual pilot: provider quirk may remain useful but should be owned by router SSOT if current.'],
  'quark-api-technical-details': ['needs-human', 'Manual pilot: unregistered/external object-store fact needs project owner before active use.'],
  'network-topology-overview': ['needs-human', 'Manual pilot: operational topology is volatile and belongs in Universe/deploy-center after verification.'],
  'memory-slot-stuck-detection-evolution': ['historical-evidence', 'Manual pilot: debug evolution; current supervision must be code/workflow-owned.'],
  'baidu-netdisk-integration-architecture': ['needs-human', 'Manual pilot: feature/project ownership unclear.'],
  'missiond-runs-on-local-mac-not-privatecloud': ['superseded-by-lisp', 'Manual pilot: runtime location belongs to MissionD Universe/project registry.'],
  'router-billing-three-layer-system': ['needs-human', 'Manual pilot: important router/payment fact, but should be verified against router/payment SSOT.'],
  'missiond-ops-diagnostic-tools-implemented': ['superseded-by-lisp', 'Manual pilot: tool surfaces and registry own this.'],
  'router-four-connectors-architecture': ['needs-human', 'Manual pilot: router model/provider topology should be router SSOT.'],
  'board-ui-and-task-management-features': ['superseded-by-lisp', 'Manual pilot: entry explicitly declares Board frontend SSOT as canonical.'],
  'gemini-vertex-json-schema-quirks': ['active', 'Manual pilot: current external provider quirk useful until provider/router SSOT fully owns tested behavior.'],
  'private-cloud-dns-split-and-minimax-integration': ['historical-evidence', 'Manual pilot: old private-cloud/MiniMax ops history, likely not active in current worker pool.'],
  'deploy-agent-autoupdate-lifecycle': ['needs-human', 'Manual pilot: important deploy-agent fact; verify against deploy-agent/deploy-center SSOT.'],
  'missiond-context-budget-manager-architecture': ['superseded-by-lisp', 'Manual pilot: current context-budget/transport policy belongs to V3 conversation/workflow/runtime.'],
  'ios-jarvis-integration-architecture': ['needs-human', 'Manual pilot: Jarvis/iOS fact should be verified against Jarvis SSOT.'],
  'missiond-maxsim-multi-topic-search-architecture': ['superseded-by-code', 'Manual pilot: search implementation owns this behavior.'],
  'jarvis-trace-store-ring-buffer': ['superseded-by-code', 'Manual pilot: code/tool registry owns Jarvis trace surfaces.'],
  'missiond-subagent-parent-session-architecture': ['superseded-by-lisp', 'Manual pilot: entry explicitly declares resident master/workflows canonical.'],
  'jsonl-full-capture-dual-table-design': ['superseded-by-lisp', 'Manual pilot: entry explicitly declares conversation ingestion canonical.'],
  'mcp-frontend-camelcase-contract-risk': ['active', 'Manual pilot: current API casing risk prevents frontend regressions.'],
  'verify-subagent-code-analysis-manually': ['active', 'Manual pilot: current user preference / operating rule.'],
  'missiond-embedding-provider-architecture': ['needs-human', 'Manual pilot: embedding plan changed several times; verify against current V3/runtime.'],
  'missiond-shared-http-client-for-router': ['superseded-by-code', 'Manual pilot: historical bugfix owned by implementation.'],
  'auth-deploy-center-subdomain-allocation': ['superseded-by-lisp', 'Manual pilot: Auth/deploy-center Universe owns domain facts.'],
  'pty-state-detection-v2-architecture': ['superseded-by-lisp', 'Manual pilot: V3 PTY recognition/upstream signatures own current behavior.'],
  'mcp-only-deploy-docker-skip-normal': ['needs-human', 'Manual pilot: deploy behavior should live in deploy-center SSOT if current.'],
  'claude-code-jsonl-role-mapping-quirks': ['active', 'Manual pilot: current provider log parsing quirk relevant to role/turn audits.'],
  'agent-update-source-priority-2026-02-21': ['needs-human', 'Manual pilot: deploy-agent source priority should be verified in deploy-agent SSOT.'],
  'missiond-token-usage-ledger-architecture': ['historical-evidence', 'Manual pilot: legacy local-store rationale; current Postgres/event ledger should be code/SSOT-owned.'],
  'missiond-claude-md-auto-sync': ['wrong-or-stale', 'Manual pilot: context preloading/autosync was intentionally disabled/reduced due KB noise.'],
  'jarvis-phase2-e2e-completed': ['historical-evidence', 'Manual pilot: completed milestone summary, not active guidance.'],
  'pty-screenshot-frontend-xterm-canvas': ['superseded-by-code', 'Manual pilot: current feature owned by code/frontend SSOT.'],
  'missiond-deep-analysis-trigger-architecture': ['superseded-by-lisp', 'Manual pilot: entry explicitly declares nightly/memory workflows canonical.'],
  'missiond-task-ack-mechanism': ['superseded-by-code', 'Manual pilot: MCP tool/implementation owns this.'],
  'missiond-slots-yaml-hot-reload-implemented': ['superseded-by-code', 'Manual pilot: runtime implementation owns this.'],
  'pty-logs-vs-direct-conversation-analysis-responsibility': ['active', 'Manual pilot: current user preference about PTY logs vs direct conversation analysis.'],
  'timeline-ai-step-explanation-gpt-5.4': ['needs-human', 'Manual pilot: product/design choice should be revalidated in timeline SSOT.'],
  'kb-write-summary-length-enforcement': ['active', 'Manual pilot: current KB quality rule directly governs future memory writes.'],
  'missiond-confirm-screen-builtin-tool-parsing': ['superseded-by-code', 'Manual pilot: historical bugfix owned by code/tests.'],
}));

function usage() {
  console.log(`Usage: node scripts/kb-memory-triage.mjs [--apply] [--all-knowledge] [--db missiond] [--batch id] [--out-dir path]

Non-destructively reviews KB rows into knowledge_review_state.
Default scope is all knowledge rows because policy/preference entries are also memory-chain inputs.
No knowledge row is deleted or mutated.`);
}

function parseArgs(argv) {
  const opts = {
    apply: false,
    db: DEFAULT_DB,
    batchId: DEFAULT_BATCH,
    outDir: '.missiond/research/kb-memory-triage-20260509',
    allKnowledge: true,
  };
  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    if (arg === '--help' || arg === '-h') {
      usage();
      process.exit(0);
    }
    if (arg === '--apply') opts.apply = true;
    else if (arg === '--all-knowledge') opts.allKnowledge = true;
    else if (arg === '--memory-only') opts.allKnowledge = false;
    else if (arg === '--db') opts.db = argv[++i];
    else if (arg === '--batch') opts.batchId = argv[++i];
    else if (arg === '--out-dir') opts.outDir = argv[++i];
    else throw new Error(`unknown argument: ${arg}`);
  }
  return opts;
}

function psql(db, sql, { input } = {}) {
  const args = ['-d', db, '-v', 'ON_ERROR_STOP=1', '-At'];
  if (sql) args.push('-c', sql);
  const result = spawnSync('psql', args, {
    input,
    encoding: 'utf8',
    maxBuffer: 1024 * 1024 * 100,
  });
  if (result.status !== 0) {
    throw new Error(`psql failed (${result.status})\nSTDOUT:\n${result.stdout}\nSTDERR:\n${result.stderr}`);
  }
  return result.stdout;
}

function loadRows(opts) {
  const where = opts.allKnowledge
    ? 'TRUE'
    : "(category = 'memory' OR category LIKE 'memory:%')";
  const sql = `SELECT COALESCE(json_agg(t), '[]'::json)
    FROM (
      SELECT id, category, key, summary, detail, source, confidence, access_count,
             created_at, updated_at, last_accessed_at, linked_task_id, kb_type,
             scope_task_id, utility_score, project_id
      FROM knowledge
      WHERE ${where}
      ORDER BY
        CASE WHEN category = 'preference' OR category LIKE 'preference:%' THEN 0
             WHEN category = 'system_rule' OR category = 'memory:rule' THEN 1
             WHEN category = 'policy:decision' OR category = 'memory:decision' OR category = 'decision' THEN 2
             ELSE 3 END,
        access_count DESC NULLS LAST,
        updated_at DESC
    ) t`;
  return JSON.parse(psql(opts.db, sql).trim() || '[]');
}

function lc(row) {
  return `${row.category}\n${row.key}\n${row.summary}\n${row.detail ?? ''}`.toLowerCase();
}

function hasAny(text, needles) {
  return needles.some((needle) => text.includes(needle.toLowerCase()));
}

function classify(row) {
  const manual = MANUAL_OVERRIDES.get(row.key);
  if (manual) {
    return {
      state: manual[0],
      confidence: manual[0] === 'active' ? 0.92 : 0.88,
      rationale: manual[1],
      evidence_refs: ['manual-pilot-batches.md'],
    };
  }

  const text = lc(row);
  const category = row.category ?? '';
  const access = Number(row.access_count ?? 0);
  const project = row.project_id ?? '';

  if (hasAny(text, ['[superseded-by-lisp]', '"status":"superseded-by-lisp"', '"status": "superseded-by-lisp"', 'v3 blueprint', 'current v3', 'current missiond v3'])) {
    return {
      state: 'superseded-by-lisp',
      confidence: 0.9,
      rationale: 'Entry or content points to current Lisp/Workflow/V3 SSOT as canonical; keep as historical evidence only.',
      evidence_refs: ['.missiond/v3/missiond-blueprint.lisp', '.missiond/workflows'],
    };
  }

  if (hasAny(text, ['[superseded-by-code]', '"status":"superseded-by-code"', '"status": "superseded-by-code"'])) {
    return {
      state: 'superseded-by-code',
      confidence: 0.9,
      rationale: 'Entry explicitly declares implementation as canonical.',
      evidence_refs: ['code-anchor'],
    };
  }

  if (hasAny(text, ['m10', 'h5', 'domain-hardening']) && hasAny(text, ['maturity', '成熟度'])) {
    return {
      state: 'wrong-or-stale',
      confidence: 0.78,
      rationale: 'Uses retired maturity vocabulary; current public maturity model is M0..M6.',
      evidence_refs: ['.missiond/v3/missiond-blueprint.lisp::project-maturity-model'],
    };
  }

  if (hasAny(text, ['legacy local store']) && hasAny(text, ['missiond', 'missiondb', '锁', 'busy', 'wal'])) {
    return {
      state: 'historical-evidence',
      confidence: 0.82,
      rationale: 'Legacy MissionD implementation history; current backend is Postgres/event-bus governed.',
      evidence_refs: ['crates/missiond-daemon/src/main.rs', 'crates/missiond-core/src/db/pg'],
    };
  }

  if (hasAny(text, ['已废弃', 'deprecated', '弃用', '不再', '旧 ', '旧的', 'legacy']) && !hasAny(text, ['必须', '禁止', 'principle'])) {
    return {
      state: 'historical-evidence',
      confidence: 0.78,
      rationale: 'Historical/deprecated wording indicates this should not guide current behavior by default.',
      evidence_refs: [],
    };
  }

  if (category === 'preference' || category.startsWith('preference:') || category === 'system_rule') {
    return {
      state: 'active',
      confidence: 0.88,
      rationale: 'User preference/system rule remains active unless explicitly superseded.',
      evidence_refs: ['user-preference'],
    };
  }

  if (category === 'memory:rule') {
    return {
      state: 'active',
      confidence: 0.86,
      rationale: 'Memory rule remains active unless explicitly superseded.',
      evidence_refs: ['memory-rule'],
    };
  }

  if (category === 'policy:decision' || category === 'memory:decision' || category === 'decision') {
    if (access >= 50) {
      return {
        state: 'active',
        confidence: 0.82,
        rationale: 'High-use policy/decision remains in active memory until a narrower SSOT constant replaces it.',
        evidence_refs: ['policy-decision'],
      };
    }
    return {
      state: 'needs-human',
      confidence: 0.72,
      rationale: 'Low-use policy/decision may be stale or too narrow; remove from default context pending review.',
      evidence_refs: ['policy-review-needed'],
    };
  }

  if (category === 'memory:bugfix' || category === 'memory:debug') {
    if (hasAny(text, ['provider', 'jsonl', 'claude', 'gemini', 'codex', 'mcp', 'pty']) && access >= 100) {
      return {
        state: 'active',
        confidence: 0.78,
        rationale: 'High-use provider/tooling quirk can still prevent recurring diagnosis errors.',
        evidence_refs: ['provider-quirk'],
      };
    }
    return {
      state: 'superseded-by-code',
      confidence: 0.82,
      rationale: 'Bugfix/debug entry should be owned by implementation/tests, not default memory.',
      evidence_refs: ['code-tests'],
    };
  }

  if (category === 'memory:feature' || category === 'feature' || category === 'feature_request') {
    if (hasAny(text, ['已实现', 'implemented', '新增', '完成', 'completed', 'done', 'commit'])) {
      return {
        state: 'superseded-by-code',
        confidence: 0.78,
        rationale: 'Implemented feature fact should be code/SSOT-owned; KB entry is historical trace.',
        evidence_refs: ['code-or-project-ssot'],
      };
    }
    return {
      state: 'needs-human',
      confidence: 0.72,
      rationale: 'Feature memory requires project ownership review before active use.',
      evidence_refs: ['feature-review-needed'],
    };
  }

  if (category.startsWith('architecture') || category === 'memory:architecture' || category === 'memory' || category === 'memory:ops') {
    if (hasAny(text, ['missiond']) && hasAny(text, ['v3', 'ssot', 'workflow', 'eventbus', 'event bus', 'workstation', 'resident master'])) {
      return {
        state: 'superseded-by-lisp',
        confidence: 0.82,
        rationale: 'MissionD architecture fact is now governed by V3 SSOT/workflows.',
        evidence_refs: ['.missiond/v3/missiond-blueprint.lisp'],
      };
    }
    if (hasAny(text, ['route', 'api', 'endpoint', 'service', 'domain', 'server', '部署', 'cloudflare', 'dns', '公网', 'tailscale', 'gcp', 'ecs', 'auth', 'router', 'deploy'])) {
      return {
        state: 'needs-human',
        confidence: 0.74,
        rationale: 'Operational/service architecture fact is volatile and should be verified against project SSOT/deploy-center/Universe before active use.',
        evidence_refs: ['project-ssot-or-universe-needed'],
      };
    }
    if (hasAny(text, ['已完成', 'completed', '最终结论', 'phase', 'e2e', 'smoke', 'commit'])) {
      return {
        state: 'historical-evidence',
        confidence: 0.76,
        rationale: 'Milestone or implementation history; useful for archaeology but not default guidance.',
        evidence_refs: ['historical-milestone'],
      };
    }
    if (project) {
      return {
        state: 'needs-human',
        confidence: 0.7,
        rationale: 'Project-specific architecture fact should be checked against that project SSOT before active use.',
        evidence_refs: ['project-ssot-needed'],
      };
    }
  }

  if (access >= 500 && hasAny(text, ['quirk', 'risk', '风险', '禁止', '必须', 'valid', '有效'])) {
    return {
      state: 'active',
      confidence: 0.76,
      rationale: 'High-use operational quirk/rule remains useful pending explicit SSOT promotion.',
      evidence_refs: ['high-use-quirk'],
    };
  }

  return {
    state: 'historical-evidence',
    confidence: 0.68,
    rationale: 'Default conservative demotion: not enough evidence to keep in active memory, but preserved as non-destructive historical evidence.',
    evidence_refs: [],
  };
}

function sqlString(s) {
  if (s == null) return 'NULL';
  return `'${String(s).replaceAll("'", "''")}'`;
}

function sqlJson(value) {
  return `${sqlString(JSON.stringify(value ?? []))}::jsonb`;
}

function writeReports(rows, decisions, opts) {
  fs.mkdirSync(opts.outDir, { recursive: true });
  const states = {};
  const categories = {};
  for (const d of decisions) {
    states[d.state] = (states[d.state] ?? 0) + 1;
    categories[d.category] ??= {};
    categories[d.category][d.state] = (categories[d.category][d.state] ?? 0) + 1;
  }
  const active = states.active ?? 0;
  const hidden = decisions.length - active;
  const report = {
    schema: 'missiond.kb-memory-triage-report.v1',
    generated_at: new Date().toISOString(),
    batch_id: opts.batchId,
    applied: opts.apply,
    rows_reviewed: decisions.length,
    active_count: active,
    hidden_count: hidden,
    active_ratio: Number((active / Math.max(decisions.length, 1)).toFixed(4)),
    target_active_ratio: 0.10,
    state_counts: states,
    category_state_counts: categories,
  };
  fs.writeFileSync(path.join(opts.outDir, 'triage-report.json'), `${JSON.stringify(report, null, 2)}\n`);
  fs.writeFileSync(
    path.join(opts.outDir, 'triage-decisions.jsonl'),
    decisions.map((d) => JSON.stringify(d)).join('\n') + '\n',
  );
  const needsHuman = decisions.filter((d) => d.state === 'needs-human');
  fs.writeFileSync(
    path.join(opts.outDir, 'needs-human.md'),
    [
      '# KB Memory Triage Needs Human',
      '',
      `Batch: \`${opts.batchId}\``,
      `Count: ${needsHuman.length}`,
      '',
      '| category | key | project | rationale |',
      '|---|---|---|---|',
      ...needsHuman.map((d) => `| ${d.category} | ${d.key} | ${d.project_id ?? ''} | ${d.rationale.replaceAll('|', '\\|')} |`),
      '',
    ].join('\n'),
  );
  return report;
}

function buildApplySql(decisions, opts) {
  const now = new Date().toISOString();
  const chunks = ['BEGIN;'];
  for (const d of decisions) {
    chunks.push(`UPDATE knowledge_review_state SET is_current = FALSE WHERE knowledge_id = ${sqlString(d.knowledge_id)} AND is_current = TRUE;`);
    chunks.push(`INSERT INTO knowledge_review_state
      (id, knowledge_id, state, batch_id, reviewer, rationale, evidence_refs, superseded_by, confidence, reviewed_at, applied_at, is_current)
      VALUES (${sqlString(crypto.randomUUID())}, ${sqlString(d.knowledge_id)}, ${sqlString(d.state)}, ${sqlString(opts.batchId)}, 'codex-memory-triage', ${sqlString(d.rationale)}, ${sqlJson(d.evidence_refs)}, ${sqlString(d.superseded_by)}, ${Number(d.confidence).toFixed(2)}, ${sqlString(now)}, ${sqlString(now)}, TRUE);`);
  }
  chunks.push('COMMIT;');
  return chunks.join('\n');
}

async function main() {
  const opts = parseArgs(process.argv.slice(2));
  const rows = loadRows(opts);
  const decisions = rows.map((row) => {
    const c = classify(row);
    return {
      knowledge_id: row.id,
      category: row.category,
      key: row.key,
      project_id: row.project_id,
      state: c.state,
      confidence: c.confidence,
      rationale: c.rationale,
      evidence_refs: c.evidence_refs ?? [],
      superseded_by: c.superseded_by ?? null,
      access_count: row.access_count ?? 0,
    };
  });

  const report = writeReports(rows, decisions, opts);
  if (opts.apply) {
    const sql = buildApplySql(decisions, opts);
    fs.writeFileSync(path.join(opts.outDir, 'applied-review-overlay.sql'), sql);
    psql(opts.db, null, { input: sql });
  }
  console.log(JSON.stringify(report, null, 2));
}

main().catch((err) => {
  console.error(err.stack || err.message);
  process.exit(1);
});
