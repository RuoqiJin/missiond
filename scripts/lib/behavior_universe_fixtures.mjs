import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';

import { validateBehaviorClosure } from './behavior_universe.mjs';
import { generateBehaviorNavigation } from '../propose-behavior-navigation.mjs';

export function runBehaviorUniverseFixtures() {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'missiond-behavior-universe-'));
  const cases = [];
  try {
    cases.push(caseUndeclaredExternalWrite(root));
    cases.push(caseDeclaredExternalWriteWithoutGuard(root));
    cases.push(caseDeclaredExternalWriteWithGuard(root));
    cases.push(caseBroadWildcardExternalWriteWithoutAnchor(root));
    cases.push(caseDeclaredObservedIdMissing(root));
    cases.push(caseIgnoredTestGeneratedWrites(root));
    cases.push(caseProjectWithoutOldLispFailsUntilClaimed(root));
    cases.push(caseRouteWildcardWithoutAnchorFails(root));
    cases.push(caseRepoLocalDbWriteBroadClaimPasses(root));
    cases.push(caseAnchorFileMissing(root));
    cases.push(caseAnchorSymbolMismatch(root));
    cases.push(caseGeneratedNavigationDedupesSymbolRisk(root));
    cases.push(caseSemanticNavigationSurvivesLineDrift(root));
    cases.push(caseCompiledNavigationSatisfiesWildcardAnchor(root));
    cases.push(caseMissingCompiledNavigationIsProjectionDiagnostic(root));
    cases.push(caseScannerProfileDoesNotInventCoverage(root));
  } finally {
    fs.rmSync(root, { recursive: true, force: true });
  }

  const failed = cases.filter((c) => !c.ok);
  return {
    ok: failed.length === 0,
    cases,
    diagnostics: failed.map((c) => ({
      file: c.file,
      line: 1,
      column: 1,
      code: 'BEHAVIOR_UNIVERSE_FIXTURE_FAILED',
      message: c.message,
    })),
  };
}

function caseUndeclaredExternalWrite(root) {
  const dir = makeCase(root, 'undeclared-external-write');
  write(dir, 'src/main.rs', `fn main() {
    let home = dirs::home_dir().unwrap();
    let path = home.join(".claude/CLAUDE.md");
    std::fs::write(path, "stale").unwrap();
}
`);
  const result = validateBehaviorClosure(dir, { projectId: 'fixture', navigationLevel: 'risk' });
  return assertCase({
    name: 'undeclared Rust home write fails',
    file: dir,
    ok: !result.ok
      && hasCode(result, 'BEHAVIOR_UNIVERSE_MISSING')
      && hasCode(result, 'OBSERVED_EFFECT_UNDECLARED')
      && hasCode(result, 'EXTERNAL_EFFECT_GUARD_BYPASS'),
    diagnostics: result.diagnostics,
  });
}

function caseDeclaredExternalWriteWithoutGuard(root) {
  const dir = makeCase(root, 'declared-external-without-guard');
  writeDeclaredExternalUniverse(dir);
  write(dir, 'src/main.rs', `fn main() {
    let home = dirs::home_dir().unwrap();
    let path = home.join(".claude/CLAUDE.md");
    std::fs::write(path, "stale").unwrap();
}
`);
  const result = validateBehaviorClosure(dir, { projectId: 'fixture', navigationLevel: 'risk' });
  return assertCase({
    name: 'declared external write without runtime guard fails',
    file: dir,
    ok: !result.ok
      && result.diagnostics.length === 1
      && hasCode(result, 'EXTERNAL_EFFECT_GUARD_BYPASS'),
    diagnostics: result.diagnostics,
  });
}

function caseDeclaredExternalWriteWithGuard(root) {
  const dir = makeCase(root, 'declared-external-with-guard');
  writeDeclaredExternalUniverse(dir);
  write(dir, 'src/main.rs', `use crate::context::effects;

fn main() {
    let home = dirs::home_dir().unwrap();
    let path = home.join(".claude/CLAUDE.md");
    effects::write_text(ctx, &path, "fresh").unwrap();
}
`);
  const result = validateBehaviorClosure(dir, { projectId: 'fixture', navigationLevel: 'risk' });
  return assertCase({
    name: 'declared external write through guard passes',
    file: dir,
    ok: result.ok,
    diagnostics: result.diagnostics,
  });
}

function caseBroadWildcardExternalWriteWithoutAnchor(root) {
  const dir = makeCase(root, 'broad-external-without-anchor');
  write(dir, '.missiond/behavior-universe.lisp', `(behavior-universe fixture
  :schema "missiond.behavior-universe.v1"
  :project fixture
  (behavior
    :id fixture-external-home-write
    :kind effect
    :owner test
    :observed ["effect:fs-write:*"]
    :code ["src/main.rs"]
    :effects [fixture-claude-write])
  (effect
    :id fixture-claude-write
    :feature fixture-claude
    :kind filesystem-write
    :operation write
    :path-pattern "~/.claude/CLAUDE.md"
    :scope external-home
    :default enabled
    :kill-switch none
    :audit test))
`);
  write(dir, 'src/main.rs', `use crate::context::effects;

fn main() {
    let home = dirs::home_dir().unwrap();
    let path = home.join(".claude/CLAUDE.md");
    effects::write_text(ctx, &path, "fresh").unwrap();
}
`);
  const result = validateBehaviorClosure(dir, { projectId: 'fixture', navigationLevel: 'risk' });
  return assertCase({
    name: 'broad wildcard external claim without anchor is projection diagnostic',
    file: dir,
    ok: result.ok
      && !result.projection_ok
      && hasProjectionCode(result, 'NAVIGATION_CRITICAL_WILDCARD_ONLY')
      && hasProjectionCode(result, 'NAVIGATION_ANCHOR_MISSING')
      && hasProjectionCode(result, 'NAVIGATION_EFFECT_CONTRACT_MISSING'),
    diagnostics: result.projectionDiagnostics,
  });
}

function caseDeclaredObservedIdMissing(root) {
  const dir = makeCase(root, 'declared-observed-missing');
  write(dir, '.missiond/behavior-universe.lisp', `(behavior-universe fixture
  :schema "missiond.behavior-universe.v1"
  :project fixture
  (behavior :id fixture-worker :kind worker :owner test :observed ["worker:missing-worker"] :code ["src/main.rs"] :effects []))
`);
  write(dir, 'src/main.rs', 'fn main() {}\n');
  const result = validateBehaviorClosure(dir, { projectId: 'fixture', navigationLevel: 'risk' });
  return assertCase({
    name: 'declared observed ID missing from code fails',
    file: dir,
    ok: !result.ok && hasCode(result, 'DECLARED_OBSERVED_ID_MISSING'),
    diagnostics: result.diagnostics,
  });
}

function caseIgnoredTestGeneratedWrites(root) {
  const dir = makeCase(root, 'ignored-test-generated-writes');
  write(dir, '.missiond/behavior-universe.lisp', `(behavior-universe fixture
  :schema "missiond.behavior-universe.v1"
  :project fixture)
`);
  write(dir, 'tests/raw_home_write.rs', `fn test_only() {
    let home = dirs::home_dir().unwrap();
    std::fs::write(home.join(".claude/CLAUDE.md"), "test").unwrap();
}
`);
  write(dir, 'src/generated/raw_home_write.rs', `fn generated() {
    let home = dirs::home_dir().unwrap();
    std::fs::write(home.join(".claude/CLAUDE.md"), "generated").unwrap();
}
`);
  const result = validateBehaviorClosure(dir, { projectId: 'fixture', navigationLevel: 'risk' });
  return assertCase({
    name: 'test and generated writes are ignored',
    file: dir,
    ok: result.ok && result.observed.length === 0,
    diagnostics: result.diagnostics,
  });
}

function caseProjectWithoutOldLispFailsUntilClaimed(root) {
  const dir = makeCase(root, 'new-project-route');
  write(dir, 'app/api/ping/route.ts', 'export function GET() { return Response.json({ ok: true }); }\n');
  const missing = validateBehaviorClosure(dir, { projectId: 'fixture' });
  write(dir, '.missiond/behavior-universe.lisp', `(behavior-universe fixture
  :schema "missiond.behavior-universe.v1"
  :project fixture
  (behavior
    :id fixture-routes
    :kind route
    :owner test
    :observed ["route:*"]
    :code ["app/api/**"]
    :effects []
    (anchor :role route :observed "route:app/api/ping/route.ts:*" :file "app/api/ping/route.ts" :symbol "GET")))
`);
  const claimed = validateBehaviorClosure(dir, { projectId: 'fixture' });
  return assertCase({
    name: 'project with active route fails until behavior-universe claims it',
    file: dir,
    ok: !missing.ok && hasCode(missing, 'BEHAVIOR_UNIVERSE_MISSING') && claimed.ok,
    diagnostics: [...missing.diagnostics, ...claimed.diagnostics],
  });
}

function caseRouteWildcardWithoutAnchorFails(root) {
  const dir = makeCase(root, 'route-wildcard-without-anchor');
  write(dir, '.missiond/behavior-universe.lisp', `(behavior-universe fixture
  :schema "missiond.behavior-universe.v1"
  :project fixture
  (behavior :id fixture-routes :kind route :owner test :observed ["route:*"] :code ["app/api/**"] :effects []))
`);
  write(dir, 'app/api/ping/route.ts', 'export function GET() { return Response.json({ ok: true }); }\n');
  const result = validateBehaviorClosure(dir, { projectId: 'fixture', navigationLevel: 'risk' });
  return assertCase({
    name: 'route wildcard without navigation anchor is projection diagnostic',
    file: dir,
    ok: result.ok
      && !result.projection_ok
      && hasProjectionCode(result, 'NAVIGATION_CRITICAL_WILDCARD_ONLY')
      && hasProjectionCode(result, 'NAVIGATION_ANCHOR_MISSING'),
    diagnostics: result.projectionDiagnostics,
  });
}

function caseRepoLocalDbWriteBroadClaimPasses(root) {
  const dir = makeCase(root, 'repo-local-db-broad-claim');
  write(dir, '.missiond/behavior-universe.lisp', `(behavior-universe fixture
  :schema "missiond.behavior-universe.v1"
  :project fixture
  (behavior :id fixture-db :kind db-write :owner test :observed ["db-write:*"] :code ["src/main.rs"] :effects []))
`);
  write(dir, 'src/main.rs', 'fn save() { sqlx::query("INSERT INTO events VALUES ($1)"); }\n');
  const result = validateBehaviorClosure(dir, { projectId: 'fixture', navigationLevel: 'risk' });
  return assertCase({
    name: 'repo-local DB write broad claim passes navigation v1',
    file: dir,
    ok: result.ok,
    diagnostics: result.diagnostics,
  });
}

function caseAnchorFileMissing(root) {
  const dir = makeCase(root, 'anchor-file-missing');
  write(dir, '.missiond/behavior-universe.lisp', `(behavior-universe fixture
  :schema "missiond.behavior-universe.v1"
  :project fixture
  (behavior
    :id fixture-routes
    :kind route
    :owner test
    :observed ["route:*"]
    :code ["app/api/**"]
    :effects []
    (anchor :role route :observed "route:app/api/ping/route.ts:*" :file "app/api/missing/route.ts" :symbol "GET")))
`);
  write(dir, 'app/api/ping/route.ts', 'export function GET() { return Response.json({ ok: true }); }\n');
  const result = validateBehaviorClosure(dir, { projectId: 'fixture', navigationLevel: 'risk' });
  return assertCase({
    name: 'anchor file missing is projection diagnostic',
    file: dir,
    ok: result.ok && !result.projection_ok && hasProjectionCode(result, 'NAVIGATION_ANCHOR_FILE_MISSING'),
    diagnostics: result.projectionDiagnostics,
  });
}

function caseAnchorSymbolMismatch(root) {
  const dir = makeCase(root, 'anchor-symbol-mismatch');
  write(dir, '.missiond/behavior-universe.lisp', `(behavior-universe fixture
  :schema "missiond.behavior-universe.v1"
  :project fixture
  (behavior
    :id fixture-routes
    :kind route
    :owner test
    :observed ["route:*"]
    :code ["app/api/**"]
    :effects []
    (anchor :role route :observed "route:app/api/ping/route.ts:*" :file "app/api/ping/route.ts" :symbol "POST")))
`);
  write(dir, 'app/api/ping/route.ts', 'export function GET() { return Response.json({ ok: true }); }\n');
  const result = validateBehaviorClosure(dir, { projectId: 'fixture', navigationLevel: 'risk' });
  return assertCase({
    name: 'anchor symbol mismatch is projection diagnostic',
    file: dir,
    ok: result.ok
      && !result.projection_ok
      && hasProjectionCode(result, 'NAVIGATION_ANCHOR_STALE')
      && hasProjectionCode(result, 'NAVIGATION_ANCHOR_MISSING'),
    diagnostics: result.projectionDiagnostics,
  });
}

function caseGeneratedNavigationDedupesSymbolRisk(root) {
  const dir = makeCase(root, 'generated-navigation-dedupe');
  write(dir, 'tools/run.mjs', `function run() {
  spawnSync('echo', ['one']);
  spawnSync('echo', ['two']);
}
`);
  const result = generateBehaviorNavigation({
    project: 'fixture',
    root: dir,
    repo: dir,
    target: path.join(dir, '.missiond/runtime/compiled/compiled-behavior-navigation.json'),
  });
  return assertCase({
    name: 'generated navigation dedupes repeated symbol risk',
    file: dir,
    ok: result.risk_count === 2
      && result.anchor_count === 1
      && result.artifact.schema_version === 'missiond.compiled-behavior-navigation.v2'
      && result.artifact.anchors[0]?.semantic_id === 'subprocess:tools/run.mjs#run:subprocess',
    diagnostics: result.artifact.diagnostics,
  });
}

function caseSemanticNavigationSurvivesLineDrift(root) {
  const dir = makeCase(root, 'semantic-line-drift');
  writeRouteUniverse(dir);
  write(dir, 'app/api/ping/route.ts', 'export function GET() { return Response.json({ ok: true }); }\n');
  writeCompiledNavigation(dir);
  const before = validateBehaviorClosure(dir, { projectId: 'fixture', navigationLevel: 'risk' });
  write(dir, 'app/api/ping/route.ts', '// inserted line\nexport function GET() { return Response.json({ ok: true }); }\n');
  const after = validateBehaviorClosure(dir, { projectId: 'fixture', navigationLevel: 'risk' });
  return assertCase({
    name: 'semantic navigation survives route line drift',
    file: dir,
    ok: before.ok
      && before.projection_ok
      && after.ok
      && after.projection_ok,
    diagnostics: [...before.projectionDiagnostics, ...after.projectionDiagnostics],
  });
}

function caseCompiledNavigationSatisfiesWildcardAnchor(root) {
  const dir = makeCase(root, 'compiled-navigation-satisfies-wildcard');
  writeRouteUniverse(dir);
  write(dir, 'app/api/ping/route.ts', 'export function GET() { return Response.json({ ok: true }); }\n');
  const missing = validateBehaviorClosure(dir, { projectId: 'fixture', navigationLevel: 'risk' });
  writeCompiledNavigation(dir);
  const compiled = validateBehaviorClosure(dir, { projectId: 'fixture', navigationLevel: 'risk' });
  return assertCase({
    name: 'compiled navigation satisfies wildcard route anchor requirement',
    file: dir,
    ok: missing.ok
      && !missing.projection_ok
      && hasProjectionCode(missing, 'BEHAVIOR_NAVIGATION_ARTIFACT_MISSING')
      && compiled.ok
      && compiled.projection_ok,
    diagnostics: [...missing.projectionDiagnostics, ...compiled.projectionDiagnostics],
  });
}

function caseMissingCompiledNavigationIsProjectionDiagnostic(root) {
  const dir = makeCase(root, 'missing-compiled-navigation');
  writeRouteUniverse(dir);
  write(dir, 'app/api/ping/route.ts', 'export function GET() { return Response.json({ ok: true }); }\n');
  const result = validateBehaviorClosure(dir, { projectId: 'fixture', navigationLevel: 'risk' });
  return assertCase({
    name: 'missing compiled navigation is projection diagnostic',
    file: dir,
    ok: result.ok
      && !result.projection_ok
      && hasProjectionCode(result, 'BEHAVIOR_NAVIGATION_ARTIFACT_MISSING'),
    diagnostics: result.projectionDiagnostics,
  });
}

function caseScannerProfileDoesNotInventCoverage(root) {
  const dir = makeCase(root, 'scanner-profile-no-fake-coverage');
  write(dir, '.missiond/behavior-universe.lisp', `(behavior-universe fixture
  :schema "missiond.behavior-universe.v1"
  :project fixture
  (scanner-profile
    :id fixture-go
    :language go
    :mode manual-contract
    :coverage ["route"]
    :manual-contracts ["go routes must be declared manually"]
    :rule "Scanner profile records coverage policy but does not synthesize observed behavior."))
`);
  write(dir, 'cmd/server/main.go', 'package main\nfunc main() {}\n');
  const result = validateBehaviorClosure(dir, { projectId: 'fixture', navigationLevel: 'risk' });
  return assertCase({
    name: 'scanner-profile does not invent observed coverage',
    file: dir,
    ok: result.ok
      && result.observed.length === 0
      && result.declared.scannerProfiles.length === 1,
    diagnostics: [...result.diagnostics, ...result.projectionDiagnostics],
  });
}

function writeDeclaredExternalUniverse(root) {
  write(root, '.missiond/behavior-universe.lisp', `(behavior-universe fixture
  :schema "missiond.behavior-universe.v1"
  :project fixture
  (behavior
    :id fixture-external-home-write
    :kind effect
    :owner test
    :observed ["effect:fs-write:*"]
    :code ["src/main.rs"]
    :effects [fixture-claude-write]
    (anchor
      :role effect-site
      :observed "effect:fs-write:src/main.rs:*"
      :file "src/main.rs"
      :symbol "main"
      :effect fixture-claude-write))
  (effect
    :id fixture-claude-write
    :feature fixture-claude
    :kind filesystem-write
    :operation write
    :path-pattern "~/.claude/CLAUDE.md"
    :scope external-home
    :default enabled
    :kill-switch none
    :audit test))
`);
}

function writeRouteUniverse(root) {
  write(root, '.missiond/behavior-universe.lisp', `(behavior-universe fixture
  :schema "missiond.behavior-universe.v1"
  :project fixture
  (behavior :id fixture-routes :kind route :owner test :observed ["route:*"] :code ["app/api/**"] :effects []))
`);
}

function writeCompiledNavigation(root) {
  const target = path.join(root, '.missiond/runtime/compiled/compiled-behavior-navigation.json');
  const result = generateBehaviorNavigation({
    project: 'fixture',
    root,
    repo: root,
    target,
  });
  write(root, '.missiond/runtime/compiled/compiled-behavior-navigation.json', `${JSON.stringify(result.artifact, null, 2)}\n`);
  return result;
}

function makeCase(root, name) {
  const dir = path.join(root, name);
  fs.mkdirSync(dir, { recursive: true });
  return dir;
}

function write(root, rel, content) {
  const file = path.join(root, rel);
  fs.mkdirSync(path.dirname(file), { recursive: true });
  fs.writeFileSync(file, content, 'utf8');
}

function hasCode(result, code) {
  return result.diagnostics.some((d) => d.code === code);
}

function hasProjectionCode(result, code) {
  return result.projectionDiagnostics.some((d) => d.code === code);
}

function assertCase({ name, file, ok, diagnostics }) {
  return {
    name,
    ok,
    file,
    message: ok
      ? null
      : `${name}: ${diagnostics.map((d) => d.code).join(', ') || 'no diagnostics'}`,
  };
}
