#!/usr/bin/env node

import fs from 'node:fs';
import path from 'node:path';

const FILES = {
  blueprint: '.missiond/v3/missiond-blueprint.lisp',
  workflow: '.missiond/workflows/lisp-code-sync.lisp',
  runtime: 'crates/missiond-daemon/src/engine/lisp_code_sync.rs',
  autopilot: 'crates/missiond-daemon/src/engine/intent_engine/autopilot.rs',
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
      sources[key] = fs.readFileSync(path.join(process.cwd(), rel), 'utf8');
    } catch (err) {
      diagnostics.push(`${rel}: cannot read: ${err.message}`);
    }
  }
  if (diagnostics.length === 0) check(sources, diagnostics);
  if (diagnostics.length > 0) {
    diagnostics.forEach((d) => console.error(d));
    console.error(`v3 lisp-code-sync check FAILED -- ${diagnostics.length} diagnostic(s)`);
    process.exit(1);
  }
  console.log('v3 lisp-code-sync isomorphism check OK');
}

function check(s, diagnostics) {
  requireAll(diagnostics, FILES.workflow, s.workflow, [
    '(workflow lisp-code-sync',
    ':workflow_id lisp-code-sync',
    'SystemEvent.ConfigChanged',
    'lisp-code-sync:<project>:<path-hash>',
    'lisp-code-sync:<project>:storm-circuit',
    'compile-v3-runtime.mjs --json',
    'code-isomorphism gate',
    'visible BoardTask',
    'exact accepted shard',
    'ignore runtime report paths',
    'suppress unchanged content fingerprints',
    'subscription consumer',
    'debounce repeated path events',
    'retention/GC',
    'reportDirs/stormCircuitHits/recentSyncTaskCreations',
    'stale_evidence/resolved_by_runtime_fix',
    'stale-boardtask-dispatch-revalidation',
    'stale runtime report tasks must not consume worker slots',
    '.missiond/v3/runtime/lisp-code-sync/<timestamp>-<path-hash>.report.lisp',
  ]);
  requireAll(diagnostics, FILES.blueprint, s.blueprint, [
    '(lisp-code-sync-loop',
    '(function lisp-code-sync',
    ':surface lisp-code-sync-loop',
    'SystemEvent::ConfigChanged',
    'MISSIOND_LISP_CODE_SYNC_WATCH',
    'runtime report paths are ignored',
    'unchanged content fingerprints are suppressed',
    'queued ConfigChanged events',
    'debounce repeated path events',
    'retention/GC',
    'check-v3-code-isomorphism-complete',
    'lisp-code-sync:<project>:<path-hash>',
    'lisp-code-sync:<project>:storm-circuit',
    'same-source-storm-circuit-breaker',
    'decision-inbox-revalidation',
    'runtime-load-explanation',
    'stale_evidence/resolved_by_runtime_fix',
    'resolved_by_runtime_fix/stale_evidence',
    'Autopilot revalidates lisp-code-sync runtime report BoardTasks before slot selection',
    '(surface lisp-code-sync-loop',
    '(surface lisp-code-sync-storm-circuit',
    '(surface decision-inbox-revalidation',
    '(surface runtime-load-explanation',
    'crates/missiond-daemon/src/engine/lisp_code_sync.rs',
    'scripts/check-v3-lisp-code-sync-isomorphism.mjs',
  ]);
  requireAll(diagnostics, FILES.runtime, s.runtime, [
    'LISP_CODE_SYNC_SUBSCRIPTION',
    'lisp_code_sync_config_changed_v1_live',
    'start_lisp_code_sync_service',
    'notify::RecommendedWatcher::new',
    'publish_system(SystemEvent::ConfigChanged',
    'lisp_sync_content_fingerprint',
    'last_content_fingerprints',
    'last_processed_content_fingerprints',
    'should_process_content_fingerprint',
    'subscribe::<SystemEvent>',
    'SystemEvent::ConfigChanged',
    'is_lisp_sync_path',
    'is_ignored_lisp_sync_runtime_path',
    'LISP_CODE_SYNC_DEBOUNCE_WINDOW',
    'LISP_CODE_SYNC_MAX_REPORTS',
    'LISP_CODE_SYNC_STORM_PRECREATE_THRESHOLD',
    'prune_report_dir',
    'collect_report_dir_status_for_state',
    'storm_dedupe_key_for_lisp_sync',
    'run_project_sync_check',
    'compile-v3-runtime.mjs',
    'check-v3-code-isomorphism-complete.mjs',
    'find_open_task_by_dedupe_key',
    'CreateBoardTaskInput',
    'auto_execute: Some(true)',
    'hidden: Some(false)',
    'lisp-code-sync:{project_id}:',
    'storm-circuit',
    'notify_board_event_direct(&ev)',
    'publish_board(ev)',
    'lisp-code-sync-report',
    'stormCircuitHits',
    'reportDirs',
    'status_snapshot',
    'path_filter_accepts_missiond_lisp_and_checker_files',
  ]);
  requireAll(diagnostics, FILES.autopilot, s.autopilot, [
    'is_stale_lisp_code_sync_runtime_report_task',
    'resolve_stale_lisp_code_sync_runtime_report_task',
    'resolved_by_runtime_fix / stale_evidence',
    '.missiond/v3/runtime/lisp-code-sync/**',
    'before dispatch',
    'runtime lisp-code-sync report tasks must be resolved before slot dispatch',
  ]);
  requireAll(diagnostics, FILES.engineMod, s.engineMod, ['pub mod lisp_code_sync;']);
  requireAll(diagnostics, FILES.main, s.main, [
    'engine::lisp_code_sync::start_lisp_code_sync_service',
  ]);
  requireAll(diagnostics, FILES.masterControl, s.masterControl, [
    'lispCodeSync',
    'crate::engine::lisp_code_sync::status_snapshot_for_state(state).await',
    'runtimeLoadExplanation',
    'runtime_load_explanation',
  ]);
  requireAll(diagnostics, FILES.aggregate, s.aggregate, [
    "'lisp-code-sync-loop'",
    'scripts/check-v3-lisp-code-sync-isomorphism.mjs',
  ]);
  requireAll(diagnostics, FILES.gitignore, s.gitignore, [
    '.missiond/v3/runtime/lisp-code-sync/*.report.lisp',
  ]);
}

function requireAll(diagnostics, file, source, needles) {
  for (const needle of needles) {
    if (!source.includes(needle)) diagnostics.push(`${file}: missing required text: ${needle}`);
  }
}

main();
