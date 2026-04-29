#!/usr/bin/env node

// wave28-03 batch renderer.
//
// Consumes a wave28-01 task-runner-manifest v1 Lisp file and emits, in this
// order:
//   1. one shared preamble at .missiond/claudecode/<wave>-shared-preamble.md
//      (rendered ONCE per wave; skipped when the file is already present
//      unless --force is passed);
//   2. one thin ClaudeCode brief per productive node at
//      .missiond/claudecode/<task-id>.md (rendered with --brief-mode thin
//      pointing at the shared preamble path declared on the manifest).
//
// The renderer NEVER shells out — it imports loadSingleTask / renderTask /
// renderSharedPreamble / deriveSharedPreamblePath directly from
// render-claudecode-task.mjs (wave28-03 added named exports + gated main()).
//
// Archive / backfill / index / lisp-backfill pseudo-nodes are forbidden in
// productive_only manifests (the wave28-01 checker already rejects this);
// this script treats the same rule as defence in depth and FAILS LOUDLY if
// it ever sees one slip through.
//
// CLI:
//   node scripts/render-wave-briefs.mjs --manifest <manifest.lisp> [--force] [--dry-fixture]
//
// --dry-fixture runs a self-contained suite of pass/fail fixtures inside a
// throwaway tmp dir; it does not touch the user's checked-in
// .missiond/claudecode/wave*-*.md briefs.

import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';

import {
  readManifestFile,
  validateManifestObject,
  FORBIDDEN_PRODUCTIVE_KINDS,
} from './check-task-runner-manifest.mjs';

import {
  loadSingleTask,
  renderTask,
  renderSharedPreamble,
  deriveSharedPreamblePath,
} from './render-claudecode-task.mjs';

const usage = `Usage:
  node scripts/render-wave-briefs.mjs --manifest <manifest.lisp> [--force] [--dry-fixture]

Reads a MissionD task-runner-manifest v1 Lisp file and emits:
  - .missiond/claudecode/<wave>-shared-preamble.md (once per wave; skipped when
    the file already exists unless --force is supplied)
  - .missiond/claudecode/<task-id>.md (one thin brief per productive node)

The renderer imports renderer functions in-process from
scripts/render-claudecode-task.mjs and NEVER shells out. The shared preamble
text is identical byte-for-byte to scripts/render-claudecode-task.mjs
--brief-mode preamble (renderSharedPreamble named export).

Archive / backfill / index / lisp-backfill pseudo-nodes are rejected as
worker briefs (defence in depth on top of the wave28-01 checker).

--dry-fixture runs self-contained fixtures inside a throwaway tmp dir; the
checked-in .missiond/claudecode/wave*-*.md briefs are NEVER touched.

Cross-wave invariants:
  - Manifests are advisory orchestration metadata, NOT a runtime backend
    switch. This renderer reads them and writes Markdown only.
  - Productive worker tasks are rendered thin; shared boilerplate lives in
    the per-wave preamble. Full briefs are still produced by
    render-claudecode-task.mjs --brief-mode full directly.
`;

// Exported so scripts/prepare-task-runner-wave.mjs (wave29-03) can reuse the
// exact same defense-in-depth substring set without forking the constant.
// Pure data; mutating it is a contract break for downstream callers.
export const FORBIDDEN_ID_SUBSTRINGS = ['-archive-', '-backfill-', '-index', 'lisp-backfill'];

function main() {
  const args = process.argv.slice(2);
  let manifestPath = null;
  let force = false;
  let dryFixture = false;

  for (let i = 0; i < args.length; i++) {
    const arg = args[i];
    if (arg === '-h' || arg === '--help') {
      console.log(usage);
      process.exit(0);
    } else if (arg === '--manifest') {
      manifestPath = args[++i];
      if (!manifestPath) fail('--manifest requires a path');
    } else if (arg === '--force') {
      force = true;
    } else if (arg === '--dry-fixture') {
      dryFixture = true;
    } else {
      fail(`unknown argument: ${arg}\n\n${usage}`);
    }
  }

  if (dryFixture) {
    runFixtures();
    return;
  }

  if (!manifestPath) fail(usage);

  const cwd = process.cwd();
  const resolvedManifest = path.resolve(cwd, manifestPath);
  const result = renderManifest({
    manifestPath: resolvedManifest,
    cwd,
    force,
  });
  console.log(
    `render-wave-briefs OK (wave ${result.wave}, manifest ${path.relative(cwd, resolvedManifest)}): ` +
      `preamble ${result.preamble.action} -> ${path.relative(cwd, result.preamble.path)}; ` +
      `${result.briefs.length} thin brief(s) written`,
  );
}

// Render every productive node in `manifestPath` against `cwd` and return a
// summary object so callers (CLI + dry-fixture) can assert post-conditions.
//
// Behavior:
//   - Reads + validates the manifest via wave28-01's readManifestFile +
//     validateManifestObject. Any structural error fails fast.
//   - Defence-in-depth: any productive node whose :kind matches
//     FORBIDDEN_PRODUCTIVE_KINDS or whose :task_id contains any forbidden
//     substring is rejected with a clear error.
//   - The shared preamble is rendered ONCE per wave at the manifest's
//     :shared_preamble_path. When the file already exists and `force` is
//     false, the preamble step is skipped (action="skipped"); existing
//     bytes are preserved untouched.
//   - Each productive worker brief is written to
//     .missiond/claudecode/<task-id>.md (deterministic; same manifest =>
//     same paths). When the brief already exists and `force` is false, the
//     existing file is left untouched.
//
// wave29-03: exported as a named export so scripts/prepare-task-runner-wave.mjs
// can reuse the exact same renderer in-process (NEVER shells out). The CLI
// entry point above continues to call this function locally; behavior and
// return shape are unchanged from wave28-03 — this is purely a surface
// addition so the wave29 prep CLI can layer report-skeleton + bootstrap
// shared-memory/session-trace generation on top of the same renderer.
export function renderManifest({ manifestPath, cwd, force }) {
  if (!fs.existsSync(manifestPath)) {
    fail(`manifest file does not exist: ${manifestPath}`);
  }

  const manifests = readManifestFile(manifestPath);
  if (manifests.length === 0) {
    fail(`no (task-runner-manifest ...) form found in ${manifestPath}`);
  }
  if (manifests.length > 1) {
    fail(
      `${manifestPath} contains ${manifests.length} (task-runner-manifest ...) forms; expected exactly 1`,
    );
  }
  const manifest = manifests[0];

  const errors = validateManifestObject(manifest);
  if (errors.length > 0) {
    const joined = errors.map((e) => `  - ${e}`).join('\n');
    fail(`manifest ${manifestPath} failed schema validation:\n${joined}`);
  }

  // Defence in depth: even though wave28-01's checker already rejects
  // archive / backfill / index / lisp-backfill nodes inside productive_only
  // manifests, treat any slip-through as a hard error here. The batch
  // renderer must never silently emit a worker brief for an
  // orchestrator-owned pseudo-node.
  for (const node of manifest.nodes) {
    if (node.kind && FORBIDDEN_PRODUCTIVE_KINDS.has(node.kind)) {
      fail(
        `node "${node.task_id}" has :kind "${node.kind}" — archive/backfill/index/lisp-backfill nodes MUST NOT be rendered as worker briefs`,
      );
    }
    for (const sub of FORBIDDEN_ID_SUBSTRINGS) {
      if (node.task_id && node.task_id.includes(sub)) {
        fail(
          `node "${node.task_id}" id contains "${sub}" — archive/backfill/index/lisp-backfill nodes MUST NOT be rendered as worker briefs`,
        );
      }
    }
  }

  // Shared preamble path. The manifest declares it (wave28-01 schema), and
  // the wave-derived auto-default lives at
  // .missiond/claudecode/<wave>-shared-preamble.md. We honor the manifest
  // value verbatim and resolve it relative to cwd.
  const sharedPreambleRel = manifest.shared_preamble_path;
  const sharedPreambleAbs = path.resolve(cwd, sharedPreambleRel);
  const preambleResult = writeSharedPreamble({
    outputAbs: sharedPreambleAbs,
    force,
  });

  // Thin briefs. We pass the same shared-preamble path the manifest
  // declared so the rendered brief points at the file the orchestrator
  // expects. Each brief lands at .missiond/claudecode/<task-id>.md.
  const briefDir = path.resolve(cwd, '.missiond', 'claudecode');
  const briefs = [];
  for (const node of manifest.nodes) {
    const taskContractPath = resolveTaskContractPath(cwd, manifest.wave, node.task_id);
    if (!taskContractPath) {
      fail(
        `node "${node.task_id}" task contract not found under .missiond/tasks/${manifest.wave}/${node.task_id}.lisp`,
      );
    }
    const task = loadSingleTask(taskContractPath);
    const markdown = renderTask(task, taskContractPath, {
      briefMode: 'thin',
      sharedPreamble: sharedPreambleRel,
    }) + renderSoftReferences(node);
    const briefAbs = path.join(briefDir, `${node.task_id}.md`);
    const action = writeIfAllowed(briefAbs, markdown, force);
    briefs.push({ task_id: node.task_id, path: briefAbs, action });
  }

  return {
    wave: manifest.wave,
    preamble: { path: sharedPreambleAbs, action: preambleResult.action },
    briefs,
  };
}

function renderSoftReferences(node) {
  const refs = Array.isArray(node.soft_refs) ? node.soft_refs : [];
  if (refs.length === 0) return '';
  const lines = refs
    .slice()
    .sort((a, b) => a.localeCompare(b))
    .map((id) => `- ${id}`)
    .join('\n');
  return `\n## Soft References\n\n${lines}\n\nSoft references are context only; they are not dispatch dependencies or blockers.\n`;
}

// Locate the task contract Lisp file for `taskId` inside `wave`. Return
// null when the file is not on disk so the caller can fail with a clear
// error pointing at the expected path. Convention is fixed:
//   .missiond/tasks/<wave>/<task-id>.lisp
function resolveTaskContractPath(cwd, wave, taskId) {
  const rel = path.join('.missiond', 'tasks', wave, `${taskId}.lisp`);
  const abs = path.resolve(cwd, rel);
  return fs.existsSync(abs) ? abs : null;
}

// Render the shared preamble Markdown ONCE. Returns { action } where action
// is "written" | "skipped" | "overwritten". Skipped means the file was
// already on disk and force was false — the existing bytes are preserved.
function writeSharedPreamble({ outputAbs, force }) {
  fs.mkdirSync(path.dirname(outputAbs), { recursive: true });
  if (fs.existsSync(outputAbs) && !force) {
    return { action: 'skipped' };
  }
  const markdown = renderSharedPreamble();
  const overwriting = fs.existsSync(outputAbs);
  fs.writeFileSync(outputAbs, markdown);
  return { action: overwriting ? 'overwritten' : 'written' };
}

// Write `markdown` to `outputAbs`, but only when the file is missing or
// force=true. Returns the action label so callers can report what happened.
function writeIfAllowed(outputAbs, markdown, force) {
  fs.mkdirSync(path.dirname(outputAbs), { recursive: true });
  if (fs.existsSync(outputAbs) && !force) {
    return 'skipped';
  }
  const overwriting = fs.existsSync(outputAbs);
  fs.writeFileSync(outputAbs, markdown);
  return overwriting ? 'overwritten' : 'written';
}

function fail(message) {
  console.error(message);
  process.exit(2);
}

// ---------------------------------------------------------------------------
// Dry fixtures.
//
// Every fixture runs inside a fresh tmp dir to keep the checked-in
// .missiond/claudecode/wave*-*.md briefs untouched. We assemble a complete
// repo-shaped tree per fixture: the task contract Lisp files plus the
// manifest Lisp file. The renderer writes the preamble + briefs into the
// tmp dir, then we assert the post-conditions.
//
// The fixture covers:
//   - pass: preamble file exists after run (written from scratch)
//   - pass: thin brief OMITS Shared Memory / Report Contract / Session
//     Trace / Router Policy boilerplate sections (they live in the shared
//     preamble) AND points at the shared preamble path
//   - pass: deterministic output paths (same manifest -> same files)
//   - pass: skip-when-present (without --force, the existing preamble is
//     preserved byte-identically)
//   - pass: --force overwrites the preamble
//   - fail: archive node id rejected as defence-in-depth
//   - fail: backfill node id rejected as defence-in-depth
//   - fail: index :kind rejected as defence-in-depth
//   - pass: full-mode dry-fixtures of render-claudecode-task.mjs continue
//     to pass (smoke check that our exports + main() gating did not regress
//     the upstream renderer)
// ---------------------------------------------------------------------------

async function runFixtures() {
  const fixtures = [];

  fixtures.push({
    name: 'preamble-and-thin-briefs-from-scratch',
    category: 'pass-render-from-scratch',
    run: async () => {
      const env = setupTmpRepo();
      try {
        seedTwoNodeManifest(env);
        const result = renderManifest({
          manifestPath: env.manifestPath,
          cwd: env.cwd,
          force: false,
        });
        const preamblePath = path.resolve(env.cwd, '.missiond/claudecode/wave99-shared-preamble.md');
        if (!fs.existsSync(preamblePath)) {
          throw new Error('preamble file should exist after first render');
        }
        if (result.preamble.action !== 'written') {
          throw new Error(`preamble action expected 'written', got '${result.preamble.action}'`);
        }
        const preambleBody = fs.readFileSync(preamblePath, 'utf8');
        if (!preambleBody.includes('# MissionD ClaudeCode Shared Preamble')) {
          throw new Error('preamble missing standard header');
        }
        if (result.briefs.length !== 2) {
          throw new Error(`expected 2 thin briefs, got ${result.briefs.length}`);
        }
        for (const b of result.briefs) {
          if (b.action !== 'written') {
            throw new Error(`brief ${b.task_id} action expected 'written', got '${b.action}'`);
          }
          if (!fs.existsSync(b.path)) {
            throw new Error(`brief file missing for ${b.task_id}`);
          }
        }
      } finally {
        cleanupTmpRepo(env);
      }
    },
  });

  fixtures.push({
    name: 'thin-brief-omits-repeated-boilerplate-and-points-at-preamble',
    category: 'pass-thin-brief-shape',
    run: async () => {
      const env = setupTmpRepo();
      try {
        seedTwoNodeManifest(env);
        renderManifest({ manifestPath: env.manifestPath, cwd: env.cwd, force: false });
        const briefPath = path.resolve(env.cwd, '.missiond/claudecode/wave99-01-foo.md');
        const brief = fs.readFileSync(briefPath, 'utf8');
        // The thin brief MUST point at the manifest-declared preamble path
        // and MUST NOT repeat the heavy shared-protocol boilerplate that
        // now lives in the preamble. These are the wave28-03 invariants:
        // any regression that re-introduces the repeated sections breaks
        // the dispatch-efficiency goal of the wave.
        const requiredSubstrings = [
          'Thin brief rendered',
          '.missiond/claudecode/wave99-shared-preamble.md',
          '## Shared Protocol',
          'verification_tier',
          'heartbeat_minutes',
        ];
        for (const literal of requiredSubstrings) {
          if (!brief.includes(literal)) {
            throw new Error(`thin brief missing required literal '${literal}'`);
          }
        }
        const forbiddenSections = [
          '## Shared Memory',
          '## Report Contract',
          '## Session Trace',
          '## Router Policy',
        ];
        for (const section of forbiddenSections) {
          if (brief.includes(section)) {
            throw new Error(`thin brief should NOT contain repeated boilerplate section '${section}'`);
          }
        }
      } finally {
        cleanupTmpRepo(env);
      }
    },
  });

  fixtures.push({
    name: 'wave30-04-thin-brief-renders-soft-references-as-context',
    category: 'wave30-04-hard-soft',
    run: async () => {
      const env = setupTmpRepo();
      try {
        const ids = ['wave99-01-atlas', 'wave99-02-patterns', 'wave99-03-runner-prep'];
        for (const id of ids) {
          fs.writeFileSync(path.join(env.tasksDir, `${id}.lisp`), taskContract(id));
        }
        const manifest = `(task-runner-manifest m-wave30-04-hard-soft
  :schema "missiond.task-runner-manifest.v2"
  :wave wave99
  :brief_mode thin
  :shared_preamble_path ".missiond/claudecode/wave99-shared-preamble.md"
  :productive_only true
  (node :task_id wave99-01-atlas
        :depends_on []
        :verification_tier local
        :dispatch_group A
        :estimated_minutes 10
        :heartbeat_minutes 5
        :write_scope ["scripts/wave99-01-atlas.mjs"])
  (node :task_id wave99-02-patterns
        :depends_on []
        :verification_tier local
        :dispatch_group A
        :estimated_minutes 60
        :heartbeat_minutes 10
        :write_scope ["scripts/wave99-02-patterns.mjs"])
  (node :task_id wave99-03-runner-prep
        :depends_on [wave99-01-atlas]
        :hard_deps [wave99-01-atlas]
        :soft_refs [wave99-02-patterns]
        :verification_tier local
        :dispatch_group B
        :estimated_minutes 10
        :heartbeat_minutes 5
        :write_scope ["scripts/wave99-03-runner-prep.mjs"]))\n`;
        fs.writeFileSync(env.manifestPath, manifest);
        renderManifest({ manifestPath: env.manifestPath, cwd: env.cwd, force: false });
        const brief = fs.readFileSync(
          path.resolve(env.cwd, '.missiond/claudecode/wave99-03-runner-prep.md'),
          'utf8',
        );
        for (const literal of [
          '## Soft References',
          'wave99-02-patterns',
          'context only',
          'not dispatch dependencies or blockers',
        ]) {
          if (!brief.includes(literal)) {
            throw new Error(`soft-reference brief missing literal '${literal}'`);
          }
        }
      } finally {
        cleanupTmpRepo(env);
      }
    },
  });

  fixtures.push({
    name: 'deterministic-output-paths-same-manifest-same-files',
    category: 'pass-deterministic-paths',
    run: async () => {
      const env = setupTmpRepo();
      try {
        seedTwoNodeManifest(env);
        const r1 = renderManifest({ manifestPath: env.manifestPath, cwd: env.cwd, force: true });
        const paths1 = [r1.preamble.path, ...r1.briefs.map((b) => b.path)].sort();
        // Capture file bytes so we can prove a second render with --force
        // is byte-identical (deterministic output).
        const bytes1 = paths1.map((p) => fs.readFileSync(p, 'utf8'));
        const r2 = renderManifest({ manifestPath: env.manifestPath, cwd: env.cwd, force: true });
        const paths2 = [r2.preamble.path, ...r2.briefs.map((b) => b.path)].sort();
        if (JSON.stringify(paths1) !== JSON.stringify(paths2)) {
          throw new Error('output paths drifted between consecutive renders');
        }
        const bytes2 = paths2.map((p) => fs.readFileSync(p, 'utf8'));
        for (let i = 0; i < bytes1.length; i++) {
          if (bytes1[i] !== bytes2[i]) {
            throw new Error(`bytes drift at ${paths1[i]} between consecutive --force renders`);
          }
        }
      } finally {
        cleanupTmpRepo(env);
      }
    },
  });

  fixtures.push({
    name: 'skip-when-present-preserves-existing-preamble',
    category: 'pass-skip-when-present',
    run: async () => {
      const env = setupTmpRepo();
      try {
        seedTwoNodeManifest(env);
        const preamblePath = path.resolve(env.cwd, '.missiond/claudecode/wave99-shared-preamble.md');
        // Pre-seed an existing preamble with a sentinel byte so we can
        // detect any accidental overwrite.
        fs.mkdirSync(path.dirname(preamblePath), { recursive: true });
        const sentinel = '# pre-existing user-shipped preamble — do not overwrite\n';
        fs.writeFileSync(preamblePath, sentinel);
        const result = renderManifest({
          manifestPath: env.manifestPath,
          cwd: env.cwd,
          force: false,
        });
        if (result.preamble.action !== 'skipped') {
          throw new Error(`expected preamble action 'skipped', got '${result.preamble.action}'`);
        }
        const after = fs.readFileSync(preamblePath, 'utf8');
        if (after !== sentinel) {
          throw new Error('skip-when-present should preserve existing preamble bytes');
        }
      } finally {
        cleanupTmpRepo(env);
      }
    },
  });

  fixtures.push({
    name: 'force-overwrites-existing-preamble',
    category: 'pass-force-overwrites',
    run: async () => {
      const env = setupTmpRepo();
      try {
        seedTwoNodeManifest(env);
        const preamblePath = path.resolve(env.cwd, '.missiond/claudecode/wave99-shared-preamble.md');
        fs.mkdirSync(path.dirname(preamblePath), { recursive: true });
        fs.writeFileSync(preamblePath, '# stale\n');
        const result = renderManifest({
          manifestPath: env.manifestPath,
          cwd: env.cwd,
          force: true,
        });
        if (result.preamble.action !== 'overwritten') {
          throw new Error(`expected preamble action 'overwritten', got '${result.preamble.action}'`);
        }
        const after = fs.readFileSync(preamblePath, 'utf8');
        if (!after.includes('# MissionD ClaudeCode Shared Preamble')) {
          throw new Error('force overwrite should write the rendered preamble');
        }
      } finally {
        cleanupTmpRepo(env);
      }
    },
  });

  fixtures.push({
    name: 'archive-node-id-rejected-as-defence-in-depth',
    category: 'fail-archive-id',
    run: async () => {
      const env = setupTmpRepo();
      try {
        // Hand-craft a manifest that smuggles an archive-shaped task id
        // past wave28-01's checker by NOT setting :productive_only true at
        // the manifest level. Our renderer still rejects it because every
        // node landing as a worker brief MUST be productive.
        seedManifestWithCustomNodes(env, [
          taskContract('wave99-00-archive-foo'),
        ]);
        let threw = false;
        try {
          renderManifest({ manifestPath: env.manifestPath, cwd: env.cwd, force: false });
        } catch (err) {
          threw = true;
          if (!String(err.message).includes('-archive-')) {
            throw new Error(`expected error to mention '-archive-', got: ${err.message}`);
          }
        }
        if (!threw) throw new Error('archive-id node should have been rejected');
      } finally {
        cleanupTmpRepo(env);
      }
    },
  });

  fixtures.push({
    name: 'backfill-node-id-rejected-as-defence-in-depth',
    category: 'fail-backfill-id',
    run: async () => {
      const env = setupTmpRepo();
      try {
        seedManifestWithCustomNodes(env, [
          taskContract('wave99-09-lisp-backfill-status'),
        ]);
        let threw = false;
        try {
          renderManifest({ manifestPath: env.manifestPath, cwd: env.cwd, force: false });
        } catch (err) {
          threw = true;
          if (!String(err.message).match(/lisp-backfill|-backfill-/)) {
            throw new Error(`expected error to mention backfill substring, got: ${err.message}`);
          }
        }
        if (!threw) throw new Error('backfill-id node should have been rejected');
      } finally {
        cleanupTmpRepo(env);
      }
    },
  });

  fixtures.push({
    name: 'index-kind-rejected-as-defence-in-depth',
    category: 'fail-index-kind',
    run: async () => {
      const env = setupTmpRepo();
      try {
        // Use a task id that does NOT match a forbidden substring so the
        // rejection is purely on :kind index. This proves the kind path
        // fires independently of the substring path.
        seedManifestWithCustomNodes(
          env,
          [taskContract('wave99-10-parallel-dispatcher')],
          { kind: 'index' },
        );
        let threw = false;
        try {
          renderManifest({ manifestPath: env.manifestPath, cwd: env.cwd, force: false });
        } catch (err) {
          threw = true;
          if (!String(err.message).includes('index')) {
            throw new Error(`expected error to mention 'index', got: ${err.message}`);
          }
        }
        if (!threw) throw new Error('index :kind node should have been rejected');
      } finally {
        cleanupTmpRepo(env);
      }
    },
  });

  // ---------------------------------------------------------------------
  // wave28-06 cross-layer smoke pins — pin the heartbeat metadata
  // propagation invariant (invariant 4) and the productive-only renderer
  // contribution (invariant 2) at the brief-renderer boundary so the
  // smoke covers each of the 5 layers explicitly.
  // ---------------------------------------------------------------------
  fixtures.push({
    name: 'wave28-06-loop-smoke-renderer-heartbeat-surfaced',
    category: 'wave28-06-loop-smoke',
    run: async () => {
      const env = setupTmpRepo();
      try {
        // Use the existing 2-node seed (heartbeat_minutes=10) which mirrors
        // the wave28-06 synthetic manifest. The thin brief MUST surface the
        // heartbeat-minutes guidance so a worker reading only the brief
        // (without re-loading the contract) still sees the cadence.
        seedTwoNodeManifest(env);
        renderManifest({ manifestPath: env.manifestPath, cwd: env.cwd, force: false });
        const briefPath = path.resolve(env.cwd, '.missiond/claudecode/wave99-01-foo.md');
        const brief = fs.readFileSync(briefPath, 'utf8');
        // Invariant 4: heartbeat metadata MUST appear in the thin brief OR
        // the shared preamble. The shared preamble we write also carries
        // the boilerplate guidance — assert at least one carries the
        // literal so a regression that strips both fails loudly.
        const preamblePath = path.resolve(env.cwd, '.missiond/claudecode/wave99-shared-preamble.md');
        const preamble = fs.readFileSync(preamblePath, 'utf8');
        const carriesHeartbeatBrief = brief.includes('heartbeat');
        const carriesHeartbeatPreamble = preamble.includes('heartbeat');
        if (!carriesHeartbeatBrief && !carriesHeartbeatPreamble) {
          throw new Error(
            'wave28-06 invariant 4: heartbeat metadata MUST surface in the thin brief OR the shared preamble',
          );
        }
        // Specifically: the thin brief MUST echo the per-task heartbeat
        // minutes so a worker reading only the brief sees the cadence
        // tied to its task.
        if (!brief.includes('heartbeat_minutes')) {
          throw new Error('thin brief MUST include `heartbeat_minutes` field');
        }
        if (!brief.includes('`10`')) {
          throw new Error(
            'thin brief MUST echo the manifest heartbeat_minutes value (10)',
          );
        }
      } finally {
        cleanupTmpRepo(env);
      }
    },
  });

  fixtures.push({
    name: 'wave28-06-loop-smoke-renderer-pseudo-skipped',
    category: 'wave28-06-loop-smoke',
    run: async () => {
      const env = setupTmpRepo();
      try {
        // Productive-only manifest with productive nodes only. The
        // renderer MUST emit one brief per node (no archive/backfill node
        // sneaks in). Pins invariant 2 at the renderer boundary —
        // mirroring the wave28-05 batch verifier skip pass.
        seedTwoNodeManifest(env);
        const result = renderManifest({
          manifestPath: env.manifestPath,
          cwd: env.cwd,
          force: false,
        });
        if (result.briefs.length !== 2) {
          throw new Error(
            `expected 2 productive briefs, got ${result.briefs.length}`,
          );
        }
        // Defence-in-depth: the rendered briefs MUST NOT carry an archive
        // / backfill / index id substring. (The seed manifest uses
        // wave99-01-foo / wave99-02-bar — the assertion catches a future
        // regression where seedTwoNodeManifest is mistakenly given a
        // pseudo-id.)
        for (const b of result.briefs) {
          for (const sub of FORBIDDEN_ID_SUBSTRINGS) {
            if (b.task_id.includes(sub)) {
              throw new Error(
                `wave28-06 invariant 2: rendered brief id "${b.task_id}" contains forbidden substring "${sub}"`,
              );
            }
          }
        }
      } finally {
        cleanupTmpRepo(env);
      }
    },
  });

  // wave29-07 cross-layer smoke (layer G): pin the navigation-context
  // anchors. When a task contract declares `:context-atlas-path` AND
  // `:pattern-card-path`, and `:context-pack-path`, the thin brief MUST mention all paths so a
  // worker reading only the brief still has stable navigation anchors.
  // This is the layer-G pin: a regression that strips the navigation
  // section from the thin brief surfaces here, near the renderer
  // boundary, BEFORE workers are dispatched.
  fixtures.push({
    name: 'wave29-07-loop-smoke-thin-brief-includes-context-anchors',
    category: 'wave29-07-loop-smoke',
    run: async () => {
      const env = setupTmpRepo();
      try {
        const taskId = 'wave99-01-foo';
        const atlasPath = '.missiond/tasks/wave99/context-atlas.lisp';
        const cardPath = '.missiond/tasks/wave99/pattern-cards.lisp';
        const packPath = '.missiond/tasks/wave99/context-pack.lisp';
        // Hand-craft a task contract with both navigation fields so the
        // renderer's renderNavigationContext path fires.
        const contract = `(task ${taskId}\n  :schema "missiond.task-contract.v1"\n  :title "Synthetic ${taskId}"\n  :kind code-alignment\n  :status ready\n  :owner "claudecode"\n  :dispatch-strategy "fresh-code-alignment"\n  :verification-tier local\n  :dispatch-group "A"\n  :estimated-minutes 25\n  :heartbeat-minutes 10\n  :context-atlas-path "${atlasPath}"\n  :pattern-card-path "${cardPath}"\n  :context-pack-path "${packPath}"\n  :goal "Synthetic ${taskId} goal."\n  :write-scope ["scripts/${taskId}.mjs"]\n  :must-not-touch []\n  :requirements ["Run renderer."]\n  :acceptance ["true"]\n  :commit (:required true :message "test: ${taskId}" :scope-check write-scope-only)\n  :report ["Commit hash."])\n`;
        fs.writeFileSync(path.join(env.tasksDir, `${taskId}.lisp`), contract);
        const manifest = `(task-runner-manifest m-wave99-anchors\n  :schema "missiond.task-runner-manifest.v1"\n  :wave wave99\n  :brief_mode thin\n  :shared_preamble_path ".missiond/claudecode/wave99-shared-preamble.md"\n  :productive_only true\n  (node :task_id ${taskId}\n        :depends_on []\n        :verification_tier local\n        :dispatch_group A\n        :estimated_minutes 25\n        :heartbeat_minutes 10\n        :write_scope ["scripts/${taskId}.mjs"]))\n`;
        fs.writeFileSync(env.manifestPath, manifest);
        const result = renderManifest({
          manifestPath: env.manifestPath,
          cwd: env.cwd,
          force: false,
        });
        if (result.briefs.length !== 1) {
          throw new Error(`expected 1 thin brief, got ${result.briefs.length}`);
        }
        const briefPath = path.resolve(env.cwd, `.missiond/claudecode/${taskId}.md`);
        const brief = fs.readFileSync(briefPath, 'utf8');
        if (!brief.includes(atlasPath)) {
          throw new Error(
            `wave29-07 layer G: thin brief MUST mention context_atlas_path "${atlasPath}"`,
          );
        }
        if (!brief.includes(cardPath)) {
          throw new Error(
            `wave29-07 layer G: thin brief MUST mention pattern_card_path "${cardPath}"`,
          );
        }
        if (!brief.includes(packPath)) {
          throw new Error(
            `wave29-07 layer G: thin brief MUST mention context_pack_path "${packPath}"`,
          );
        }
        if (!brief.includes('accepted shards')) {
          throw new Error('wave29-07 layer G: thin brief MUST tell workers to treat context-pack accepted shards as authority');
        }
        if (!brief.includes('## Context Navigation')) {
          throw new Error(
            'wave29-07 layer G: thin brief MUST carry the ## Context Navigation section header when both paths are present',
          );
        }
      } finally {
        cleanupTmpRepo(env);
      }
    },
  });

  fixtures.push({
    name: 'render-claudecode-task-full-fixtures-still-green',
    category: 'pass-backward-compat',
    run: async () => {
      // wave28-03 must NOT regress the upstream renderer's dry-fixtures.
      // We drive the same fixture entry point in-process by importing the
      // renderer module fresh and exercising its renderTask + the wave28
      // thin-brief shape via the shared preamble pointer. This fixture
      // mirrors the wave28-thin-brief case from render-claudecode-task.mjs
      // so a regression in either side surfaces here.
      const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), 'wave28-03-bc-'));
      const tmpTask = path.join(tmpDir, 'wave28-bc-fixture.lisp');
      const lisp = `(task wave28-bc-fixture\n  :schema "missiond.task-contract.v1"\n  :title "BC fixture"\n  :kind code-alignment\n  :status ready\n  :owner "claudecode"\n  :dispatch-strategy "fresh-code-alignment"\n  :verification-tier local\n  :dispatch-group "A"\n  :estimated-minutes 25\n  :heartbeat-minutes 10\n  :goal "Backward compat goal"\n  :write-scope ["scripts/x.mjs"]\n  :must-not-touch []\n  :requirements ["Use thin mode"]\n  :acceptance ["true"]\n  :commit (:required true :message "test: bc" :scope-check write-scope-only)\n  :report ["Commit hash."])\n`;
      try {
        fs.writeFileSync(tmpTask, lisp, 'utf8');
        const task = loadSingleTask(tmpTask);
        const thin = renderTask(task, tmpTask, {
          briefMode: 'thin',
          sharedPreamble: '.missiond/claudecode/wave28-shared-preamble.md',
        });
        if (!thin.includes('Thin brief rendered')) {
          throw new Error('exported renderTask thin mode regressed');
        }
        const full = renderTask(task, tmpTask);
        // Full mode MUST keep the rich boilerplate sections that thin
        // strips. Asserting both shapes here proves the wave28-03 named
        // exports preserve the wave24-05 / wave25-04 / wave26-05 / wave27-05
        // surfaces that downstream tooling depends on (the on-disk
        // dry-fixture suite of render-claudecode-task.mjs already verifies
        // the literal-by-literal asserts; this is the in-process smoke).
        const fullSections = ['Generated from MissionD task-contract v1.', '## Goal', '## Commit'];
        for (const literal of fullSections) {
          if (!full.includes(literal)) {
            throw new Error(`exported renderTask full mode missing '${literal}'`);
          }
        }
        // The exported renderSharedPreamble must still emit the canonical
        // header so that scripts/render-wave-briefs.mjs and
        // scripts/render-claudecode-task.mjs --brief-mode preamble produce
        // the SAME bytes (single source of truth).
        const preamble = renderSharedPreamble();
        for (const literal of [
          '# MissionD ClaudeCode Shared Preamble',
          '## Shared Memory',
          '## Report Contract',
          '## Commit Protocol',
        ]) {
          if (!preamble.includes(literal)) {
            throw new Error(`exported renderSharedPreamble missing '${literal}'`);
          }
        }
        // Sanity: deriveSharedPreamblePath returns the wave-prefixed path
        // used by every wave dispatch.
        const derived = deriveSharedPreamblePath('wave28-03-wave-brief-batch-renderer-v0');
        const expected = path.join('.missiond', 'claudecode', 'wave28-shared-preamble.md');
        if (derived !== expected) {
          throw new Error(`deriveSharedPreamblePath drift: got '${derived}', expected '${expected}'`);
        }
      } finally {
        fs.rmSync(tmpDir, { recursive: true, force: true });
      }
    },
  });

  // Drive every fixture; tally categories + failures. Each fixture is
  // self-contained so a single failure does not cascade.
  let failed = 0;
  const categories = new Set();
  for (const fixture of fixtures) {
    categories.add(fixture.category);
    try {
      // Allow async fixture bodies (Promise) — current bodies are sync
      // but the await is harmless and future-proofs the runner.
      // eslint-disable-next-line no-await-in-loop
      await fixture.run();
    } catch (err) {
      failed += 1;
      console.error(`fixture failed: ${fixture.name}`);
      console.error(`  ${err.message}`);
    }
  }
  if (failed > 0) {
    console.error(`render-wave-briefs fixtures FAILED — ${failed} of ${fixtures.length}`);
    process.exit(1);
  }
  console.log(
    `render-wave-briefs fixtures OK (${fixtures.length} cases, ${categories.size} categories)`,
  );
}

// Build a fresh tmp repo skeleton with the directories the renderer
// expects: .missiond/tasks/<wave>/ + .missiond/claudecode/. Returns the
// cwd + paths the fixture body uses; cleanup is the caller's job
// (cleanupTmpRepo) so a panic mid-fixture does not leak tmp files.
function setupTmpRepo() {
  const cwd = fs.mkdtempSync(path.join(os.tmpdir(), 'wave28-03-render-'));
  const tasksDir = path.join(cwd, '.missiond', 'tasks', 'wave99');
  const briefDir = path.join(cwd, '.missiond', 'claudecode');
  fs.mkdirSync(tasksDir, { recursive: true });
  fs.mkdirSync(briefDir, { recursive: true });
  const manifestPath = path.join(cwd, '.missiond', 'tasks', 'wave99', 'manifest.lisp');
  return { cwd, tasksDir, briefDir, manifestPath };
}

function cleanupTmpRepo(env) {
  fs.rmSync(env.cwd, { recursive: true, force: true });
}

// Build a synthetic task-contract Lisp file body for `taskId`. The opts
// argument lets fixtures vary :kind so we can exercise the index-kind
// rejection path; everything else is held constant so we cleanly isolate
// the dispatch_group / brief_mode behaviors we are asserting.
function taskContract(taskId, opts = {}) {
  const kindLine = opts.taskKind ? `  :kind ${opts.taskKind}` : '  :kind code-alignment';
  return `(task ${taskId}\n  :schema "missiond.task-contract.v1"\n  :title "Synthetic ${taskId}"\n${kindLine}\n  :status ready\n  :owner "claudecode"\n  :dispatch-strategy "fresh-code-alignment"\n  :verification-tier local\n  :dispatch-group "A"\n  :estimated-minutes 25\n  :heartbeat-minutes 10\n  :goal "Synthetic ${taskId} goal."\n  :write-scope ["scripts/${taskId}.mjs"]\n  :must-not-touch []\n  :requirements ["Run renderer."]\n  :acceptance ["true"]\n  :commit (:required true :message "test: ${taskId}" :scope-check write-scope-only)\n  :report ["Commit hash."])\n`;
}

// Seed a 2-node productive manifest so the happy-path fixtures cover the
// minimum interesting topology (single dispatch_group, two distinct
// productive workers). Both nodes get their task-contract Lisp files
// written to tmp tasks dir alongside the manifest.
function seedTwoNodeManifest(env) {
  const fooContract = taskContract('wave99-01-foo');
  const barContract = taskContract('wave99-02-bar');
  fs.writeFileSync(path.join(env.tasksDir, 'wave99-01-foo.lisp'), fooContract);
  fs.writeFileSync(path.join(env.tasksDir, 'wave99-02-bar.lisp'), barContract);
  const manifest = `(task-runner-manifest m-wave99\n  :schema "missiond.task-runner-manifest.v1"\n  :wave wave99\n  :brief_mode thin\n  :shared_preamble_path ".missiond/claudecode/wave99-shared-preamble.md"\n  :productive_only true\n  (node :task_id wave99-01-foo\n        :depends_on []\n        :verification_tier local\n        :dispatch_group A\n        :estimated_minutes 25\n        :heartbeat_minutes 10\n        :write_scope ["scripts/wave99-01-foo.mjs"])\n  (node :task_id wave99-02-bar\n        :depends_on [wave99-01-foo]\n        :verification_tier local\n        :dispatch_group B\n        :estimated_minutes 25\n        :heartbeat_minutes 10\n        :write_scope ["scripts/wave99-02-bar.mjs"]))\n`;
  fs.writeFileSync(env.manifestPath, manifest);
}

// Seed a manifest with caller-supplied node task contracts. Used by the
// defence-in-depth fail fixtures to smuggle archive / backfill / index
// shapes past the upstream checker — the manifest is :productive_only false
// so wave28-01's checker accepts it; our renderer is the second guard that
// must still reject these node shapes for worker briefs.
function seedManifestWithCustomNodes(env, contracts, opts = {}) {
  const ids = [];
  for (const contract of contracts) {
    const idMatch = contract.match(/^\(task\s+([a-z0-9][a-z0-9._-]*)/);
    if (!idMatch) throw new Error('synthetic contract missing task id');
    const id = idMatch[1];
    ids.push(id);
    fs.writeFileSync(path.join(env.tasksDir, `${id}.lisp`), contract);
  }
  const kindAttr = opts.kind ? `\n        :kind ${opts.kind}` : '';
  const nodes = ids
    .map(
      (id) =>
        `  (node :task_id ${id}\n        :depends_on []\n        :verification_tier local\n        :dispatch_group A\n        :estimated_minutes 25\n        :heartbeat_minutes 10\n        :write_scope ["scripts/${id}.mjs"]${kindAttr})`,
    )
    .join('\n');
  const manifest = `(task-runner-manifest m-wave99-custom\n  :schema "missiond.task-runner-manifest.v1"\n  :wave wave99\n  :brief_mode thin\n  :shared_preamble_path ".missiond/claudecode/wave99-shared-preamble.md"\n  :productive_only false\n${nodes})\n`;
  fs.writeFileSync(env.manifestPath, manifest);
}

// fail() above terminates the process; for fixtures we need to throw so
// the per-fixture try/catch can record the failure without bringing down
// the whole suite. Override fail() inside renderManifest? No — instead we
// monkey-patch process.exit during fixtures to convert exit(2) into a
// throw. This keeps the production path's fail-fast behavior intact while
// letting fixtures assert on rejection messages.
const originalExit = process.exit.bind(process);
const originalConsoleError = console.error.bind(console);
function patchProcessExitForFixtures() {
  process.exit = function patchedExit(code) {
    if (code === 2) {
      const err = new Error(_lastFailMessage || 'process.exit(2)');
      throw err;
    }
    return originalExit(code);
  };
  let _last = null;
  Object.defineProperty(globalThis, '_lastFailMessage', {
    get() { return _last; },
    set(v) { _last = v; },
  });
  console.error = function patchedErr(msg) {
    _last = typeof msg === 'string' ? msg : String(msg);
    return originalConsoleError(msg);
  };
}
function unpatchProcessExitForFixtures() {
  process.exit = originalExit;
  console.error = originalConsoleError;
}

// Wrap runFixtures so the patch applies only inside the fixture loop.
const _runFixturesUnpatched = runFixtures;
async function runFixturesPatched() {
  patchProcessExitForFixtures();
  try {
    await _runFixturesUnpatched();
  } finally {
    unpatchProcessExitForFixtures();
  }
}

// Async main wrapper: only invoked when this file is executed directly,
// not when imported. Mirrors the gating pattern used in
// scripts/render-claudecode-task.mjs (wave28-03) and
// scripts/check-task-runner-manifest.mjs (wave28-01).
async function mainAsync() {
  const args = process.argv.slice(2);
  if (args.includes('--dry-fixture')) {
    await runFixturesPatched();
    return;
  }
  main();
}

if (import.meta.url === `file://${process.argv[1]}`) {
  mainAsync().catch((err) => {
    console.error(err.stack || err.message);
    process.exit(1);
  });
}
