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
    cases.push(caseDeclaredObservedIdMissing(root));
    cases.push(caseIgnoredTestGeneratedWrites(root));
    cases.push(caseProjectWithoutOldLispFailsUntilClaimed(root));
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
  const result = validateBehaviorClosure(dir, { projectId: 'fixture' });
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
  const result = validateBehaviorClosure(dir, { projectId: 'fixture' });
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
  const result = validateBehaviorClosure(dir, { projectId: 'fixture' });
  return assertCase({
    name: 'declared external write through guard passes',
    file: dir,
    ok: result.ok,
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
  const result = validateBehaviorClosure(dir, { projectId: 'fixture' });
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
  const result = validateBehaviorClosure(dir, { projectId: 'fixture' });
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
  (behavior :id fixture-routes :kind route :owner test :observed ["route:*"] :code ["app/api/**"] :effects []))
`);
  const claimed = validateBehaviorClosure(dir, { projectId: 'fixture' });
  return assertCase({
    name: 'project with active route fails until behavior-universe claims it',
    file: dir,
    ok: !missing.ok && hasCode(missing, 'BEHAVIOR_UNIVERSE_MISSING') && claimed.ok,
    diagnostics: [...missing.diagnostics, ...claimed.diagnostics],
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
