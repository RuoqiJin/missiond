#!/usr/bin/env node

import crypto from 'node:crypto';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';

const usage = `Usage:
  node scripts/backfill-board-runtime-metadata.mjs [--apply] [--limit N]

Preview or backfill BoardTask runtime_metadata + task_contracts for the hard-cut control plane.
Runtime code must not parse BoardTask.description; this migration tool may read
legacy descriptions once and materialize typed runtime_metadata, task_contracts,
and grants.
`;

const opts = parseArgs(process.argv.slice(2));
const rows = queryTasks(opts.limit);
const updates = rows.map((task) => buildBackfill(task));

if (!opts.apply) {
  console.log(JSON.stringify({
    schema: 'missiond.board-runtime-metadata-backfill.preview.v1',
    apply: false,
    count: updates.length,
    tasks: updates.map(({ id, title, runtime_metadata, task_contract, grants }) => ({
      id,
      title,
      runtime_metadata,
      task_contract,
      grants: grants.map(({ operation, scope_kind, scope_key }) => ({
        operation,
        scope_kind,
        scope_key,
      })),
    })),
  }, null, 2));
  process.exit(0);
}

applyBackfill(updates);
console.log(JSON.stringify({
  schema: 'missiond.board-runtime-metadata-backfill.apply.v1',
  apply: true,
  count: updates.length,
}, null, 2));

function parseArgs(args) {
  const parsed = { apply: false, limit: 100 };
  for (let i = 0; i < args.length; i += 1) {
    const arg = args[i];
    if (arg === '--help' || arg === '-h') {
      console.log(usage);
      process.exit(0);
    }
    if (arg === '--apply') {
      parsed.apply = true;
      continue;
    }
    if (arg === '--limit') {
      const value = Number(args[++i]);
      if (!Number.isInteger(value) || value <= 0) throw new Error('--limit must be a positive integer');
      parsed.limit = value;
      continue;
    }
    throw new Error(`unknown arg: ${arg}\n${usage}`);
  }
  return parsed;
}

function queryTasks(limit) {
  const sql = `
    SELECT COALESCE(json_agg(row_to_json(t)), '[]'::json)
    FROM (
      SELECT bt.id, bt.title, bt.description, bt.project, bt.category, bt.context_intent, bt.runtime_metadata
      FROM board_tasks bt
      LEFT JOIN task_contracts tc ON tc.task_id = bt.id
      WHERE bt.runtime_metadata IS NULL
         OR bt.runtime_metadata = '{}'::jsonb
         OR NOT (bt.runtime_metadata ? 'task_contract_id')
         OR tc.task_id IS NULL
      ORDER BY bt.created_at ASC
      LIMIT ${Number(limit)}
    ) t
  `;
  const out = runPsql(['-X', '-A', '-t', '-c', sql]);
  return JSON.parse(out.trim() || '[]');
}

function buildBackfill(task) {
  const legacy = parseLegacyDescription(task.description || '');
  const readScope = stringList(legacy.read_scope);
  const writeScope = stringList(legacy.write_scope);
  const mustNotTouch = stringList(legacy.must_not_touch);
  const metadata = {
    ...(isObject(task.runtime_metadata) ? task.runtime_metadata : {}),
    schema: 'missiond.board-task-runtime-metadata.v1',
    source: 'backfill-board-runtime-metadata',
    control_state: 'task_contracts',
    task_contract_id: `board-task:${task.id}`,
    dispatch_metadata: {
      project_id: task.project || null,
      task_id: task.id,
      task_class: legacy.task_class || task.context_intent || task.category || 'general',
      accepted_shard_id: legacy.accepted_shard_id || null,
      context_pack_path: legacy.context_pack_path || null,
      grounding_context_id: legacy.grounding_context_id || null,
      read_scope: readScope,
      write_scope: writeScope,
      must_not_touch: mustNotTouch,
    },
    read_scope: readScope,
    write_scope: writeScope,
    must_not_touch: mustNotTouch,
    capability_grant_ids: [],
    sandbox_profile: writeScope.length > 0 ? 'workspace-write-policy' : 'read-only',
    projection_policy: 'description_notes_are_projection_only',
  };
  const grants = grantsForTask(task, readScope, writeScope, mustNotTouch);
  metadata.capability_grant_ids = grants.map((grant) => grant.id);
  metadata.dispatch_metadata.capability_grant_ids = metadata.capability_grant_ids;
  metadata.dispatch_metadata.sandbox_profile = metadata.sandbox_profile;
  metadata.dispatch_metadata.task_contract_id = metadata.task_contract_id;
  const taskContract = taskContractForBackfill(task, metadata, readScope, writeScope, mustNotTouch);
  return { id: task.id, title: task.title, runtime_metadata: metadata, task_contract: taskContract, grants };
}

function taskContractForBackfill(task, metadata, readScope, writeScope, mustNotTouch) {
  return {
    id: `task-contract:${task.id}`,
    task_id: task.id,
    project_id: task.project || null,
    task_contract_id: metadata.task_contract_id,
    dispatch_metadata: metadata.dispatch_metadata,
    read_scope: readScope,
    write_scope: writeScope,
    must_not_touch: mustNotTouch,
    capability_grant_ids: metadata.capability_grant_ids,
    sandbox_profile: metadata.sandbox_profile,
    completion_materialization_policy: metadata.completion_materialization_policy || null,
    grounding_refs: metadata.grounding_refs || [],
    context_refs: metadata.context_refs || [],
  };
}

function grantsForTask(task, readScope, writeScope, mustNotTouch) {
  const base = {
    subject_kind: 'task',
    subject_id: task.id,
    project_id: task.project || null,
    task_id: task.id,
    issuer: 'backfill-board-runtime-metadata',
  };
  const grants = [];
  for (const scope of readScope) {
    grants.push({ ...base, id: crypto.randomUUID(), operation: 'read', scope_kind: 'path', scope_key: scope, evidence_requirement: null, details: { source: base.issuer } });
  }
  for (const scope of writeScope) {
    grants.push({ ...base, id: crypto.randomUUID(), operation: 'write', scope_kind: 'path', scope_key: scope, evidence_requirement: 'verification_and_changed_paths', details: { source: base.issuer, must_not_touch: mustNotTouch } });
  }
  grants.push({ ...base, id: crypto.randomUUID(), operation: 'write', scope_kind: 'task', scope_key: task.id, evidence_requirement: 'canonical_task_result_artifact', details: { source: base.issuer } });
  grants.push({ ...base, id: crypto.randomUUID(), operation: 'settle', scope_kind: 'task', scope_key: task.id, evidence_requirement: 'canonical_task_result_artifact', details: { source: base.issuer } });
  grants.push({ ...base, id: crypto.randomUUID(), operation: 'claim', scope_kind: 'task', scope_key: task.id, evidence_requirement: null, details: { source: base.issuer } });
  return grants;
}

function parseLegacyDescription(description) {
  const parsed = {};
  try {
    const json = JSON.parse(description);
    if (isObject(json)) {
      Object.assign(parsed, json.dispatch_metadata || json.metadata || {}, json);
    }
  } catch {
    for (const line of description.split(/\r?\n/)) {
      const match = line.trim().match(/^- ([A-Za-z0-9_-]+):\s*(.+?)\s*$/);
      if (!match) continue;
      parsed[match[1]] = parseScalarOrList(match[2]);
    }
  }
  return parsed;
}

function parseScalarOrList(value) {
  const trimmed = value.trim();
  if (trimmed.startsWith('[') && trimmed.endsWith(']')) {
    try {
      const parsed = JSON.parse(trimmed);
      if (Array.isArray(parsed)) return parsed;
    } catch {}
  }
  if (trimmed.includes(',')) return trimmed.split(',').map((part) => part.trim()).filter(Boolean);
  return trimmed;
}

function stringList(value) {
  if (Array.isArray(value)) return value.map(String).map((v) => v.trim()).filter(Boolean);
  if (typeof value === 'string') return value.split(',').map((v) => v.trim()).filter(Boolean);
  return [];
}

function applyBackfill(updates) {
  if (updates.length === 0) return;
  const statements = [];
  for (const update of updates) {
    statements.push(`UPDATE board_tasks SET runtime_metadata = ${sqlString(JSON.stringify(update.runtime_metadata))}::jsonb, updated_at = now() WHERE id = ${sqlString(update.id)};`);
    statements.push(`
      INSERT INTO task_contracts
        (id, task_id, project_id, task_contract_id, dispatch_metadata, read_scope, write_scope, must_not_touch, capability_grant_ids, sandbox_profile, completion_materialization_policy, grounding_refs, context_refs)
      VALUES
        (${sqlString(update.task_contract.id)}, ${sqlString(update.task_contract.task_id)}, ${sqlNullable(update.task_contract.project_id)}, ${sqlString(update.task_contract.task_contract_id)}, ${sqlString(JSON.stringify(update.task_contract.dispatch_metadata))}::jsonb, ${sqlString(JSON.stringify(update.task_contract.read_scope))}::jsonb, ${sqlString(JSON.stringify(update.task_contract.write_scope))}::jsonb, ${sqlString(JSON.stringify(update.task_contract.must_not_touch))}::jsonb, ${sqlString(JSON.stringify(update.task_contract.capability_grant_ids))}::jsonb, ${sqlNullable(update.task_contract.sandbox_profile)}, ${sqlNullable(update.task_contract.completion_materialization_policy)}, ${sqlString(JSON.stringify(update.task_contract.grounding_refs))}::jsonb, ${sqlString(JSON.stringify(update.task_contract.context_refs))}::jsonb)
      ON CONFLICT (task_id)
      DO UPDATE SET project_id = EXCLUDED.project_id,
                    task_contract_id = EXCLUDED.task_contract_id,
                    dispatch_metadata = EXCLUDED.dispatch_metadata,
                    read_scope = EXCLUDED.read_scope,
                    write_scope = EXCLUDED.write_scope,
                    must_not_touch = EXCLUDED.must_not_touch,
                    capability_grant_ids = EXCLUDED.capability_grant_ids,
                    sandbox_profile = EXCLUDED.sandbox_profile,
                    completion_materialization_policy = EXCLUDED.completion_materialization_policy,
                    grounding_refs = EXCLUDED.grounding_refs,
                    context_refs = EXCLUDED.context_refs,
                    updated_at = now();
    `);
    for (const grant of update.grants) {
      statements.push(`
        INSERT INTO capability_grants
          (id, subject_kind, subject_id, operation, scope_kind, scope_key, project_id, task_id, issuer, evidence_requirement, details)
        VALUES
          (${sqlString(grant.id)}, ${sqlString(grant.subject_kind)}, ${sqlString(grant.subject_id)}, ${sqlString(grant.operation)}, ${sqlString(grant.scope_kind)}, ${sqlString(grant.scope_key)}, ${sqlNullable(grant.project_id)}, ${sqlString(grant.task_id)}, ${sqlString(grant.issuer)}, ${sqlNullable(grant.evidence_requirement)}, ${sqlString(JSON.stringify(grant.details))}::jsonb)
        ON CONFLICT (id) DO NOTHING;
      `);
    }
  }
  const file = path.join(os.tmpdir(), `missiond-board-runtime-backfill-${process.pid}.sql`);
  fs.writeFileSync(file, `BEGIN;\n${statements.join('\n')}\nCOMMIT;\n`);
  try {
    runPsql(['-X', '-v', 'ON_ERROR_STOP=1', '-f', file]);
  } finally {
    fs.rmSync(file, { force: true });
  }
}

function runPsql(args) {
  const env = { ...process.env };
  if (!env.DATABASE_URL && env.MISSIOND_DATABASE_URL) env.DATABASE_URL = env.MISSIOND_DATABASE_URL;
  const result = spawnSync('psql', args, { env, encoding: 'utf8' });
  if (result.status !== 0) {
    throw new Error(result.stderr || result.stdout || `psql exited ${result.status}`);
  }
  return result.stdout;
}

function sqlString(value) {
  return `'${String(value).replaceAll("'", "''")}'`;
}

function sqlNullable(value) {
  return value == null || value === '' ? 'NULL' : sqlString(value);
}

function isObject(value) {
  return value && typeof value === 'object' && !Array.isArray(value);
}
