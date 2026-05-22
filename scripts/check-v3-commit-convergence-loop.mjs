#!/usr/bin/env node

import fs from 'node:fs';
import path from 'node:path';
import { readBlueprintWithEvidenceSidecars } from './lib/v3_blueprint_contract_source.mjs';

const FILES = {
  blueprint: '.missiond/v3/missiond-blueprint.lisp',
  workflow: '.missiond/workflows/commit-lisp-convergence.lisp',
  runtime: 'crates/missiond-daemon/src/engine/commit_convergence.rs',
  engineMod: 'crates/missiond-daemon/src/engine/mod.rs',
  main: 'crates/missiond-daemon/src/main.rs',
  masterControl: 'crates/missiond-daemon/src/engine/master_control.rs',
  aggregate: 'scripts/check-v3-code-isomorphism-complete.mjs',
  gitignore: '.gitignore',
};

function main() {
  const diagnostics = [];
  const sources = {};
  for (const [key, rel] of Object.entries(FILES)) {
    try {
      sources[key] = key === 'blueprint'
        ? readBlueprintWithEvidenceSidecars(process.cwd(), rel)
        : fs.readFileSync(path.join(process.cwd(), rel), 'utf8');
    } catch (err) {
      diagnostics.push(`${rel}: cannot read: ${err.message}`);
    }
  }
  if (diagnostics.length === 0) check(sources, diagnostics);
  if (diagnostics.length > 0) {
    diagnostics.forEach((d) => console.error(d));
    console.error(`v3 commit-convergence loop check FAILED -- ${diagnostics.length} diagnostic(s)`);
    process.exit(1);
  }
  console.log('v3 commit-convergence loop check OK');
}

function check(s, diagnostics) {
  requireAll(diagnostics, FILES.workflow, s.workflow, [
    '(workflow commit-lisp-convergence',
    ':workflow_id commit-lisp-convergence',
    'SystemEvent.ContextualCommitDetected',
    'dedupe-key "commit-lisp-backfill:<project>:<sha>"',
    'git diff-tree --root --no-commit-id -r --name-only <sha>',
    'external-or-unavailable-commit',
    'covered when code changes have same-commit Lisp/checker/evidence coverage',
    'code-only commits need backfill',
    'visible BoardTask',
    '.missiond/v3/runtime/commit-lisp-convergence/<sha>.report.lisp',
    'Lisp/checker/evidence-only backfill commits must not create another backfill task',
  ]);
  requireAll(diagnostics, FILES.blueprint, s.blueprint, [
    '(commit-lisp-convergence-loop',
    '(function commit-lisp-convergence',
    ':surface commit-lisp-convergence-loop',
    'SystemEvent::ContextualCommitDetected',
    'provider conversation project/project_id metadata',
    'git diff-tree --root --no-commit-id -r --name-only <sha>',
    'external-or-unavailable-commit',
    'commit-lisp-backfill:<project>:<sha>',
    '(surface commit-lisp-convergence-loop',
    'crates/missiond-daemon/src/engine/commit_convergence.rs',
    'scripts/check-v3-commit-convergence-loop.mjs',
  ]);
  requireAll(diagnostics, FILES.runtime, s.runtime, [
    'COMMIT_CONVERGENCE_SUBSCRIPTION',
    'commit_convergence_contextual_commit_v1_live',
    'start_commit_convergence_service',
    'subscribe::<SystemEvent>',
    'SystemEvent::ContextualCommitDetected',
    'resolve_project_from_conversation',
    'get_conversation(conversation_id)',
    'conversation.project_id',
    'conversation.project',
    'StartFrom::Latest',
    'CursorFlush::PerEvent',
    'git_diff_tree_changed_files',
    '"diff-tree"',
    '"--root"',
    '"--no-commit-id"',
    '"-r"',
    '"--name-only"',
    'classify_changed_files',
    'CommitCoverageStatus::NeedsBackfill',
    'CommitCoverageStatus::LispOnly',
    'CommitCoverageStatus::ExternalOrUnavailableCommit',
    'find_open_task_by_dedupe_key',
    'CreateBoardTaskInput',
    'auto_execute: Some(false)',
    'hidden: Some(false)',
    'commit-lisp-backfill:{project_id}:{commit_hash}',
    'notify_board_event_direct(&ev)',
    'publish_board(ev)',
    'status_snapshot',
    'commit-lisp-convergence-report',
    'classifies_code_only_commit_as_needs_backfill',
    'unavailable_commit_has_precise_status_label',
  ]);
  requireAll(diagnostics, FILES.engineMod, s.engineMod, ['pub mod commit_convergence;']);
  requireAll(diagnostics, FILES.main, s.main, [
    'engine::commit_convergence::start_commit_convergence_service',
  ]);
  requireAll(diagnostics, FILES.masterControl, s.masterControl, [
    'commitConvergence',
    'crate::engine::commit_convergence::status_snapshot().await',
  ]);
  requireAll(diagnostics, FILES.aggregate, s.aggregate, [
    "'commit-lisp-convergence-loop'",
    'scripts/check-v3-commit-convergence-loop.mjs',
  ]);
  requireAll(diagnostics, FILES.gitignore, s.gitignore, [
    '.missiond/v3/runtime/commit-lisp-convergence/*.report.lisp',
  ]);
}

function requireAll(diagnostics, file, source, needles) {
  for (const needle of needles) {
    if (!source.includes(needle)) diagnostics.push(`${file}: missing required text: ${needle}`);
  }
}

main();
