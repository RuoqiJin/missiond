#!/usr/bin/env node

import path from 'node:path';
import {
  head,
  isList,
  keywordPropText,
  nodeText,
  nodeToStringArray,
  parseLisp,
  readKeywordProps,
  readLispFile,
} from './lib/missiond_lisp.mjs';

const usage = `Usage:
  node scripts/check-router-policy.mjs [--json] [--dry-fixture] <policy.lisp...>

Checks MissionD router-policy v1 Lisp files:
  - validates reader syntax through the shared MissionD Lisp parser
  - validates the (router-policy <id> ...) header: :schema, :version,
    :dry-run-only true, :runtime-replacement false
  - validates each (rule ...) entry: required :id :priority :when
    :recommend :non-goals; optional :notes
  - rejects: schema mismatch, missing/false :dry-run-only,
    missing/true :runtime-replacement, duplicate :id, duplicate :priority,
    unknown backend in :recommend, missing :non-goals, malformed predicates,
    unknown entry head, unknown rule field

Cross-wave invariant enforced: every policy file MUST declare
:dry-run-only true AND :runtime-replacement false. Router output is
advisory only; this checker rejects any policy that claims runtime
replacement.

Use --dry-fixture to run self-contained pass/fail fixtures.
`;

export const SCHEMA = 'missiond.router-policy.v1';

const ID_RE = /^[a-z0-9][a-z0-9._-]*$/;

export const POLICY_HEAD = 'router-policy';
export const RULE_HEAD = 'rule';

export const BACKEND_CLASSES = new Set([
  'claudecode',
  'missiond-llm-router',
  'deterministic-checker',
  'patch-worker',
  'verifier-worker',
]);

export const PREDICATE_HEADS = new Set([
  'kind',
  'dispatch_strategy',
  'dispatch-strategy',
  'owner',
  'status',
  'path-glob',
  'any',
  'all',
]);

const HEADER_REQUIRED = [':schema', ':version', ':dry-run-only', ':runtime-replacement'];
const HEADER_OPTIONAL = new Set([':description']);
const HEADER_ALLOWED = new Set([...HEADER_REQUIRED, ...HEADER_OPTIONAL]);

const RULE_REQUIRED = [':id', ':priority', ':when', ':recommend', ':non-goals'];
const RULE_OPTIONAL = [':notes'];
const RULE_ALLOWED = new Set([...RULE_REQUIRED, ...RULE_OPTIONAL]);

const RECOMMEND_REQUIRED = [':backend', ':reasoning'];
const RECOMMEND_ALLOWED = new Set(RECOMMEND_REQUIRED);

function main() {
  const args = process.argv.slice(2);
  let json = false;
  let dryFixture = false;
  const inputs = [];

  for (const arg of args) {
    if (arg === '-h' || arg === '--help') {
      console.log(usage);
      process.exit(0);
    } else if (arg === '--json') {
      json = true;
    } else if (arg === '--dry-fixture') {
      dryFixture = true;
    } else {
      inputs.push(arg);
    }
  }

  if (dryFixture) {
    runFixtures(json);
    return;
  }

  if (inputs.length === 0) {
    console.error(usage);
    process.exit(2);
  }

  const cwd = process.cwd();
  const files = inputs.map((input) => path.resolve(cwd, input));

  const diagnostics = [];
  let policyCount = 0;
  let rulesValidated = 0;

  for (const file of [...new Set(files)]) {
    let forms;
    try {
      forms = readLispFile(file);
    } catch (err) {
      diagnostics.push({
        severity: 'error',
        file,
        line: err.line ?? 1,
        column: err.column ?? 1,
        message: err.message,
      });
      continue;
    }
    for (const form of forms) {
      if (!isList(form) || head(form) !== POLICY_HEAD) continue;
      policyCount += 1;
      rulesValidated += validatePolicy(file, form, diagnostics);
    }
  }

  const errors = diagnostics.filter((d) => d.severity === 'error');
  const ok = errors.length === 0;

  if (json) {
    console.log(
      JSON.stringify(
        {
          ok,
          files: files.length,
          policies: policyCount,
          rules_validated: rulesValidated,
          errors,
        },
        null,
        2,
      ),
    );
  } else if (ok) {
    console.log(
      `router-policy check OK (${policyCount} polic${policyCount === 1 ? 'y' : 'ies'}, ${rulesValidated} rule${rulesValidated === 1 ? '' : 's'})`,
    );
  } else {
    for (const d of diagnostics) {
      console.error(`${d.file}:${d.line}:${d.column}: ${d.severity}: ${d.message}`);
    }
    console.error(
      `router-policy check FAILED — ${errors.length} error(s), ${policyCount} polic${policyCount === 1 ? 'y' : 'ies'}, ${rulesValidated} rule(s)`,
    );
  }
  process.exit(ok ? 0 : 1);
}

function validatePolicy(file, policy, diagnostics) {
  const policyId = nodeText(policy.children[1]);
  if (!policyId) {
    addError(diagnostics, file, policy.loc, '(router-policy ...) missing policy id as second form');
  } else if (!ID_RE.test(policyId)) {
    addError(
      diagnostics,
      file,
      policy.children[1].loc,
      `policy id "${policyId}" must match ${ID_RE}`,
    );
  }

  const props = readKeywordProps(policy, { start: 2 });
  for (const field of HEADER_REQUIRED) {
    if (!props[field]) {
      addError(
        diagnostics,
        file,
        policy.loc,
        `(${POLICY_HEAD} ${policyId ?? '<missing>'} ...) missing required header ${field}`,
      );
    }
  }
  for (const key of Object.keys(props)) {
    if (!HEADER_ALLOWED.has(key)) {
      addError(
        diagnostics,
        file,
        props[key].keyNode.loc,
        `(${POLICY_HEAD} ...) has unknown header ${key}`,
      );
    }
  }

  const schema = keywordPropText(props, ':schema');
  if (schema !== SCHEMA) {
    addError(
      diagnostics,
      file,
      props[':schema']?.value?.loc ?? policy.loc,
      `:schema must be "${SCHEMA}", got ${JSON.stringify(schema)}`,
    );
  }

  // Cross-wave invariant: dry-run-only must be literal `true`.
  // We require the exact atom `true` (not yes/on) so the data contract is
  // unambiguous and the checker is the single source of truth for what the
  // invariant looks like on disk.
  if (props[':dry-run-only']) {
    const text = nodeText(props[':dry-run-only'].value);
    if (text !== 'true') {
      addError(
        diagnostics,
        file,
        props[':dry-run-only'].value.loc,
        `:dry-run-only must be literal true; got ${JSON.stringify(text)}. Router policy is advisory only — runtime dispatch is not driven by this file.`,
      );
    }
  }

  // Cross-wave invariant: runtime-replacement must be literal `false`.
  // A policy claiming runtime replacement (true) is a structural error: the
  // checker is the line of defence that prevents drift toward live dispatch.
  if (props[':runtime-replacement']) {
    const text = nodeText(props[':runtime-replacement'].value);
    if (text !== 'false') {
      addError(
        diagnostics,
        file,
        props[':runtime-replacement'].value.loc,
        `:runtime-replacement must be literal false; got ${JSON.stringify(text)}. Router policy is advisory only — any policy claiming runtime replacement is rejected.`,
      );
    }
  }

  // Reject any list child whose head is not `rule` (only `rule` allowed at
  // the top level). Header keywords are not lists so they pass through.
  for (const child of policy.children) {
    if (!isList(child)) continue;
    const h = head(child);
    if (h !== RULE_HEAD) {
      addError(
        diagnostics,
        file,
        child.loc,
        `unknown entry head "${h}"; expected "${RULE_HEAD}"`,
      );
    }
  }

  const rules = policy.children.filter((c) => isList(c) && head(c) === RULE_HEAD);
  const seenIds = new Map();
  const seenPriorities = new Map();
  for (const rule of rules) {
    const result = validateRule(file, rule, diagnostics);
    if (result.id) {
      if (seenIds.has(result.id)) {
        addError(
          diagnostics,
          file,
          rule.loc,
          `duplicate rule :id "${result.id}" (also at line ${seenIds.get(result.id)})`,
        );
      } else {
        seenIds.set(result.id, rule.loc?.line ?? 1);
      }
    }
    if (result.priority != null) {
      if (seenPriorities.has(result.priority)) {
        addError(
          diagnostics,
          file,
          rule.loc,
          `duplicate rule :priority ${result.priority} (also at line ${seenPriorities.get(result.priority)})`,
        );
      } else {
        seenPriorities.set(result.priority, rule.loc?.line ?? 1);
      }
    }
  }

  return rules.length;
}

function validateRule(file, rule, diagnostics) {
  const props = readKeywordProps(rule, { start: 1 });

  for (const field of RULE_REQUIRED) {
    if (!props[field]) {
      addError(
        diagnostics,
        file,
        rule.loc,
        `(${RULE_HEAD} ...) missing required field ${field}`,
      );
    }
  }
  for (const key of Object.keys(props)) {
    if (!RULE_ALLOWED.has(key)) {
      addError(
        diagnostics,
        file,
        props[key].keyNode.loc,
        `(${RULE_HEAD} ...) has unknown field ${key}`,
      );
    }
  }

  const id = keywordPropText(props, ':id');
  if (id != null && !ID_RE.test(id)) {
    addError(
      diagnostics,
      file,
      props[':id'].value.loc,
      `rule :id "${id}" must match ${ID_RE}`,
    );
  }

  let priority = null;
  if (props[':priority']) {
    const text = keywordPropText(props, ':priority');
    if (text == null || !/^\d+$/.test(text)) {
      addError(
        diagnostics,
        file,
        props[':priority'].value.loc,
        `rule :priority must be a non-negative integer, got ${JSON.stringify(text)}`,
      );
    } else {
      priority = Number.parseInt(text, 10);
    }
  }

  validateWhen(file, props, diagnostics, rule);
  validateRecommend(file, props, diagnostics, rule);
  validateNonGoals(file, props, diagnostics, rule);

  if (props[':notes']) {
    const notes = keywordPropText(props, ':notes');
    if (notes != null && notes.trim() === '') {
      addError(
        diagnostics,
        file,
        props[':notes'].value.loc,
        'rule :notes must be a non-empty string when present (omit the field if empty)',
      );
    }
  }

  return { id, priority };
}

function validateWhen(file, props, diagnostics, rule) {
  if (!props[':when']) return; // missing-required already reported
  const node = props[':when'].value;
  if (!isList(node)) {
    addError(
      diagnostics,
      file,
      node?.loc ?? rule.loc,
      'rule :when must be a non-empty list of predicate clauses',
    );
    return;
  }
  if (node.children.length === 0) {
    addError(
      diagnostics,
      file,
      node.loc,
      'rule :when must contain at least one predicate clause',
    );
    return;
  }
  for (const clause of node.children) {
    validatePredicateClause(file, clause, diagnostics);
  }
}

function validatePredicateClause(file, clause, diagnostics) {
  if (!isList(clause)) {
    addError(
      diagnostics,
      file,
      clause?.loc,
      `predicate clause must be a list (e.g. (kind docs)), got ${clause?.type ?? 'nothing'}`,
    );
    return;
  }
  if (clause.children.length === 0) {
    addError(diagnostics, file, clause.loc, 'predicate clause is empty');
    return;
  }
  const h = head(clause);
  if (h == null) {
    addError(diagnostics, file, clause.loc, 'predicate clause missing head atom');
    return;
  }
  if (!PREDICATE_HEADS.has(h)) {
    addError(
      diagnostics,
      file,
      clause.children[0].loc,
      `predicate head "${h}" not in ${[...PREDICATE_HEADS].join('|')}`,
    );
    return;
  }
  // For composite predicates (any/all), nested clauses must also be lists.
  if (h === 'any' || h === 'all') {
    if (clause.children.length < 2) {
      addError(
        diagnostics,
        file,
        clause.loc,
        `predicate "${h}" must wrap at least one nested clause`,
      );
      return;
    }
    for (let i = 1; i < clause.children.length; i++) {
      validatePredicateClause(file, clause.children[i], diagnostics);
    }
    return;
  }
  // Leaf predicates require a payload atom/string after the head.
  if (clause.children.length < 2) {
    addError(
      diagnostics,
      file,
      clause.loc,
      `predicate "${h}" missing payload (e.g. (${h} <pattern>))`,
    );
  }
}

function validateRecommend(file, props, diagnostics, rule) {
  if (!props[':recommend']) return;
  const node = props[':recommend'].value;
  if (!isList(node)) {
    addError(
      diagnostics,
      file,
      node?.loc ?? rule.loc,
      'rule :recommend must be a property list (:backend <class> :reasoning <string>)',
    );
    return;
  }
  const recProps = readKeywordProps(node, { start: 0 });
  for (const field of RECOMMEND_REQUIRED) {
    if (!recProps[field]) {
      addError(
        diagnostics,
        file,
        node.loc,
        `rule :recommend missing required field ${field}`,
      );
    }
  }
  for (const key of Object.keys(recProps)) {
    if (!RECOMMEND_ALLOWED.has(key)) {
      addError(
        diagnostics,
        file,
        recProps[key].keyNode.loc,
        `rule :recommend has unknown field ${key}`,
      );
    }
  }
  const backend = keywordPropText(recProps, ':backend');
  if (backend != null && !BACKEND_CLASSES.has(backend)) {
    addError(
      diagnostics,
      file,
      recProps[':backend'].value.loc,
      `rule :recommend :backend "${backend}" must be one of ${[...BACKEND_CLASSES].join('|')}`,
    );
  }
  const reasoning = keywordPropText(recProps, ':reasoning');
  if (reasoning != null && reasoning.trim() === '') {
    addError(
      diagnostics,
      file,
      recProps[':reasoning'].value.loc,
      'rule :recommend :reasoning must be a non-empty string',
    );
  }
}

function validateNonGoals(file, props, diagnostics, rule) {
  if (!props[':non-goals']) return;
  const node = props[':non-goals'].value;
  if (!isList(node)) {
    addError(
      diagnostics,
      file,
      node?.loc ?? rule.loc,
      'rule :non-goals must be a vector/list of strings (every rule must list at least one explicit non-goal)',
    );
    return;
  }
  const items = nodeToStringArray(node);
  if (items.length === 0) {
    addError(
      diagnostics,
      file,
      node.loc,
      'rule :non-goals must contain at least one entry — explicit non-goals are required so reviewers can audit drift toward live dispatch',
    );
    return;
  }
  for (const it of items) {
    if (typeof it !== 'string' || it.trim() === '') {
      addError(diagnostics, file, node.loc, 'rule :non-goals entries must be non-empty strings');
    }
  }
}

function addError(diagnostics, file, loc, message) {
  diagnostics.push({
    severity: 'error',
    file,
    line: loc?.line ?? 1,
    column: loc?.column ?? 1,
    message,
  });
}

function runFixtures(json = false) {
  const fixtures = [
    {
      name: 'valid minimal policy',
      category: 'pass',
      source: `(router-policy seed-min
        :schema "missiond.router-policy.v1"
        :version "v1"
        :dry-run-only true
        :runtime-replacement false
        (rule
          :id r-1
          :priority 10
          :when ((kind docs))
          :recommend (:backend claudecode :reasoning "interactive editor")
          :non-goals ["does not replace runtime dispatch"]))`,
      ok: true,
    },
    {
      name: 'all backend classes accepted',
      category: 'pass',
      source: buildAllBackendsPolicy(),
      ok: true,
    },
    {
      name: 'composite predicates (any/all)',
      category: 'pass',
      source: `(router-policy seed-composite
        :schema "missiond.router-policy.v1"
        :version "v1"
        :dry-run-only true
        :runtime-replacement false
        (rule
          :id r-composite
          :priority 5
          :when ((all (kind code-alignment)
                      (any (owner "claudecode")
                           (owner "patch-worker"))))
          :recommend (:backend deterministic-checker :reasoning "scripted acceptance")
          :non-goals ["does not replace runtime dispatch"]))`,
      ok: true,
    },
    {
      name: 'wrong schema',
      category: 'fail-schema',
      source: `(router-policy bad-schema
        :schema "missiond.router-policy.v0"
        :version "v0"
        :dry-run-only true
        :runtime-replacement false)`,
      ok: false,
    },
    {
      name: 'missing :dry-run-only',
      category: 'fail-dry-run-only',
      source: `(router-policy bad-no-dryrun
        :schema "missiond.router-policy.v1"
        :version "v1"
        :runtime-replacement false
        (rule
          :id r-1
          :priority 1
          :when ((kind docs))
          :recommend (:backend claudecode :reasoning "ok")
          :non-goals ["does not replace runtime dispatch"]))`,
      ok: false,
    },
    {
      name: ':dry-run-only false rejected',
      category: 'fail-dry-run-only',
      source: `(router-policy bad-dryrun-false
        :schema "missiond.router-policy.v1"
        :version "v1"
        :dry-run-only false
        :runtime-replacement false
        (rule
          :id r-1
          :priority 1
          :when ((kind docs))
          :recommend (:backend claudecode :reasoning "ok")
          :non-goals ["does not replace runtime dispatch"]))`,
      ok: false,
    },
    {
      name: 'runtime-replacement true REJECTED (cross-wave invariant)',
      category: 'fail-runtime-replacement',
      source: `(router-policy bad-runtime-replacement
        :schema "missiond.router-policy.v1"
        :version "v1"
        :dry-run-only true
        :runtime-replacement true
        (rule
          :id r-1
          :priority 1
          :when ((kind docs))
          :recommend (:backend claudecode :reasoning "ok")
          :non-goals ["does not replace runtime dispatch"]))`,
      ok: false,
    },
    {
      name: 'missing :runtime-replacement header',
      category: 'fail-runtime-replacement',
      source: `(router-policy bad-no-rr
        :schema "missiond.router-policy.v1"
        :version "v1"
        :dry-run-only true
        (rule
          :id r-1
          :priority 1
          :when ((kind docs))
          :recommend (:backend claudecode :reasoning "ok")
          :non-goals ["does not replace runtime dispatch"]))`,
      ok: false,
    },
    {
      name: 'duplicate rule :id',
      category: 'fail-duplicate-id',
      source: `(router-policy dup-id
        :schema "missiond.router-policy.v1"
        :version "v1"
        :dry-run-only true
        :runtime-replacement false
        (rule
          :id r-dup
          :priority 1
          :when ((kind docs))
          :recommend (:backend claudecode :reasoning "ok")
          :non-goals ["does not replace runtime dispatch"])
        (rule
          :id r-dup
          :priority 2
          :when ((kind review))
          :recommend (:backend verifier-worker :reasoning "ok")
          :non-goals ["does not replace runtime dispatch"]))`,
      ok: false,
    },
    {
      name: 'duplicate rule :priority',
      category: 'fail-duplicate-priority',
      source: `(router-policy dup-prio
        :schema "missiond.router-policy.v1"
        :version "v1"
        :dry-run-only true
        :runtime-replacement false
        (rule
          :id r-a
          :priority 7
          :when ((kind docs))
          :recommend (:backend claudecode :reasoning "ok")
          :non-goals ["does not replace runtime dispatch"])
        (rule
          :id r-b
          :priority 7
          :when ((kind review))
          :recommend (:backend verifier-worker :reasoning "ok")
          :non-goals ["does not replace runtime dispatch"]))`,
      ok: false,
    },
    {
      name: 'unknown backend in :recommend',
      category: 'fail-unknown-backend',
      source: `(router-policy bad-backend
        :schema "missiond.router-policy.v1"
        :version "v1"
        :dry-run-only true
        :runtime-replacement false
        (rule
          :id r-bad
          :priority 1
          :when ((kind docs))
          :recommend (:backend gpt-5 :reasoning "ok")
          :non-goals ["does not replace runtime dispatch"]))`,
      ok: false,
    },
    {
      name: 'missing :non-goals on rule',
      category: 'fail-missing-non-goals',
      source: `(router-policy no-nongoals
        :schema "missiond.router-policy.v1"
        :version "v1"
        :dry-run-only true
        :runtime-replacement false
        (rule
          :id r-nogoals
          :priority 1
          :when ((kind docs))
          :recommend (:backend claudecode :reasoning "ok")))`,
      ok: false,
    },
    {
      name: 'empty :non-goals vector',
      category: 'fail-missing-non-goals',
      source: `(router-policy empty-nongoals
        :schema "missiond.router-policy.v1"
        :version "v1"
        :dry-run-only true
        :runtime-replacement false
        (rule
          :id r-empty
          :priority 1
          :when ((kind docs))
          :recommend (:backend claudecode :reasoning "ok")
          :non-goals []))`,
      ok: false,
    },
    {
      name: 'malformed predicate (non-list clause)',
      category: 'fail-malformed-predicate',
      source: `(router-policy bad-pred
        :schema "missiond.router-policy.v1"
        :version "v1"
        :dry-run-only true
        :runtime-replacement false
        (rule
          :id r-bad-pred
          :priority 1
          :when (kind)
          :recommend (:backend claudecode :reasoning "ok")
          :non-goals ["does not replace runtime dispatch"]))`,
      ok: false,
    },
    {
      name: 'malformed predicate (unknown head)',
      category: 'fail-malformed-predicate',
      source: `(router-policy bad-pred-head
        :schema "missiond.router-policy.v1"
        :version "v1"
        :dry-run-only true
        :runtime-replacement false
        (rule
          :id r-bad-pred-head
          :priority 1
          :when ((wave wave24))
          :recommend (:backend claudecode :reasoning "ok")
          :non-goals ["does not replace runtime dispatch"]))`,
      ok: false,
    },
    {
      name: 'unknown entry head at top level',
      category: 'fail-unknown-entry-head',
      source: `(router-policy bad-entry
        :schema "missiond.router-policy.v1"
        :version "v1"
        :dry-run-only true
        :runtime-replacement false
        (recommendation
          :id r-1
          :priority 1
          :when ((kind docs))
          :recommend (:backend claudecode :reasoning "ok")
          :non-goals ["does not replace runtime dispatch"]))`,
      ok: false,
    },
    {
      name: 'unknown rule field',
      category: 'fail-unknown-field',
      source: `(router-policy unknown-field
        :schema "missiond.router-policy.v1"
        :version "v1"
        :dry-run-only true
        :runtime-replacement false
        (rule
          :id r-x
          :priority 1
          :when ((kind docs))
          :recommend (:backend claudecode :reasoning "ok")
          :non-goals ["does not replace runtime dispatch"]
          :extra "nope"))`,
      ok: false,
    },
    {
      name: 'missing :recommend :backend',
      category: 'fail-recommend',
      source: `(router-policy bad-recommend
        :schema "missiond.router-policy.v1"
        :version "v1"
        :dry-run-only true
        :runtime-replacement false
        (rule
          :id r-no-backend
          :priority 1
          :when ((kind docs))
          :recommend (:reasoning "ok")
          :non-goals ["does not replace runtime dispatch"]))`,
      ok: false,
    },
    {
      name: 'malformed :priority (non-integer)',
      category: 'fail-priority',
      source: `(router-policy bad-priority
        :schema "missiond.router-policy.v1"
        :version "v1"
        :dry-run-only true
        :runtime-replacement false
        (rule
          :id r-bad-prio
          :priority high
          :when ((kind docs))
          :recommend (:backend claudecode :reasoning "ok")
          :non-goals ["does not replace runtime dispatch"]))`,
      ok: false,
    },
  ];

  let failed = 0;
  const categories = new Set();
  for (const fixture of fixtures) {
    categories.add(fixture.category);
    const file = `<fixture:${fixture.name}>`;
    const diagnostics = [];
    let forms;
    try {
      forms = parseLisp(fixture.source, file);
    } catch (err) {
      diagnostics.push({
        severity: 'error',
        file,
        line: err.line ?? 1,
        column: err.column ?? 1,
        message: err.message,
      });
      forms = [];
    }
    for (const form of forms) {
      if (isList(form) && head(form) === POLICY_HEAD) {
        validatePolicy(file, form, diagnostics);
      }
    }
    const ok = !diagnostics.some((d) => d.severity === 'error');
    if (ok !== fixture.ok) {
      failed += 1;
      console.error(
        `fixture failed: ${fixture.name} (expected ok=${fixture.ok}, got ok=${ok})`,
      );
      for (const d of diagnostics) {
        console.error(`  ${d.file}:${d.line}:${d.column}: ${d.message}`);
      }
    }
  }
  if (json) {
    console.log(
      JSON.stringify(
        {
          ok: failed === 0,
          fixtures: fixtures.length,
          failed,
          categories: [...categories],
        },
        null,
        2,
      ),
    );
  }
  if (failed > 0) {
    console.error(`router-policy fixtures FAILED — ${failed} of ${fixtures.length}`);
    process.exit(1);
  }
  if (!json) {
    console.log(
      `router-policy fixtures OK (${fixtures.length} cases, ${categories.size} categories)`,
    );
  }
}

function buildAllBackendsPolicy() {
  const backends = [
    'claudecode',
    'missiond-llm-router',
    'deterministic-checker',
    'patch-worker',
    'verifier-worker',
  ];
  const rules = backends
    .map((backend, idx) => {
      const priority = (idx + 1) * 10;
      return `(rule
        :id r-backend-${backend}
        :priority ${priority}
        :when ((kind code-alignment))
        :recommend (:backend ${backend} :reasoning "exercise enum slot ${backend}")
        :non-goals ["does not replace runtime dispatch"])`;
    })
    .join('\n        ');
  return `(router-policy all-backends
        :schema "missiond.router-policy.v1"
        :version "v1"
        :dry-run-only true
        :runtime-replacement false
        ${rules})`;
}

// ---------------------------------------------------------------------------
// Named exports for downstream tooling (wave24-03 recommendation CLI).
//
// These helpers expose the already-validated structure of a router-policy
// file as plain JavaScript objects, so a recommendation CLI can reason over
// rules without re-implementing the Lisp reader. The functions are READ-ONLY:
// they parse, project, and return — they do not mutate input nodes and they
// never write files. The CLI surface (main / runFixtures / dry-fixture) is
// intentionally untouched.
// ---------------------------------------------------------------------------

// Project a single (rule ...) form into a structured object suitable for the
// recommendation CLI. The :when tree is preserved as a nested predicate-clause
// shape so callers can evaluate it without re-walking Lisp nodes.
//
// Returned rule shape:
//   { id, priority, when: clause, recommend: { backend, reasoning },
//     non_goals: string[], notes: string|null, loc: { line, column } }
// where `clause` is one of:
//   { head: 'kind'|'dispatch_strategy'|'dispatch-strategy'|'owner'|'status',
//     value: string }
//   { head: 'path-glob', pattern: string }
//   { head: 'any'|'all', children: clause[] }
// or `null` if :when is missing/malformed (the validator already rejects this
// upstream; the projector simply returns null so callers can skip the rule).
export function projectRule(rule) {
  if (!isList(rule) || head(rule) !== RULE_HEAD) return null;
  const props = readKeywordProps(rule, { start: 1 });
  const id = keywordPropText(props, ':id');
  const priorityText = keywordPropText(props, ':priority');
  const priority =
    priorityText != null && /^\d+$/.test(priorityText)
      ? Number.parseInt(priorityText, 10)
      : null;

  const whenNode = props[':when']?.value ?? null;
  const whenClause = whenNode && isList(whenNode) ? projectWhenList(whenNode) : null;

  const recommendNode = props[':recommend']?.value ?? null;
  let recommend = null;
  if (recommendNode && isList(recommendNode)) {
    const recProps = readKeywordProps(recommendNode, { start: 0 });
    recommend = {
      backend: keywordPropText(recProps, ':backend'),
      reasoning: keywordPropText(recProps, ':reasoning'),
    };
  }

  const nonGoalsNode = props[':non-goals']?.value ?? null;
  const nonGoals = nonGoalsNode ? nodeToStringArray(nonGoalsNode) : [];

  const notes = keywordPropText(props, ':notes');

  return {
    id,
    priority,
    when: whenClause,
    recommend,
    non_goals: nonGoals,
    notes: notes ?? null,
    loc: rule.loc ?? null,
  };
}

// Project the top-level :when list (which is implicit-`all` over its children)
// into a single composite clause. Each direct child is a predicate clause; the
// projector wraps them in `{ head: 'all', children: [...] }` so evaluators have
// a uniform tree shape regardless of whether the policy author wrote multiple
// top-level clauses or a single (all ...) clause.
function projectWhenList(node) {
  const children = node.children
    .map((child) => projectClause(child))
    .filter((clause) => clause !== null);
  if (children.length === 1) return children[0];
  return { head: 'all', children };
}

function projectClause(node) {
  if (!isList(node) || node.children.length === 0) return null;
  const h = head(node);
  if (h == null) return null;
  if (h === 'any' || h === 'all') {
    const children = [];
    for (let i = 1; i < node.children.length; i++) {
      const projected = projectClause(node.children[i]);
      if (projected !== null) children.push(projected);
    }
    return { head: h, children };
  }
  if (h === 'path-glob') {
    const payload = node.children[1];
    return { head: 'path-glob', pattern: nodeText(payload) ?? '' };
  }
  if (
    h === 'kind' ||
    h === 'dispatch_strategy' ||
    h === 'dispatch-strategy' ||
    h === 'owner' ||
    h === 'status'
  ) {
    const payload = node.children[1];
    return { head: h, value: nodeText(payload) ?? '' };
  }
  // Unknown head — surface as a clause that always evaluates false in the
  // evaluator; the validator already rejects this upstream so we never get here
  // for files that have passed the checker.
  return { head: h, value: nodeText(node.children[1]) ?? '' };
}

// Project an entire router-policy form into a structured object:
//   { id, schema, version, dry_run_only, runtime_replacement, description,
//     rules: Rule[] (sorted by priority ascending), source_path }
// `dry_run_only` and `runtime_replacement` are booleans projected from the
// literal Lisp atoms. The validator enforces dry_run_only===true and
// runtime_replacement===false; this projector simply reflects what the file
// declares so callers can re-check the cross-wave invariant before consuming.
export function projectPolicy(form, sourcePath = null) {
  if (!isList(form) || head(form) !== POLICY_HEAD) return null;
  const id = nodeText(form.children[1]);
  const props = readKeywordProps(form, { start: 2 });
  const schema = keywordPropText(props, ':schema');
  const version = keywordPropText(props, ':version');
  const dryRunText = nodeText(props[':dry-run-only']?.value);
  const runtimeReplacementText = nodeText(props[':runtime-replacement']?.value);
  const description = keywordPropText(props, ':description');
  const rules = form.children
    .filter((c) => isList(c) && head(c) === RULE_HEAD)
    .map((rule) => projectRule(rule))
    .filter((rule) => rule !== null);
  rules.sort((a, b) => {
    const ap = a.priority ?? Number.MAX_SAFE_INTEGER;
    const bp = b.priority ?? Number.MAX_SAFE_INTEGER;
    return ap - bp;
  });
  return {
    id,
    schema,
    version,
    dry_run_only: dryRunText === 'true',
    runtime_replacement: runtimeReplacementText === 'true',
    description: description ?? null,
    rules,
    source_path: sourcePath,
  };
}

// Read a router-policy Lisp file from disk and return the FIRST projected
// policy form. Throws (with file context) when the reader fails or no
// router-policy form is present. This helper is the simplest entry point for
// the recommendation CLI: it doesn't validate, only projects — callers that
// need validation should run main() / runFixtures() separately.
export function readRouterPolicyFile(file) {
  const forms = readLispFile(file);
  for (const form of forms) {
    if (isList(form) && head(form) === POLICY_HEAD) {
      return projectPolicy(form, file);
    }
  }
  throw new Error(`router-policy: no (router-policy ...) form found in ${file}`);
}

// Only run the CLI when invoked directly. Keeps named exports clean for
// future tooling that wants to reuse SCHEMA / BACKEND_CLASSES / PREDICATE_HEADS
// and the projectPolicy / projectRule / readRouterPolicyFile helpers.
if (import.meta.url === `file://${process.argv[1]}`) {
  main();
}
