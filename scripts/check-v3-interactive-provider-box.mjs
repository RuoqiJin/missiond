#!/usr/bin/env node

import fs from 'node:fs';
import path from 'node:path';
import { readBlueprintWithEvidenceSidecars } from './lib/v3_blueprint_contract_source.mjs';

const usage = `Usage:
  node scripts/check-v3-interactive-provider-box.mjs [--json] [--repo <path>]

Checks the MissionD interactive provider box contract:
  - V3 declares provider CLI access as PTY + durable-log final extraction.
  - V3 declares worker-turn, model-switch, usage-probe, model-catalog-export,
    and pure-text-single-turn as box modes rather than caller-owned PTY scripts.
  - Remaining headless/text-only provider CLI implementations are inventoried
    as migration targets.
  - Normal worker PTY command construction stays interactive.
`;

const FILES = {
  blueprint: '.missiond/v3/missiond-blueprint.lisp',
  workstationRuntime: '.missiond/v3/shards/workstation-runtime.lisp',
  requestRuntime: '.missiond/v3/shards/request-runtime.lisp',
  requestSurfaces: '.missiond/v3/shards/implementation/request-surfaces.lisp',
  universeService: '.missiond/v3/shards/universe/service-runtime.lisp',
  workstationEvidence: '.missiond/v3/evidence/workstation-pool.lisp',
  session: 'crates/missiond-pty/src/session.rs',
  genericCli: 'crates/missiond-daemon/src/slot_orchestrator/generic_cli.rs',
  geminiPty: 'crates/missiond-daemon/src/llm/gemini_pty.rs',
  agyParser: 'crates/missiond-core/src/agy_cli/parser.rs',
  agyWatcher: 'crates/missiond-core/src/agy_cli/watcher.rs',
  daemonMain: 'crates/missiond-daemon/src/main.rs',
  ptyRecognition: 'crates/missiond-pty/src/pty_recognition.rs',
  providerBoxMod: 'crates/missiond-daemon/src/provider_box/mod.rs',
  providerBoxTypes: 'crates/missiond-daemon/src/provider_box/types.rs',
  providerBoxDriver: 'crates/missiond-daemon/src/provider_box/driver.rs',
  providerBoxRuntime: 'crates/missiond-daemon/src/provider_box/runtime.rs',
  providerBoxArtifact: 'crates/missiond-daemon/src/provider_box/artifact.rs',
  providerBoxCodexDriver: 'crates/missiond-daemon/src/provider_box/codex_driver.rs',
  providerBoxClaudeCodeDriver: 'crates/missiond-daemon/src/provider_box/claude_code_driver.rs',
  providerBoxHttpAdapter: 'crates/missiond-daemon/src/provider_box/http_adapter.rs',
  jarvisServer: 'crates/missiond-core/src/ws/server.rs',
  runner: 'crates/missiond-runner/src/runner.rs',
  runnerLib: 'crates/missiond-runner/src/lib.rs',
  codexCli: 'crates/missiond-daemon/src/llm/codex_cli.rs',
  codexVision: 'crates/missiond-daemon/src/workers/codex/vision_worker.rs',
  geminiCli: 'crates/missiond-daemon/src/llm/gemini_cli.rs',
  deployDaemon: 'scripts/deploy-daemon.sh',
  aggregate: 'scripts/check-v3-code-isomorphism-complete.mjs',
};

const INVENTORIED_LEGACY = [
  {
    id: 'missiond-runner-claude-print',
    files: [FILES.runner, FILES.runnerLib],
    needles: ['--print', 'claude --print --output-format stream-json'],
  },
  {
    id: 'jarvis-author-text-provider-env',
    files: [FILES.jarvisServer],
    needles: [
      'jarvis_author_text_provider',
      'MISSIOND_JARVIS_AUTHOR_TEXT_ONLY_PROVIDER',
      'MISSIOND_JARVIS_ALLOW_NON_CODEX_AUTHORS',
      'semantic-authoring',
      'pure-text-single-turn',
    ],
  },
  {
    id: 'provider-box-codex-exec-text-only',
    files: [FILES.providerBoxDriver.replace('driver.rs', 'codex_driver.rs')],
    needles: [
      'codex exec --json --output-last-message',
      'no_tools/no_mcp/no_shell/no_file_access',
      'DIAG_PROVIDER_TEXT_ONLY_VIOLATION',
    ],
  },
  {
    id: 'jarvis-grounded-direct-answer-text-only',
    files: [FILES.jarvisServer, FILES.deployDaemon],
    needles: [
      'MISSIOND_XJPCODE_TEXT_ONLY_URL',
      'MISSIOND_JARVIS_DIRECT_ANSWER_PROVIDER',
      '/provider/v1/text-only/completions',
    ],
  },
  {
    id: 'codex-vision-exec',
    files: [FILES.codexCli, FILES.codexVision],
    needles: [
      'codex exec --json',
      'codex exec --json -i',
      '--ephemeral',
    ],
  },
  {
    id: 'gemini-router-subprocess',
    files: [FILES.geminiCli],
    needles: [
      'gemini -o stream-json -p',
      'stdin(std::process::Stdio::null())',
    ],
  },
  {
    id: 'search-center-missiond-text-only',
    files: [FILES.universeService],
    needles: [
      'missiond-xjpcode-text-only',
      'SEARCH_CENTER_MISSIOND_TEXT_ONLY_URL',
      ':migration-target provider-interaction-box',
    ],
  },
];

const SUSPICIOUS_RUST_PATTERNS = [
  { id: 'claude-print', re: /"--print"\.to_string\(\)|\.arg\("--print"\)|claude --print --output-format/ },
  { id: 'codex-exec', re: /cmd\.arg\("exec"\)|--output-last-message|codex exec --json/ },
  { id: 'xjpcode-text-only', re: /MISSIOND_XJPCODE_TEXT_ONLY|MISSIOND_JARVIS_(DIRECT_ANSWER|AUTHOR)_TEXT_ONLY_PROVIDER/ },
  { id: 'gemini-prompt-json', re: /cmd\.args\(\["-p", prompt, "-m", model, "-o", "stream-json", "--yolo"\]\)|gemini -o stream-json -p/ },
];

function parseArgs(argv) {
  const opts = { json: false, repo: process.cwd() };
  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    if (arg === '--json') opts.json = true;
    else if (arg === '--repo') opts.repo = path.resolve(argv[++i] ?? '');
    else if (arg.startsWith('--repo=')) opts.repo = path.resolve(arg.slice('--repo='.length));
    else if (arg === '--help' || arg === '-h') {
      console.log(usage);
      process.exit(0);
    } else {
      console.error(`unknown arg: ${arg}`);
      console.error(usage);
      process.exit(2);
    }
  }
  return opts;
}

function main() {
  const opts = parseArgs(process.argv.slice(2));
  const repo = path.resolve(opts.repo);
  const diagnostics = [];
  const sources = {};

  for (const [key, rel] of Object.entries(FILES)) {
    try {
      sources[key] = key === 'blueprint'
        ? readBlueprintWithEvidenceSidecars(repo, rel)
        : fs.readFileSync(path.join(repo, rel), 'utf8');
    } catch (err) {
      diagnostics.push(diag(rel, `cannot read required file: ${err.message}`));
      sources[key] = '';
    }
  }

  if (diagnostics.length === 0) {
    requireAll(diagnostics, FILES.workstationRuntime, sources.workstationRuntime, [
      '(interactive-provider-box',
      'missiond.interactive-provider-box.v1',
      'missiond.provider-interaction-turn.v1',
      ':forbidden-headless-provider-cli',
      'claude-print',
      'codex-exec',
      'gemini-prompt-stream-json',
      'stdin-closed-provider-subprocess',
      ':public-commands',
      'POST /provider-box/v1/turns',
      'worker-turn',
      'model-switch',
      'usage-probe',
      'model-catalog-export',
      'pure-text-single-turn',
      'timeout_cancel_policy',
      ':driver-capabilities',
      'stepwise-operator',
      'provider-prompt-authorization',
      'provider_authorization_allowlist',
      'MISSIOND_PROVIDER_BOX_CLAUDE_CODE_MCP_AUTH_ALLOWLIST',
      'DIAG_PROVIDER_CONTROL_ACTION_UNVERIFIED',
      'screen-state-recognition',
      'screen_identity',
      'screen_usage',
      'cli_version',
      'current model',
      'Model Quota',
      'model_quotas',
      'Generating...',
      'Working...',
      'Loading...',
      'esc to cancel',
      '`>` prompt alone is not idle',
      'observe -> act -> observe -> verify -> record',
      'switch-model',
      'usage-probe',
      'model-catalog',
      'interactive /model picker',
      'AGY 1.0.5 help confirms --model',
      'request.model maps to agy --model',
      'MissionDAgy',
      'self-built proxy deployment program',
      'pure-text-guard',
      'timeout-cancel-retry',
      'PROVIDER_TURN_STALLED',
      'PROVIDER_TURN_TIMEOUT_CANCEL_FAILED',
      'Interrupted · What should Antigravity CLI do instead?',
      '? for shortcuts',
      ':legacy-headless-migration-targets',
      'missiond-runner-claude-print',
      'codex-vision-exec',
      'gemini-router-subprocess',
      'search-center-missiond-text-only',
      'transcript_full.jsonl/history.jsonl',
      'Codex rollout JSONL',
      'durable provider logs',
      'PTY screen text is diagnostic only',
      'step_records',
      ':current-implementation-gaps',
      'Agy export comes from interactive /model picker observations',
      'remaining usage is unknown',
      'Router export consumes provider-model-catalog/provider-router-export artifacts',
      'node scripts/check-v3-interactive-provider-box.mjs --json',
    ]);
    requireAll(diagnostics, FILES.requestRuntime, sources.requestRuntime, [
      'provider-interaction-turn',
      'missiond.provider-interaction-turn.v1',
      'provider-usage-snapshot',
      'missiond.provider-usage-snapshot.v1',
      'provider-model-catalog',
      'missiond.provider-model-catalog.v1',
      'provider-router-export',
      'missiond.provider-router-export.v1',
      'provider-agent-source',
      'missiond.provider-agent-source.v1',
      '(function provider-interaction-box-turn',
      '(function provider-box-model-control',
      '(function provider-box-usage-and-catalog-export',
      'interactive-provider-box',
      'direct `claude --print`',
      '`codex exec`',
      'Gemini `-p -o stream-json`',
      'durable provider logs',
      'PTY screen text',
      'pure-text-single-turn',
      'timeout_cancel_policy',
      'step_records',
      'screen_identity',
      'screen_usage',
      'selected_model',
      'model_quotas',
      'Esc -> /usage -> Enter',
      'Generating...',
      'Working...',
      'Loading...',
      'esc to cancel',
      '`>` alone is not idle',
      'observe -> act -> observe -> verify -> record',
      'MODEL_SWITCH_UNSUPPORTED',
      'MODEL_SWITCH_UNVERIFIED',
      'PROVIDER_TURN_STALLED',
      'PROVIDER_TURN_TIMEOUT_CANCEL_FAILED',
      'Interrupted · What should Antigravity CLI do instead?',
      '? for shortcuts',
      'usage unknown',
      'Router export consumes provider-model-catalog',
      'MissionDAgy',
      'Meow61 provider-source pattern',
      'text=true/tools=false/vision=false',
      'self-built proxy deployment program',
      'codex_exec_text',
      'provider_agent_sources[]',
      'MissionDProviderBox',
      'missiond-codex-agent-gpt-55-xhigh',
      'missiond-claude-code-agent-opus-4-8-xhigh',
      'output-last-message alone is never treated as proof that no tools ran',
    ]);
    requireAll(diagnostics, FILES.requestSurfaces, sources.requestSurfaces, [
      ':interactive-provider-box "provider-interaction-box"',
      ':headless-legacy-status retired-for-jarvis',
      'provider-interaction-box mode=semantic-authoring',
      'provider-interaction-box mode=grounded-direct-answer',
      'MUST NOT silently fall back to deterministic intent generation, `codex exec`, xjpcode text-only, or interactive TUI screen text as semantic output',
    ]);
    requireAll(diagnostics, FILES.workstationEvidence, sources.workstationEvidence, [
      'provider-interaction-box migration target',
      'durable Codex rollout JSONL',
      'headless `codex exec --json --output-last-message`',
    ]);
    requireAll(diagnostics, FILES.universeService, sources.universeService, [
      ':migration-target provider-interaction-box',
      'provider-interaction-box HTTP adapter',
      'interactive PTY plus durable provider-log final extraction',
    ]);
    requireAll(diagnostics, FILES.session, sources.session, [
      'CliEngine::Agy',
      '"agy".to_string()',
      'Agy CLI: model override',
      ' --model ',
      'Codex CLI: workspace root override',
      'Gemini CLI: plan/read-only approval mode enabled',
    ]);
    rejectAll(diagnostics, FILES.session, sources.session, [
      '--print',
      'cmd.arg("exec")',
      '--output-last-message',
      'stdin(std::process::Stdio::null())',
    ]);
    requireAll(diagnostics, FILES.genericCli, sources.genericCli, [
      'send_fire_and_forget',
      'TextComplete',
      'canonical_source_for_engine(self.engine)',
    ]);
    requireAll(diagnostics, FILES.geminiPty, sources.geminiPty, [
      'PTY-based transport for Gemini CLI',
      'driver.ask',
      'ensure_spawned',
    ]);
    requireAll(diagnostics, FILES.agyParser, sources.agyParser, [
      'transcript_full.jsonl',
      'history.jsonl',
      'results come from jsonl',
    ]);
    requireAll(diagnostics, FILES.agyWatcher, sources.agyWatcher, [
      'transcript_full.jsonl',
      'antigravity-cli/brain',
    ]);
    requireAll(diagnostics, FILES.daemonMain, sources.daemonMain, [
      'mod provider_box;',
    ]);
    requireAll(diagnostics, FILES.providerBoxMod, sources.providerBoxMod, [
      'ProviderDriver',
      'ProviderInteractionBox',
      'ProviderBoxArtifactWriter',
      'UnsupportedProviderDriver',
    ]);
    requireAll(diagnostics, FILES.providerBoxTypes, sources.providerBoxTypes, [
      'BoxCommand',
      'WorkerTurn',
      'ModelSwitch',
      'UsageProbe',
      'ModelCatalogExport',
      'PureTextSingleTurn',
      'ProviderBoxResult',
      'PtyObservation',
      'PtyStepAction',
      'PtyStepRecord',
      'PtyStepVerificationStatus',
      'TimeoutCancelPolicy',
      'step_records',
      'timeout_cancel_policy',
      'ProviderUsageSnapshot',
      'ProviderModelCatalog',
      'ProviderRouterExport',
      'MODEL_SWITCH_UNSUPPORTED',
      'USAGE_UNKNOWN',
      'AGY_MODEL_CATALOG_UNSUPPORTED',
      'PURE_TEXT_GUARD_UNSUPPORTED',
      'PROVIDER_TURN_STALLED',
      'PROVIDER_TURN_TIMEOUT_CANCELLED',
      'PROVIDER_TURN_TIMEOUT_CANCEL_FAILED',
      'timeout_cancel_policy_defaults_to_escape_cancel_and_one_retry',
    ]);
    requireAll(diagnostics, FILES.ptyRecognition, sources.ptyRecognition, [
      'ProviderScreenIdentity',
      'ProviderUsageScreen',
      'ProviderModelQuota',
      'screen_identity',
      'screen_usage',
      'cli_version',
      'account',
      'plan',
      'current_model',
      'selected_model',
      'cwd',
      'model_quotas',
      'extract_agy_screen_identity',
      'extract_agy_usage_screen',
      'has_agy_active_status_line',
      'agy:interrupted_ready_for_retry',
      'agy_model_picker_tracks_current_and_selected_model',
      'agy_usage_meter_extracts_visible_model_quotas',
      'agy_generating_spinner_is_running_even_when_prompt_visible',
      'agy_stale_spinner_with_ready_footer_is_idle',
      'agy_interrupted_prompt_is_idle_ready_for_retry',
      'mcp_server_authorization',
      'provider_authorization',
      'startup_provider_authorization',
      'claude_code_mcp_server_authorization_is_provider_authorization_blocked',
    ]);
    requireAll(diagnostics, FILES.providerBoxDriver, sources.providerBoxDriver, [
      'trait ProviderDriver',
      'prompt_authorization',
      'submit_turn',
      'switch_model',
      'probe_usage',
      'discover_models',
      'pure_text_single_turn',
      'UnsupportedProviderDriver',
    ]);
    requireAll(diagnostics, FILES.providerBoxRuntime, sources.providerBoxRuntime, [
      'struct ProviderInteractionBox',
      'register_driver',
      'BoxCommand::WorkerTurn',
      'BoxCommand::ModelSwitch',
      'BoxCommand::UsageProbe',
      'BoxCommand::ModelCatalogExport',
      'BoxCommand::PureTextSingleTurn',
      'usage_probe_returns_unknown_not_fake_remaining_quota',
      'agy_model_catalog_export_is_explicitly_unsupported',
      'pure_text_turn_fails_closed_until_guard_exists',
    ]);
    requireAll(diagnostics, FILES.providerBoxArtifact, sources.providerBoxArtifact, [
      'ProviderBoxArtifactWriter',
      'put_json_artifact',
      'provider-interaction-turn',
    ]);
    requireAll(diagnostics, FILES.providerBoxClaudeCodeDriver, sources.providerBoxClaudeCodeDriver, [
      'accept_prompt_authorization_locked',
      'claude_code_prompt_authorization_allowlist',
      'MISSIOND_PROVIDER_BOX_CLAUDE_CODE_MCP_AUTH_ALLOWLIST',
      'MISSIOND_PROVIDER_BOX_CLAUDE_CODE_AUTH_ALLOWLIST',
      'PROVIDER_PROMPT_AUTHORIZATION_CONFIRMED',
      'DIAG_PROVIDER_CONTROL_ACTION_UNVERIFIED',
      'claude_code_prompt_authorization_extracts_allowlisted_mcp_target',
      'claude_code_prompt_authorization_rejects_unknown_mcp_target',
    ]);
    requireAll(diagnostics, FILES.providerBoxHttpAdapter, sources.providerBoxHttpAdapter, [
      '"prompt_authorization"',
      '"allowlist"',
      '"observe-act-observe selected option before Enter"',
      '"model_launch_selector"',
      '"supported": true',
      '"flag": "--model"',
      'provider_agent_sources',
      'missiond.provider-agent-source.v1',
      'missiond-codex-agent-gpt-55-xhigh',
      'missiond-claude-code-agent-opus-4-8-xhigh',
      'MissionDProviderBox',
    ]);
    requireAll(diagnostics, FILES.aggregate, sources.aggregate, [
      'scripts/check-v3-interactive-provider-box.mjs',
    ]);

    checkLegacyInventoryCoverage(diagnostics, sources.workstationRuntime);
    checkSuspiciousRustCoverage(repo, diagnostics);
  }

  const result = {
    ok: diagnostics.length === 0,
    checked_files: Object.keys(FILES).length,
    legacy_targets: INVENTORIED_LEGACY.map((entry) => entry.id),
    diagnostics,
  };

  if (opts.json) {
    console.log(JSON.stringify(result, null, 2));
  } else if (result.ok) {
    console.log('v3 interactive provider box check OK');
  } else {
    for (const d of diagnostics) console.error(`${d.file}: ${d.message}`);
    console.error(`v3 interactive provider box check FAILED -- ${diagnostics.length} diagnostic(s)`);
  }
  process.exit(result.ok ? 0 : 1);
}

function checkLegacyInventoryCoverage(diagnostics, workstationRuntime) {
  for (const target of INVENTORIED_LEGACY) {
    if (!workstationRuntime.includes(`target ${target.id}`)) {
      diagnostics.push(diag(FILES.workstationRuntime, `legacy target missing from SSOT inventory: ${target.id}`));
    }
    for (const file of target.files) {
      if (!workstationRuntime.includes(file)) {
        diagnostics.push(diag(FILES.workstationRuntime, `legacy target ${target.id} missing file inventory: ${file}`));
      }
    }
    for (const needle of target.needles) {
      if (!workstationRuntime.includes(needle)) {
        diagnostics.push(diag(FILES.workstationRuntime, `legacy target ${target.id} missing pattern inventory: ${needle}`));
      }
    }
  }
}

function checkSuspiciousRustCoverage(repo, diagnostics) {
  const allowedFiles = new Set(INVENTORIED_LEGACY.flatMap((entry) => entry.files));
  for (const rel of listFiles(path.join(repo, 'crates'), '.rs', repo)) {
    const source = fs.readFileSync(path.join(repo, rel), 'utf8');
    for (const pattern of SUSPICIOUS_RUST_PATTERNS) {
      if (!pattern.re.test(source)) continue;
      if (allowedFiles.has(rel)) continue;
      if (isApprovedCodexExecTextLane(rel, pattern.id, source)) continue;
      diagnostics.push(diag(rel, `un-inventoried provider CLI headless marker: ${pattern.id}`));
    }
  }
}

function isApprovedCodexExecTextLane(rel, patternId, source) {
  if (rel !== FILES.providerBoxCodexDriver) return false;
  if (patternId !== 'codex-exec') return false;
  const required = [
    'CODEX_EXEC_TEXT_PROVIDER',
    'codex_exec_text',
    '--output-last-message',
    '--ignore-user-config',
    '--ignore-rules',
    '--sandbox',
    'read-only',
    'features.shell_tool=false',
    'analyze_codex_exec_jsonl',
    'DIAG_PROVIDER_TEXT_ONLY_VIOLATION',
  ];
  return required.every((needle) => source.includes(needle))
    && (source.includes('approval_policy="never"') || source.includes('approval_policy=\\"never\\"'));
}

function listFiles(dir, suffix, repo) {
  const out = [];
  let entries;
  try {
    entries = fs.readdirSync(dir, { withFileTypes: true });
  } catch {
    return out;
  }
  for (const entry of entries) {
    const abs = path.join(dir, entry.name);
    if (entry.isDirectory()) {
      if (entry.name === 'target') continue;
      out.push(...listFiles(abs, suffix, repo));
    } else if (entry.isFile() && entry.name.endsWith(suffix)) {
      out.push(path.relative(repo, abs));
    }
  }
  return out;
}

function requireAll(diagnostics, file, source, needles) {
  for (const needle of needles) {
    if (!source.includes(needle)) {
      diagnostics.push(diag(file, `missing required text: ${needle}`));
    }
  }
}

function rejectAll(diagnostics, file, source, needles) {
  for (const needle of needles) {
    if (source.includes(needle)) {
      diagnostics.push(diag(file, `forbidden text present in interactive PTY command construction: ${needle}`));
    }
  }
}

function diag(file, message) {
  return { file, line: 1, column: 1, code: 'V3_INTERACTIVE_PROVIDER_BOX', message };
}

main();
