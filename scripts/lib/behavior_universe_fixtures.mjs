import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';

import { validateBehaviorClosure } from './behavior_universe.mjs';

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
    name: 'broad wildcard external claim without anchor fails',
    file: dir,
    ok: !result.ok
      && hasCode(result, 'NAVIGATION_CRITICAL_WILDCARD_ONLY')
      && hasCode(result, 'NAVIGATION_ANCHOR_MISSING')
      && hasCode(result, 'NAVIGATION_EFFECT_CONTRACT_MISSING'),
    diagnostics: result.diagnostics,
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
    name: 'route wildcard without navigation anchor fails',
    file: dir,
    ok: !result.ok
      && hasCode(result, 'NAVIGATION_CRITICAL_WILDCARD_ONLY')
      && hasCode(result, 'NAVIGATION_ANCHOR_MISSING'),
    diagnostics: result.diagnostics,
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
    name: 'anchor file missing fails',
    file: dir,
    ok: !result.ok && hasCode(result, 'NAVIGATION_ANCHOR_FILE_MISSING'),
    diagnostics: result.diagnostics,
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
    name: 'anchor symbol mismatch fails',
    file: dir,
    ok: !result.ok
      && hasCode(result, 'NAVIGATION_ANCHOR_STALE')
      && hasCode(result, 'NAVIGATION_ANCHOR_MISSING'),
    diagnostics: result.diagnostics,
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
