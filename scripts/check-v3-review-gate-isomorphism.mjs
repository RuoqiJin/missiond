#!/usr/bin/env node

import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';

const usage = `Usage:
  node scripts/check-v3-review-gate-isomorphism.mjs [--json] [--dry-fixture]

Checks the V3 review-gate Lisp/code isomorphism contract:
  - V3 blueprint declares (surface review-gate ...) with :status "code-aligned",
    :code paths covering review_gate.rs + the directive/plan/workflow callers
    + matching MCP tool schemas, and a :note that names alignment-review-gate
    + plan-review-gate + the two-gate-default / never-auto-approve semantics.
  - compression-contract :checks pins this checker.
  - review_gate.rs exposes the stable handler surface: ReviewGatePolicy
    {Manual|EmitQuestion|Off}, ReviewDecision {Approved|Rejected|NeedsChanges},
    parse_compile_review_gate / parse_review_gate_policy, the
    apply_compile_review_gates dispatcher, and the
    maybe_emit_review_question_{created,resolved} +
    auto_emit_review_question_after_artifact_write helpers (never blocks the
    primary action, never auto-approves, surfaces review_question_warning on
    bus failure).
  - directive.rs / plan.rs / workflow.rs go through that dispatcher (no bypass)
    and call maybe_emit_review_question_resolved on approve/reject/needs_changes.
  - The MCP directive / plan / workflow tools expose review_gate_policy with
    the manual|emit_question|off enum AND the wave-15 review_decision with
    the approved|rejected|needs_changes enum.
`;

const DEFAULT_FILES = {
  blueprint: '.missiond/v3/missiond-blueprint.lisp',
  reviewGate: 'crates/missiond-daemon/src/handlers/knowledge/review_gate.rs',
  directive: 'crates/missiond-daemon/src/handlers/knowledge/directive.rs',
  plan: 'crates/missiond-daemon/src/handlers/knowledge/plan.rs',
  planCompileAuthoring: 'crates/missiond-daemon/src/handlers/knowledge/plan/compile_authoring.rs',
  workflow: 'crates/missiond-daemon/src/handlers/knowledge/workflow.rs',
  mcpDirective: 'crates/missiond-mcp/src/tools/knowledge/directive.rs',
  mcpPlan: 'crates/missiond-mcp/src/tools/knowledge/plan.rs',
  mcpWorkflow: 'crates/missiond-mcp/src/tools/knowledge/workflow.rs',
};

function main() {
  const args = process.argv.slice(2);
  let json = false;
  let dryFixture = false;
  for (const arg of args) {
    if (arg === '--help' || arg === '-h') {
      console.log(usage);
      process.exit(0);
    } else if (arg === '--json') {
      json = true;
    } else if (arg === '--dry-fixture') {
      dryFixture = true;
    } else {
      console.error(`unknown arg: ${arg}`);
      console.error(usage);
      process.exit(2);
    }
  }

  if (dryFixture) {
    runFixtures(json);
    return;
  }

  const repoRoot = process.cwd();
  const diagnostics = checkFiles(repoRoot, DEFAULT_FILES);
  const result = {
    ok: diagnostics.length === 0,
    files: Object.keys(DEFAULT_FILES).length,
    diagnostics,
  };

  if (json) {
    console.log(JSON.stringify(result, null, 2));
  } else if (result.ok) {
    console.log('v3 review-gate Lisp/code isomorphism check OK');
  } else {
    for (const d of diagnostics) {
      console.error(`${d.file}: ${d.message}`);
    }
    console.error(
      `v3 review-gate Lisp/code isomorphism check FAILED -- ${diagnostics.length} diagnostic(s)`,
    );
  }

  process.exit(result.ok ? 0 : 1);
}

const BLUEPRINT_NEEDLES = [
  '(surface review-gate',
  ':status "code-aligned"',
  'crates/missiond-daemon/src/handlers/knowledge/review_gate.rs',
  'crates/missiond-daemon/src/handlers/knowledge/directive.rs',
  'crates/missiond-daemon/src/handlers/knowledge/plan.rs',
  'crates/missiond-daemon/src/handlers/knowledge/plan/compile_authoring.rs',
  'crates/missiond-daemon/src/handlers/knowledge/workflow.rs',
  'crates/missiond-mcp/src/tools/knowledge/directive.rs',
  'crates/missiond-mcp/src/tools/knowledge/plan.rs',
  'crates/missiond-mcp/src/tools/knowledge/workflow.rs',
  'alignment-review-gate',
  'plan-review-gate',
  'two-gate-default',
  'never auto-approve',
  'manual',
  'emit_question',
  'off',
  'approved',
  'rejected',
  'needs_changes',
  'review_question_warning',
  'node scripts/check-v3-review-gate-isomorphism.mjs',
];

const REVIEW_GATE_RS_NEEDLES = [
  'pub(crate) enum ReviewGatePolicy',
  'ReviewGatePolicy::Manual',
  'ReviewGatePolicy::EmitQuestion',
  'ReviewGatePolicy::Off',
  'pub(crate) enum ReviewDecision',
  'ReviewDecision::Approved',
  'ReviewDecision::Rejected',
  'ReviewDecision::NeedsChanges',
  'pub(crate) fn parse_compile_review_gate',
  'pub(crate) fn parse_review_gate_policy',
  'pub(crate) fn review_gate_policy_was_explicit',
  'pub(crate) fn derive_review_question_id',
  'pub(crate) fn derive_review_question_id_for_artifact',
  'pub(crate) async fn apply_compile_review_gates',
  'pub(crate) async fn maybe_emit_review_question_created',
  'pub(crate) async fn maybe_emit_review_question_resolved',
  'pub(crate) async fn auto_emit_review_question_after_artifact_write',
  'pub(crate) const PLAN_NODE_REVIEW_DEFAULT_ACTION',
  'review_question_warning',
  'BUS_PUBLISH_FAILED',
  'persisted artifact remains intact',
  'DB action already committed',
];

const DIRECTIVE_RS_NEEDLES = [
  'use crate::handlers::knowledge::review_gate::',
  'parse_review_gate_policy(args)',
  'review_gate_policy_was_explicit(args)',
  'parse_compile_review_gate(args)',
  'apply_compile_review_gates(',
  'maybe_emit_review_question_resolved',
];

const PLAN_RS_NEEDLES = [
  'use crate::handlers::knowledge::review_gate::',
  'maybe_emit_review_question_resolved',
];

const PLAN_COMPILE_AUTHORING_RS_NEEDLES = [
  'parse_review_gate_policy(args)',
  'review_gate_policy_was_explicit(args)',
  'parse_compile_review_gate(args)',
  'apply_compile_review_gates(',
];

const WORKFLOW_RS_NEEDLES = [
  'use crate::handlers::knowledge::review_gate::',
  'parse_review_gate_policy(args)',
  'review_gate_policy_was_explicit(args)',
  'parse_compile_review_gate(args)',
  'apply_compile_review_gates(',
];

const MCP_DIRECTIVE_NEEDLES = [
  '"review_gate_policy"',
  '"manual"',
  '"emit_question"',
  '"off"',
  '"review_question_id"',
  '"review_decision"',
  'approved',
  'rejected',
  'needs_changes',
  'never auto-approve',
];

const MCP_PLAN_NEEDLES = [
  '"review_gate_policy"',
  '"manual"',
  '"emit_question"',
  '"off"',
  '"review_question_id"',
  '"review_decision"',
  'approved',
  'rejected',
  'needs_changes',
  'never auto-approve',
];

const MCP_WORKFLOW_NEEDLES = [
  '"review_gate_policy"',
  'manual',
  'emit_question',
  'off',
  '"review_question_id"',
  '"review_decision"',
  'approved',
  'rejected',
  'needs_changes',
  'NEVER auto-rejects',
];

function checkFiles(root, files) {
  const diagnostics = [];
  const sources = {};
  for (const [key, rel] of Object.entries(files)) {
    const abs = path.join(root, rel);
    try {
      sources[key] = fs.readFileSync(abs, 'utf8');
    } catch (err) {
      diagnostics.push({ file: rel, message: `cannot read: ${err.message}` });
    }
  }
  if (diagnostics.length > 0) return diagnostics;

  requireAll(diagnostics, files.blueprint, sources.blueprint, BLUEPRINT_NEEDLES);
  // The (surface review-gate ...) note must explicitly anchor the
  // never-auto-approve invariant near the surface declaration so the V3
  // gate's promise (intent/plan review never bypassed) is not buried in
  // unrelated text.
  requireSurfaceNoteContains(
    diagnostics,
    files.blueprint,
    sources.blueprint,
    'review-gate',
    ['alignment-review-gate', 'plan-review-gate', 'never auto-approve'],
  );

  requireAll(diagnostics, files.reviewGate, sources.reviewGate, REVIEW_GATE_RS_NEEDLES);
  requireAll(diagnostics, files.directive, sources.directive, DIRECTIVE_RS_NEEDLES);
  requireAll(diagnostics, files.plan, sources.plan, PLAN_RS_NEEDLES);
  requireAll(diagnostics, files.planCompileAuthoring, sources.planCompileAuthoring, PLAN_COMPILE_AUTHORING_RS_NEEDLES);
  requireAll(diagnostics, files.workflow, sources.workflow, WORKFLOW_RS_NEEDLES);
  requireAll(diagnostics, files.mcpDirective, sources.mcpDirective, MCP_DIRECTIVE_NEEDLES);
  requireAll(diagnostics, files.mcpPlan, sources.mcpPlan, MCP_PLAN_NEEDLES);
  requireAll(diagnostics, files.mcpWorkflow, sources.mcpWorkflow, MCP_WORKFLOW_NEEDLES);

  return diagnostics;
}

function requireAll(diagnostics, file, source, needles) {
  for (const needle of needles) {
    if (!source.includes(needle)) {
      diagnostics.push({ file, message: `missing required contract text: ${needle}` });
    }
  }
}

// Find the (surface <name> ...) form by simple paren matching and assert that
// every needle appears inside its body. This is intentionally narrower than a
// whole-file `requireAll`: the V3 contract is that the named surface itself
// carries the semantics, not some unrelated section that happens to mention
// the same words.
function requireSurfaceNoteContains(diagnostics, file, source, surfaceName, needles) {
  const opener = `(surface ${surfaceName}`;
  const start = source.indexOf(opener);
  if (start < 0) {
    diagnostics.push({
      file,
      message: `(surface ${surfaceName} ...) form not found; cannot verify surface-local notes: ${needles.join(', ')}`,
    });
    return;
  }
  let depth = 0;
  let end = -1;
  for (let i = start; i < source.length; i += 1) {
    const ch = source[i];
    if (ch === '(') depth += 1;
    else if (ch === ')') {
      depth -= 1;
      if (depth === 0) {
        end = i + 1;
        break;
      }
    }
  }
  if (end < 0) {
    diagnostics.push({
      file,
      message: `(surface ${surfaceName} ...) form is not balanced; cannot verify surface-local notes`,
    });
    return;
  }
  const body = source.slice(start, end);
  for (const needle of needles) {
    if (!body.includes(needle)) {
      diagnostics.push({
        file,
        message: `(surface ${surfaceName} ...) note missing required anchor: ${needle}`,
      });
    }
  }
}

function runFixtures(json) {
  const cases = [];

  // ── Pass: every required key present in every file. ────────────────
  const goodFiles = {
    [DEFAULT_FILES.blueprint]: buildGoodBlueprint(),
    [DEFAULT_FILES.reviewGate]: buildGoodReviewGate(),
    [DEFAULT_FILES.directive]: buildGoodCallerRs(),
    [DEFAULT_FILES.plan]: buildGoodPlanFacadeRs(),
    [DEFAULT_FILES.planCompileAuthoring]: buildGoodCallerRs(),
    [DEFAULT_FILES.workflow]: buildGoodWorkflowRs(),
    [DEFAULT_FILES.mcpDirective]: buildGoodMcpRs({ neverAutoApprove: true }),
    [DEFAULT_FILES.mcpPlan]: buildGoodMcpRs({ neverAutoApprove: true }),
    [DEFAULT_FILES.mcpWorkflow]: buildGoodMcpRs({ workflow: true }),
  };
  cases.push({
    name: 'pass: blueprint surface + review_gate.rs + callers + MCP schemas all aligned',
    expectOk: true,
    files: goodFiles,
  });

  // ── Fail: blueprint missing the (surface review-gate ...) form. ────
  const missingSurface = { ...goodFiles };
  missingSurface[DEFAULT_FILES.blueprint] = goodFiles[DEFAULT_FILES.blueprint].replace(
    '(surface review-gate',
    '(surface review-GHOST',
  );
  cases.push({
    name: 'fail: blueprint missing (surface review-gate ...)',
    expectOk: false,
    expectMessage: /\(surface review-gate/,
    files: missingSurface,
  });

  // ── Fail: blueprint surface body missing the never-auto-approve anchor. ─
  const missingAnchor = { ...goodFiles };
  missingAnchor[DEFAULT_FILES.blueprint] = goodFiles[DEFAULT_FILES.blueprint].replace(
    'never auto-approve',
    'never auto-approxx',
  );
  cases.push({
    name: 'fail: blueprint review-gate surface note loses never-auto-approve anchor',
    expectOk: false,
    expectMessage: /never auto-approve/,
    files: missingAnchor,
  });

  // ── Fail: review_gate.rs missing ReviewGatePolicy enum. ────────────
  const missingPolicy = { ...goodFiles };
  missingPolicy[DEFAULT_FILES.reviewGate] = goodFiles[DEFAULT_FILES.reviewGate].replace(
    'pub(crate) enum ReviewGatePolicy',
    'pub(crate) enum ReviewGateGHOST',
  );
  cases.push({
    name: 'fail: review_gate.rs lost the ReviewGatePolicy enum',
    expectOk: false,
    expectMessage: /pub\(crate\) enum ReviewGatePolicy/,
    files: missingPolicy,
  });

  // ── Fail: directive.rs stops calling apply_compile_review_gates. ───
  const directiveBypass = { ...goodFiles };
  directiveBypass[DEFAULT_FILES.directive] = goodFiles[DEFAULT_FILES.directive].replace(
    'apply_compile_review_gates(',
    'apply_compile_review_GHOST(',
  );
  cases.push({
    name: 'fail: directive.rs bypasses apply_compile_review_gates',
    expectOk: false,
    expectMessage: /apply_compile_review_gates/,
    files: directiveBypass,
  });

  // ── Fail: MCP plan tool drops review_gate_policy property. ─────────
  const mcpDrop = { ...goodFiles };
  mcpDrop[DEFAULT_FILES.mcpPlan] = goodFiles[DEFAULT_FILES.mcpPlan].replace(
    '"review_gate_policy"',
    '"review_gate_GHOST"',
  );
  cases.push({
    name: 'fail: MCP plan tool no longer exposes review_gate_policy',
    expectOk: false,
    expectMessage: /review_gate_policy/,
    files: mcpDrop,
  });

  let failed = 0;
  for (const c of cases) {
    const root = materializeFixture(c.files);
    try {
      const diagnostics = checkFiles(root, DEFAULT_FILES);
      const ok = diagnostics.length === 0;
      if (ok !== c.expectOk) {
        failed += 1;
        console.error(`fixture FAILED: ${c.name}: expected ok=${c.expectOk}, got ok=${ok}`);
        for (const d of diagnostics) console.error(`  ${d.file}: ${d.message}`);
        continue;
      }
      if (c.expectMessage) {
        const messages = diagnostics.map((d) => d.message).join(' | ');
        if (!c.expectMessage.test(messages)) {
          failed += 1;
          console.error(
            `fixture FAILED: ${c.name}: expected diagnostic matching ${c.expectMessage}, got: ${messages || '(none)'}`,
          );
        }
      }
    } finally {
      fs.rmSync(root, { recursive: true, force: true });
    }
  }

  if (failed > 0) {
    console.error(`v3 review-gate fixtures FAILED -- ${failed}/${cases.length}`);
    process.exit(1);
  }
  if (json) {
    console.log(JSON.stringify({ ok: true, fixtures: cases.length }, null, 2));
  } else {
    console.log(`v3 review-gate fixtures OK (${cases.length} cases)`);
  }
}

function materializeFixture(filesByPath) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'missiond-v3-review-gate-iso-'));
  for (const [rel, body] of Object.entries(filesByPath)) {
    const abs = path.join(root, rel);
    fs.mkdirSync(path.dirname(abs), { recursive: true });
    fs.writeFileSync(abs, body);
  }
  return root;
}

function buildGoodBlueprint() {
  // Minimal but realistic V3 blueprint snippet that satisfies every
  // BLUEPRINT_NEEDLES entry AND the surface-body anchors. The note
  // intentionally embeds plain-prose semantics so a real human-readable
  // surface declaration would also pass.
  return `;; fixture
(missiond-blueprint
  (axioms
    (two-gate-default
      :rule "Human mode requires intent approval and plan approval before execution."))
  (implementation-map
    (surface review-gate
      :status "code-aligned"
      :implements [alignment-review-gate plan-review-gate two-gate-default]
      :code ["crates/missiond-daemon/src/handlers/knowledge/review_gate.rs"
             "crates/missiond-daemon/src/handlers/knowledge/directive.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan/compile_authoring.rs"
             "crates/missiond-daemon/src/handlers/knowledge/workflow.rs"
             "crates/missiond-mcp/src/tools/knowledge/directive.rs"
             "crates/missiond-mcp/src/tools/knowledge/plan.rs"
             "crates/missiond-mcp/src/tools/knowledge/workflow.rs"]
      :note "alignment-review-gate + plan-review-gate event emission for the file-first directive / plan / workflow surfaces. ReviewGatePolicy is a closed enum manual | emit_question | off; the dispatcher apply_compile_review_gates fans out to maybe_emit_review_question_created (legacy explicit) or auto_emit_review_question_after_artifact_write (wave-14 auto). ReviewDecision is a closed enum approved | rejected | needs_changes carried by the wave-15 review-resolution input. The gate never auto-approves, never blocks the primary DB / file-write outcome, and never waits for a human; bus failures surface review_question_warning + the deterministic review_question_id so callers can retry or resolve manually with the same id. directive / plan / workflow approve / reject paths route through maybe_emit_review_question_resolved without bypassing the gate."))
  (compression-contract
    :checks ["node scripts/check-v3-review-gate-isomorphism.mjs"]))
`;
}

function buildGoodReviewGate() {
  return `// fixture
pub(crate) enum ReviewGatePolicy { Manual, EmitQuestion, Off }
const POLICIES: &[&str] = &["ReviewGatePolicy::Manual", "ReviewGatePolicy::EmitQuestion", "ReviewGatePolicy::Off"];
pub(crate) enum ReviewDecision { Approved, Rejected, NeedsChanges }
const DECISIONS: &[&str] = &["ReviewDecision::Approved", "ReviewDecision::Rejected", "ReviewDecision::NeedsChanges"];
pub(crate) fn parse_compile_review_gate() {}
pub(crate) fn parse_review_gate_policy() {}
pub(crate) fn review_gate_policy_was_explicit() -> bool { false }
pub(crate) fn derive_review_question_id() -> String { String::new() }
pub(crate) fn derive_review_question_id_for_artifact() -> String { String::new() }
pub(crate) async fn apply_compile_review_gates() {}
pub(crate) async fn maybe_emit_review_question_created() {}
pub(crate) async fn maybe_emit_review_question_resolved() {}
pub(crate) async fn auto_emit_review_question_after_artifact_write() {}
pub(crate) const PLAN_NODE_REVIEW_DEFAULT_ACTION: &str = "plan-node";
// payload markers: review_question_warning + BUS_PUBLISH_FAILED
// "persisted artifact remains intact" / "DB action already committed"
`;
}

function buildGoodCallerRs() {
  return `// fixture
use crate::handlers::knowledge::review_gate::{
    apply_compile_review_gates, maybe_emit_review_question_resolved,
    parse_compile_review_gate, parse_review_gate_policy, review_gate_policy_was_explicit,
};

fn caller(args: &serde_json::Value) {
    let policy = parse_review_gate_policy(args);
    let policy_explicit = review_gate_policy_was_explicit(args);
    let legacy = parse_compile_review_gate(args);
    apply_compile_review_gates();
    maybe_emit_review_question_resolved();
    let _ = (policy, policy_explicit, legacy);
}
`;
}

function buildGoodPlanFacadeRs() {
  return `// fixture
use crate::handlers::knowledge::review_gate::{
    maybe_emit_review_question_resolved,
};

fn caller() {
    maybe_emit_review_question_resolved();
}
`;
}

function buildGoodWorkflowRs() {
  // workflow.rs is a caller too, but the resolution path is split across
  // resolve_review and never invokes maybe_emit_review_question_resolved at
  // the file's top level today. Keep its contract narrower.
  return `// fixture
use crate::handlers::knowledge::review_gate::{
    apply_compile_review_gates, parse_compile_review_gate, parse_review_gate_policy,
    review_gate_policy_was_explicit,
};

fn caller(args: &serde_json::Value) {
    let policy = parse_review_gate_policy(args);
    let policy_explicit = review_gate_policy_was_explicit(args);
    let legacy = parse_compile_review_gate(args);
    apply_compile_review_gates();
    let _ = (policy, policy_explicit, legacy);
}
`;
}

function buildGoodMcpRs({ neverAutoApprove = false, workflow = false } = {}) {
  // Realistic-shaped MCP tool fixture. The needle list is a flat substring
  // search, so the body just has to mention each token at least once.
  const lines = [
    '// fixture',
    '"review_gate_policy": { "enum": ["manual", "emit_question", "off"] }',
    '"review_question_id": "deterministic"',
    '"review_decision": { "enum": ["approved", "rejected", "needs_changes"] }',
    '// approved | rejected | needs_changes are the closed wave-15 set',
  ];
  if (neverAutoApprove) {
    lines.push('// description: NEVER auto-rejects, never auto-approve, caller decision wins');
  }
  if (workflow) {
    lines.push('// description: NEVER auto-rejects, caller decision always wins');
  }
  return `${lines.join('\n')}\n`;
}

main();
