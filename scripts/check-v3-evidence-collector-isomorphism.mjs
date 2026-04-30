#!/usr/bin/env node

import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';

const usage = `Usage:
  node scripts/check-v3-evidence-collector-isomorphism.mjs [--json] [--dry-fixture]

Checks the V3 evidence-collector Lisp/code isomorphism contract:
  - V3 blueprint declares (surface evidence-collector ...) with :status
    "code-aligned", :code paths covering evidence_collector.rs and the plan
    runner that writes verification evidence, and a :note that names the
    EVIDENCE_SCHEMA_VERSION = "v0", the closed EventRefStatus / EventRefProvenance
    enums with their wire-string projections, the EventRefResolver passive cache
    cap (1024 entries) + log-query miss-reason constants, and the wrap_legacy
    path that lifts caller JSON evidence into typed EvidenceEntry shape.
  - compression-contract :checks pins this checker.
  - evidence_collector.rs exposes the stable public surface: the schema
    version constant, EventRefStatus {Live | Log | Unavailable},
    EventRefProvenance {Live | PassiveCache | EventLogQuery | Unavailable},
    EventRef + EvidenceEntry structs, AppendOutcome, the async append entry
    point, append_entry_to_project_root, wrap_legacy_record_evidence,
    EVENT_REF_CACHE_CAP = 1024, the log-query miss / error reason constants,
    and the EventRefResolver struct that owns resolver tier composition.
  - plan/evidence_sidecar.rs and plan/execution_runtime.rs route evidence writes
    through the sibling evidence_collector module instead of hand-rolling
    sidecar JSON.
`;

const DEFAULT_FILES = {
  blueprint: '.missiond/v3/missiond-blueprint.lisp',
  evidenceCollector: 'crates/missiond-daemon/src/handlers/knowledge/evidence_collector.rs',
  evidenceCollectorTests: 'crates/missiond-daemon/src/handlers/knowledge/evidence_collector/tests.rs',
  planEvidenceSidecar: 'crates/missiond-daemon/src/handlers/knowledge/plan/evidence_sidecar.rs',
  planExecutionRuntime: 'crates/missiond-daemon/src/handlers/knowledge/plan/execution_runtime.rs',
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
    console.log('v3 evidence-collector Lisp/code isomorphism check OK');
  } else {
    for (const d of diagnostics) {
      console.error(`${d.file}: ${d.message}`);
    }
    console.error(
      `v3 evidence-collector Lisp/code isomorphism check FAILED -- ${diagnostics.length} diagnostic(s)`,
    );
  }

  process.exit(result.ok ? 0 : 1);
}

const BLUEPRINT_NEEDLES = [
  '(surface evidence-collector',
  ':status "code-aligned"',
  'crates/missiond-daemon/src/handlers/knowledge/evidence_collector.rs',
  'crates/missiond-daemon/src/handlers/knowledge/evidence_collector/tests.rs',
  'verification-receipt',
  'EventRefStatus',
  'EventRefProvenance',
  'EVIDENCE_SCHEMA_VERSION',
  'live',
  'log',
  'unavailable',
  'passive_cache',
  'event_log_query',
  'EVENT_REF_CACHE_CAP',
  '1024',
  'wrap_legacy_record_evidence',
  'node scripts/check-v3-evidence-collector-isomorphism.mjs',
];

const EVIDENCE_COLLECTOR_RS_NEEDLES = [
  'pub(crate) const EVIDENCE_SCHEMA_VERSION: &str = "v0"',
  'pub(crate) enum EventRefStatus',
  'EventRefStatus::Live',
  'EventRefStatus::Log',
  'EventRefStatus::Unavailable',
  'pub(crate) enum EventRefProvenance',
  'EventRefProvenance::Live',
  'EventRefProvenance::PassiveCache',
  'EventRefProvenance::EventLogQuery',
  'EventRefProvenance::Unavailable',
  'pub(crate) struct EventRef',
  'pub(crate) struct EvidenceEntry',
  'pub(crate) enum AppendOutcome',
  'pub(crate) async fn append',
  'pub(crate) fn append_entry_to_project_root',
  'pub(crate) fn wrap_legacy_record_evidence',
  'pub(crate) const EVENT_REF_CACHE_CAP: usize = 1024',
  'pub(crate) const EVENT_REF_RESOLVER_MISS_REASON',
  'pub(crate) const EVENT_REF_LOG_QUERY_MISS_REASON',
  'pub(crate) const EVENT_REF_LOG_QUERY_ERROR_REASON_PREFIX',
  'pub(crate) const EVENT_REF_LOG_QUERY_SCAN_LIMIT',
  'pub(crate) struct EventRefResolver',
  '"live"',
  '"log"',
  '"unavailable"',
  '"passive_cache"',
  '"event_log_query"',
  'mod tests;',
];

const EVIDENCE_COLLECTOR_TESTS_NEEDLES = [
  'entry_always_carries_canonical_stamps',
  'resolver_log_query_returns_log_ref_after_cache_miss',
  'wrap_legacy_keeps_inner_evidence_under_evidence_key',
  'sidecar_append_preserves_order_and_schema_version',
  'distill_chain_records_are_strictly_additive_per_plan',
];

const PLAN_EXECUTION_RUNTIME_RS_NEEDLES = [
  'super::super::evidence_collector::EvidenceEntry::new',
  'super::super::evidence_collector::append(',
];

const PLAN_EVIDENCE_SIDECAR_RS_NEEDLES = [
  'evidence_collector::wrap_legacy_record_evidence',
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
  requireSurfaceNoteContains(
    diagnostics,
    files.blueprint,
    sources.blueprint,
    'evidence-collector',
    ['EventRefStatus', 'EventRefProvenance', 'verification-receipt'],
  );

  requireAll(
    diagnostics,
    files.evidenceCollector,
    sources.evidenceCollector,
    EVIDENCE_COLLECTOR_RS_NEEDLES,
  );
  requireAll(
    diagnostics,
    files.evidenceCollectorTests,
    sources.evidenceCollectorTests,
    EVIDENCE_COLLECTOR_TESTS_NEEDLES,
  );
  requireAll(
    diagnostics,
    files.planExecutionRuntime,
    sources.planExecutionRuntime,
    PLAN_EXECUTION_RUNTIME_RS_NEEDLES,
  );
  requireAll(
    diagnostics,
    files.planEvidenceSidecar,
    sources.planEvidenceSidecar,
    PLAN_EVIDENCE_SIDECAR_RS_NEEDLES,
  );

  return diagnostics;
}

function requireAll(diagnostics, file, source, needles) {
  for (const needle of needles) {
    if (!source.includes(needle)) {
      diagnostics.push({ file, message: `missing required contract text: ${needle}` });
    }
  }
}

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

  const goodFiles = {
    [DEFAULT_FILES.blueprint]: buildGoodBlueprint(),
    [DEFAULT_FILES.evidenceCollector]: buildGoodEvidenceCollector(),
    [DEFAULT_FILES.evidenceCollectorTests]: buildGoodEvidenceCollectorTests(),
    [DEFAULT_FILES.planExecutionRuntime]: buildGoodPlanExecutionRuntime(),
    [DEFAULT_FILES.planEvidenceSidecar]: buildGoodPlanEvidenceSidecar(),
  };
  cases.push({
    name: 'pass: blueprint surface + evidence_collector.rs API + plan.rs caller all aligned',
    expectOk: true,
    files: goodFiles,
  });

  const missingSurface = { ...goodFiles };
  missingSurface[DEFAULT_FILES.blueprint] = goodFiles[DEFAULT_FILES.blueprint].replace(
    '(surface evidence-collector',
    '(surface evidence-GHOST',
  );
  cases.push({
    name: 'fail: blueprint missing (surface evidence-collector ...)',
    expectOk: false,
    expectMessage: /\(surface evidence-collector/,
    files: missingSurface,
  });

  const missingAnchor = { ...goodFiles };
  missingAnchor[DEFAULT_FILES.blueprint] = goodFiles[DEFAULT_FILES.blueprint].replace(
    'EventRefProvenance',
    'EventRefGHOST',
  );
  cases.push({
    name: 'fail: blueprint evidence-collector surface note loses EventRefProvenance anchor',
    expectOk: false,
    expectMessage: /EventRefProvenance/,
    files: missingAnchor,
  });

  const missingApi = { ...goodFiles };
  missingApi[DEFAULT_FILES.evidenceCollector] = goodFiles[DEFAULT_FILES.evidenceCollector].replace(
    'pub(crate) const EVIDENCE_SCHEMA_VERSION: &str = "v0"',
    'pub(crate) const EVIDENCE_SCHEMA_GHOST: &str = "v0"',
  );
  cases.push({
    name: 'fail: evidence_collector.rs lost EVIDENCE_SCHEMA_VERSION constant',
    expectOk: false,
    expectMessage: /EVIDENCE_SCHEMA_VERSION/,
    files: missingApi,
  });

  const wrongCacheCap = { ...goodFiles };
  wrongCacheCap[DEFAULT_FILES.evidenceCollector] = goodFiles[DEFAULT_FILES.evidenceCollector]
    .replace('pub(crate) const EVENT_REF_CACHE_CAP: usize = 1024', 'pub(crate) const EVENT_REF_CACHE_CAP: usize = 999');
  cases.push({
    name: 'fail: passive cache cap drifted from 1024',
    expectOk: false,
    expectMessage: /EVENT_REF_CACHE_CAP/,
    files: wrongCacheCap,
  });

  const planBypass = { ...goodFiles };
  planBypass[DEFAULT_FILES.planExecutionRuntime] = goodFiles[
    DEFAULT_FILES.planExecutionRuntime
  ].replace(
    'super::super::evidence_collector::EvidenceEntry::new',
    'hand_rolled_json::EvidenceEntry::new',
  );
  cases.push({
    name: 'fail: plan.rs stops routing writes through evidence_collector',
    expectOk: false,
    expectMessage: /evidence_collector/,
    files: planBypass,
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
    console.error(`v3 evidence-collector fixtures FAILED -- ${failed}/${cases.length}`);
    process.exit(1);
  }
  if (json) {
    console.log(JSON.stringify({ ok: true, fixtures: cases.length }, null, 2));
  } else {
    console.log(`v3 evidence-collector fixtures OK (${cases.length} cases)`);
  }
}

function materializeFixture(filesByPath) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'missiond-v3-evidence-collector-iso-'));
  for (const [rel, body] of Object.entries(filesByPath)) {
    const abs = path.join(root, rel);
    fs.mkdirSync(path.dirname(abs), { recursive: true });
    fs.writeFileSync(abs, body);
  }
  return root;
}

function buildGoodBlueprint() {
  return `;; fixture
(missiond-blueprint
  (artifact-contracts
    (artifact verification-receipt
      :path ".missiond/requests/<request_id>/receipts/<receipt_id>.lisp"))
  (implementation-map
    (surface evidence-collector
      :status "code-aligned"
      :implements [verification-receipt]
      :code ["crates/missiond-daemon/src/handlers/knowledge/evidence_collector.rs"
             "crates/missiond-daemon/src/handlers/knowledge/evidence_collector/tests.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan/evidence_sidecar.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan/execution_runtime.rs"]
      :note "EVIDENCE_SCHEMA_VERSION = \\"v0\\" pins the wire shape; EventRefStatus is the closed enum live | log | unavailable describing whether the ref is live-from-publish, log-recovered post hoc, or simply unavailable. EventRefProvenance further pivots the recovery tier as live | passive_cache | event_log_query | unavailable so consumers can attribute lookups to the wave-16 in-memory passive cache (EVENT_REF_CACHE_CAP = 1024 FIFO entries) vs the wave-18 bounded event_log_query path. wrap_legacy_record_evidence lifts caller-supplied JSON evidence into the typed EvidenceEntry envelope without losing prior fields, so the verification-receipt artifact stays consistent with what plan.rs already wrote."))
  (compression-contract
    :checks ["node scripts/check-v3-evidence-collector-isomorphism.mjs"]))
`;
}

function buildGoodEvidenceCollector() {
  return `// fixture
pub(crate) const EVIDENCE_SCHEMA_VERSION: &str = "v0";

pub(crate) enum EventRefStatus { Live, Log, Unavailable }
fn status_wire(s: EventRefStatus) -> &'static str {
    match s {
        EventRefStatus::Live => "live",
        EventRefStatus::Log => "log",
        EventRefStatus::Unavailable => "unavailable",
    }
}

pub(crate) enum EventRefProvenance {
    Live,
    PassiveCache,
    EventLogQuery,
    Unavailable,
}
fn provenance_wire(p: EventRefProvenance) -> &'static str {
    match p {
        EventRefProvenance::Live => "live",
        EventRefProvenance::PassiveCache => "passive_cache",
        EventRefProvenance::EventLogQuery => "event_log_query",
        EventRefProvenance::Unavailable => "unavailable",
    }
}

pub(crate) struct EventRef {}
pub(crate) struct EvidenceEntry {}
pub(crate) enum AppendOutcome { Written, NoOp }

pub(crate) async fn append() {}
pub(crate) fn append_entry_to_project_root() {}
pub(crate) fn wrap_legacy_record_evidence() {}

pub(crate) const EVENT_REF_CACHE_CAP: usize = 1024;
pub(crate) const EVENT_REF_RESOLVER_MISS_REASON: &str = "passive_cache_miss";
pub(crate) const EVENT_REF_LOG_QUERY_MISS_REASON: &str = "log_query_miss";
pub(crate) const EVENT_REF_LOG_QUERY_ERROR_REASON_PREFIX: &str = "log_query_error:";
pub(crate) const EVENT_REF_LOG_QUERY_SCAN_LIMIT: usize = 512;

pub(crate) struct EventRefResolver {}
mod tests;
`;
}

function buildGoodEvidenceCollectorTests() {
  return `// fixture
fn entry_always_carries_canonical_stamps() {}
fn resolver_log_query_returns_log_ref_after_cache_miss() {}
fn wrap_legacy_keeps_inner_evidence_under_evidence_key() {}
fn sidecar_append_preserves_order_and_schema_version() {}
fn distill_chain_records_are_strictly_additive_per_plan() {}
`;
}

function buildGoodPlanExecutionRuntime() {
  return `// fixture
fn dispatch_caller() {
    let entry = super::super::evidence_collector::EvidenceEntry::new(
        super::super::evidence_collector::source::PLAN_RUNNER_DISPATCH,
        super::super::evidence_collector::kind::DISPATCH,
    );
    let _ = super::super::evidence_collector::append(entry);
}
`;
}

function buildGoodPlanEvidenceSidecar() {
  return `// fixture
fn manual_caller() {
    let _ = super::evidence_collector::wrap_legacy_record_evidence();
}
`;
}

main();
