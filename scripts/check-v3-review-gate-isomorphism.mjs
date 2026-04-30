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
  - directive.rs + directive/approval_review.rs +
    directive/approval_review/proposer.rs / plan.rs / workflow.rs go through
    that dispatcher (no bypass) and call maybe_emit_review_question_resolved
    on approve/reject/needs_changes, while directive proposer helpers stay in
    their own V3-pinned module.
  - The MCP directive / plan / workflow tools expose review_gate_policy with
    the manual|emit_question|off enum AND the wave-15 review_decision with
    the approved|rejected|needs_changes enum.
`;

const DEFAULT_FILES = {
  blueprint: '.missiond/v3/missiond-blueprint.lisp',
  reviewGate: 'crates/missiond-daemon/src/handlers/knowledge/review_gate.rs',
  reviewGateCreated: 'crates/missiond-daemon/src/handlers/knowledge/review_gate/created.rs',
  reviewGateResolution: 'crates/missiond-daemon/src/handlers/knowledge/review_gate/resolution.rs',
  reviewGateResolutionAutomation:
    'crates/missiond-daemon/src/handlers/knowledge/review_gate/resolution/automation.rs',
  reviewGateResolutionEmitter:
    'crates/missiond-daemon/src/handlers/knowledge/review_gate/resolution/emitter.rs',
  reviewGateResolutionEnvelope:
    'crates/missiond-daemon/src/handlers/knowledge/review_gate/resolution/envelope.rs',
  reviewGateResolutionInput:
    'crates/missiond-daemon/src/handlers/knowledge/review_gate/resolution/input.rs',
  reviewGateResolutionPayload:
    'crates/missiond-daemon/src/handlers/knowledge/review_gate/resolution/payload.rs',
  reviewGateResolutionSubscriber:
    'crates/missiond-daemon/src/handlers/knowledge/review_gate/resolution/subscriber.rs',
  reviewGateAutoAnswer: 'crates/missiond-daemon/src/handlers/knowledge/review_gate/auto_answer.rs',
  reviewGateLlmApproval: 'crates/missiond-daemon/src/handlers/knowledge/review_gate/llm_approval.rs',
  reviewGateLlmProposal:
    'crates/missiond-daemon/src/handlers/knowledge/review_gate/llm_approval/proposal.rs',
  reviewGateLlmApplyGate:
    'crates/missiond-daemon/src/handlers/knowledge/review_gate/llm_approval/apply_gate.rs',
  reviewGateLlmApplyGateEvaluate:
    'crates/missiond-daemon/src/handlers/knowledge/review_gate/llm_approval/apply_gate/evaluate.rs',
  reviewGateLlmApplyGateHash:
    'crates/missiond-daemon/src/handlers/knowledge/review_gate/llm_approval/apply_gate/hash.rs',
  reviewGateLlmApplyGateInput:
    'crates/missiond-daemon/src/handlers/knowledge/review_gate/llm_approval/apply_gate/input.rs',
  reviewGateLlmApplyGateOutcome:
    'crates/missiond-daemon/src/handlers/knowledge/review_gate/llm_approval/apply_gate/outcome.rs',
  reviewGateLlmApplyGatePayload:
    'crates/missiond-daemon/src/handlers/knowledge/review_gate/llm_approval/apply_gate/payload.rs',
  reviewGateLlmApplyGatePreflight:
    'crates/missiond-daemon/src/handlers/knowledge/review_gate/llm_approval/apply_gate/preflight.rs',
  reviewGateTests: 'crates/missiond-daemon/src/handlers/knowledge/review_gate/tests.rs',
  directive: 'crates/missiond-daemon/src/handlers/knowledge/directive.rs',
  directiveCompileAuthoring: 'crates/missiond-daemon/src/handlers/knowledge/directive/compile_authoring.rs',
  directiveApprovalReview: 'crates/missiond-daemon/src/handlers/knowledge/directive/approval_review.rs',
  directiveApprovalApprove:
    'crates/missiond-daemon/src/handlers/knowledge/directive/approval_review/approve.rs',
  directiveApprovalArchive:
    'crates/missiond-daemon/src/handlers/knowledge/directive/approval_review/archive.rs',
  directiveApprovalProposer:
    'crates/missiond-daemon/src/handlers/knowledge/directive/approval_review/proposer.rs',
  directiveApprovalSubscriber:
    'crates/missiond-daemon/src/handlers/knowledge/directive/approval_review/subscriber.rs',
  plan: 'crates/missiond-daemon/src/handlers/knowledge/plan.rs',
  planCompileAuthoring: 'crates/missiond-daemon/src/handlers/knowledge/plan/compile_authoring.rs',
  planApprovalReview: 'crates/missiond-daemon/src/handlers/knowledge/plan/approval_review.rs',
  planApprovalApprove: 'crates/missiond-daemon/src/handlers/knowledge/plan/approval_review/approve.rs',
  planApprovalMark: 'crates/missiond-daemon/src/handlers/knowledge/plan/approval_review/mark.rs',
  planApprovalProposer:
    'crates/missiond-daemon/src/handlers/knowledge/plan/approval_review/proposer.rs',
  planApprovalSubscriber:
    'crates/missiond-daemon/src/handlers/knowledge/plan/approval_review/subscriber.rs',
  planApprovalSupersede:
    'crates/missiond-daemon/src/handlers/knowledge/plan/approval_review/supersede.rs',
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
  'crates/missiond-daemon/src/handlers/knowledge/review_gate/created.rs',
  'crates/missiond-daemon/src/handlers/knowledge/review_gate/resolution.rs',
  'crates/missiond-daemon/src/handlers/knowledge/review_gate/resolution/automation.rs',
  'crates/missiond-daemon/src/handlers/knowledge/review_gate/resolution/emitter.rs',
  'crates/missiond-daemon/src/handlers/knowledge/review_gate/resolution/envelope.rs',
  'crates/missiond-daemon/src/handlers/knowledge/review_gate/resolution/input.rs',
  'crates/missiond-daemon/src/handlers/knowledge/review_gate/resolution/payload.rs',
  'crates/missiond-daemon/src/handlers/knowledge/review_gate/resolution/subscriber.rs',
  'crates/missiond-daemon/src/handlers/knowledge/review_gate/auto_answer.rs',
  'crates/missiond-daemon/src/handlers/knowledge/review_gate/llm_approval.rs',
  'crates/missiond-daemon/src/handlers/knowledge/review_gate/llm_approval/proposal.rs',
  'crates/missiond-daemon/src/handlers/knowledge/review_gate/llm_approval/apply_gate.rs',
  'crates/missiond-daemon/src/handlers/knowledge/review_gate/llm_approval/apply_gate/evaluate.rs',
  'crates/missiond-daemon/src/handlers/knowledge/review_gate/llm_approval/apply_gate/hash.rs',
  'crates/missiond-daemon/src/handlers/knowledge/review_gate/llm_approval/apply_gate/input.rs',
  'crates/missiond-daemon/src/handlers/knowledge/review_gate/llm_approval/apply_gate/outcome.rs',
  'crates/missiond-daemon/src/handlers/knowledge/review_gate/llm_approval/apply_gate/payload.rs',
  'crates/missiond-daemon/src/handlers/knowledge/review_gate/llm_approval/apply_gate/preflight.rs',
  'crates/missiond-daemon/src/handlers/knowledge/review_gate/tests.rs',
  'crates/missiond-daemon/src/handlers/knowledge/directive.rs',
  'crates/missiond-daemon/src/handlers/knowledge/directive/approval_review.rs',
  'crates/missiond-daemon/src/handlers/knowledge/directive/approval_review/approve.rs',
  'crates/missiond-daemon/src/handlers/knowledge/directive/approval_review/archive.rs',
  'crates/missiond-daemon/src/handlers/knowledge/directive/approval_review/proposer.rs',
  'crates/missiond-daemon/src/handlers/knowledge/directive/approval_review/subscriber.rs',
  'crates/missiond-daemon/src/handlers/knowledge/plan.rs',
  'crates/missiond-daemon/src/handlers/knowledge/plan/compile_authoring.rs',
  'crates/missiond-daemon/src/handlers/knowledge/plan/approval_review.rs',
  'crates/missiond-daemon/src/handlers/knowledge/plan/approval_review/approve.rs',
  'crates/missiond-daemon/src/handlers/knowledge/plan/approval_review/mark.rs',
  'crates/missiond-daemon/src/handlers/knowledge/plan/approval_review/proposer.rs',
  'crates/missiond-daemon/src/handlers/knowledge/plan/approval_review/subscriber.rs',
  'crates/missiond-daemon/src/handlers/knowledge/plan/approval_review/supersede.rs',
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
  'mod tests;',
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

const DIRECTIVE_APPROVAL_REVIEW_RS_NEEDLES = [
  'mod approve;',
  'mod archive;',
  'mod proposer;',
  'mod subscriber;',
  'pub(super) use self::approve::action_approve;',
  'pub(super) use self::archive::action_archive;',
  'use self::proposer::{',
  'request_directive_auto_approve_proposal',
  'pub(crate) use self::subscriber::{handle_review_resolved_event, DirectiveSubscriberOutcome};',
];

const DIRECTIVE_APPROVAL_APPROVE_RS_NEEDLES = [
  'use super::*;',
  'pub(in crate::handlers::knowledge::directive) async fn action_approve',
  'async fn action_approve_with_resolution',
  'async fn action_approve_with_policy_only',
  'parse_review_resolution_input(args)',
  'maybe_emit_review_question_resolved',
  'directive_approve',
];

const DIRECTIVE_APPROVAL_ARCHIVE_RS_NEEDLES = [
  'use super::*;',
  'pub(in crate::handlers::knowledge::directive) async fn action_archive',
  'async fn action_archive_with_resolution',
  'async fn action_archive_with_policy_only',
  'parse_review_resolution_input(args)',
  'maybe_emit_review_question_resolved',
  'DirectiveStatus::Archived',
];

const DIRECTIVE_APPROVAL_PROPOSER_RS_NEEDLES = [
  'use super::*;',
  'pub(super) async fn request_directive_auto_approve_proposal',
  'pub(super) fn attach_directive_proposal_block',
  'pub(super) fn attach_directive_apply_gate_block',
  'pub(super) fn parse_proposer_mode_or_error',
  'pub(super) fn directive_proposer_summary',
  'DIRECTIVE_REVIEW_PROPOSER_CALLER',
  'SONNET_PROPOSER_MAX_TOKENS',
];

const DIRECTIVE_APPROVAL_SUBSCRIBER_RS_NEEDLES = [
  'use super::*;',
  'pub(crate) enum DirectiveSubscriberOutcome',
  'pub(crate) async fn handle_review_resolved_event',
  'validate_review_resolution_envelope',
  'DIRECTIVE_REVIEW_ACTIONS',
  'DirectiveStatus::Archived',
];

const PLAN_RS_NEEDLES = [
  'mod approval_review',
  'use approval_review::{action_approve, action_mark, action_supersede}',
  'pub(crate) use approval_review::{handle_review_resolved_event, PlanSubscriberOutcome}',
];

const PLAN_COMPILE_AUTHORING_RS_NEEDLES = [
  'parse_review_gate_policy(args)',
  'review_gate_policy_was_explicit(args)',
  'parse_compile_review_gate(args)',
  'apply_compile_review_gates(',
];

const PLAN_APPROVAL_REVIEW_RS_NEEDLES = [
  'mod approve;',
  'mod mark;',
  'mod proposer;',
  'mod subscriber;',
  'mod supersede;',
  'pub(super) use self::approve::action_approve;',
  'pub(super) use self::mark::action_mark;',
  'use self::proposer::{',
  'request_plan_auto_approve_proposal',
  'pub(crate) use self::subscriber::{handle_review_resolved_event, PlanSubscriberOutcome};',
  'pub(super) use self::supersede::action_supersede;',
];

const PLAN_APPROVAL_APPROVE_RS_NEEDLES = [
  'use super::*;',
  'async fn action_approve',
  'async fn action_approve_with_resolution',
  'async fn plan_action_approve_with_policy_only',
  'parse_review_resolution_input(args)',
  'maybe_emit_review_question_resolved(',
  'PlanStatus::Approved',
];

const PLAN_APPROVAL_MARK_RS_NEEDLES = [
  'use super::*;',
  'async fn action_mark',
  'async fn action_mark_with_resolution',
  'async fn plan_action_mark_with_policy_only',
  'parse_review_resolution_input(args)',
  'maybe_emit_review_question_resolved(',
  'target_raw',
  'PlanStatus::Approved',
];

const PLAN_APPROVAL_SUPERSEDE_RS_NEEDLES = [
  'use super::*;',
  'async fn action_supersede',
  'async fn action_supersede_with_resolution',
  'async fn plan_action_supersede_with_policy_only',
  'parse_review_resolution_input(args)',
  'maybe_emit_review_question_resolved(',
  'PlanStatus::Superseded',
  'destructive',
];

const PLAN_APPROVAL_PROPOSER_RS_NEEDLES = [
  'use super::*;',
  'pub(super) fn build_plan_automation_ctx',
  'pub(super) async fn request_plan_auto_approve_proposal',
  'pub(super) fn attach_plan_proposal_block',
  'pub(super) fn attach_plan_apply_gate_block',
  'PLAN_REVIEW_PROPOSER_CALLER',
  'SONNET_PLAN_PROPOSER_MAX_TOKENS',
];

const PLAN_APPROVAL_SUBSCRIBER_RS_NEEDLES = [
  'use super::*;',
  'pub(crate) enum PlanSubscriberOutcome',
  'pub(crate) async fn handle_review_resolved_event',
  'validate_review_resolution_envelope',
  'PLAN_REVIEW_ACTIONS',
  'PlanStatus::Approved',
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

  const reviewGateSurface = `${sources.reviewGate}\n${sources.reviewGateCreated}\n${sources.reviewGateResolution}\n${sources.reviewGateResolutionAutomation}\n${sources.reviewGateResolutionEmitter}\n${sources.reviewGateResolutionEnvelope}\n${sources.reviewGateResolutionInput}\n${sources.reviewGateResolutionPayload}\n${sources.reviewGateResolutionSubscriber}\n${sources.reviewGateAutoAnswer}\n${sources.reviewGateLlmApproval}\n${sources.reviewGateLlmProposal}\n${sources.reviewGateLlmApplyGate}\n${sources.reviewGateLlmApplyGateEvaluate}\n${sources.reviewGateLlmApplyGateHash}\n${sources.reviewGateLlmApplyGateInput}\n${sources.reviewGateLlmApplyGateOutcome}\n${sources.reviewGateLlmApplyGatePayload}\n${sources.reviewGateLlmApplyGatePreflight}`;
  const reviewGateSurfaceLabel = `${files.reviewGate} + ${files.reviewGateCreated} + ${files.reviewGateResolution} + ${files.reviewGateResolutionAutomation} + ${files.reviewGateResolutionEmitter} + ${files.reviewGateResolutionEnvelope} + ${files.reviewGateResolutionInput} + ${files.reviewGateResolutionPayload} + ${files.reviewGateResolutionSubscriber} + ${files.reviewGateAutoAnswer} + ${files.reviewGateLlmApproval} + ${files.reviewGateLlmProposal} + ${files.reviewGateLlmApplyGate} + ${files.reviewGateLlmApplyGateEvaluate} + ${files.reviewGateLlmApplyGateHash} + ${files.reviewGateLlmApplyGateInput} + ${files.reviewGateLlmApplyGateOutcome} + ${files.reviewGateLlmApplyGatePayload} + ${files.reviewGateLlmApplyGatePreflight}`;
  requireAll(diagnostics, reviewGateSurfaceLabel, reviewGateSurface, REVIEW_GATE_RS_NEEDLES);
  requireAll(diagnostics, files.reviewGateCreated, sources.reviewGateCreated, [
    'pub(crate) fn derive_review_question_id',
    'pub(crate) fn parse_compile_review_gate',
    'pub(crate) enum ReviewGatePolicy',
    'pub(crate) async fn apply_compile_review_gates',
    'pub(crate) async fn auto_emit_review_question_after_artifact_write',
    'pub(crate) const PLAN_NODE_REVIEW_DEFAULT_ACTION',
  ]);
  requireAll(diagnostics, files.reviewGateResolution, sources.reviewGateResolution, [
    'mod automation;',
    'mod emitter;',
    'mod envelope;',
    'mod input;',
    'mod payload;',
    'mod subscriber;',
    'pub(crate) use self::automation::*;',
    'pub(crate) use self::emitter::*;',
    'pub(crate) use self::envelope::*;',
    'pub(crate) use self::input::*;',
    'pub(crate) use self::payload::*;',
    'pub(crate) use self::subscriber::*;',
  ]);
  requireAll(diagnostics, files.reviewGateResolutionEmitter, sources.reviewGateResolutionEmitter, [
    'pub(crate) fn parse_resolution_review_question_id',
    'pub(crate) struct ResolutionDecisionMeta',
    'pub(crate) fn build_resolution_event',
    'pub(crate) async fn maybe_emit_review_question_resolved',
    'BUS_PUBLISH_FAILED',
    'DB action already committed',
  ]);
  requireAll(diagnostics, files.reviewGateResolutionInput, sources.reviewGateResolutionInput, [
    'pub(crate) enum ReviewDecision',
    'ReviewDecision::Approved',
    'ReviewDecision::Rejected',
    'ReviewDecision::NeedsChanges',
    'pub(crate) fn parse_review_resolution_input',
    'pub(crate) fn parse_plan_node_resume_input',
  ]);
  requireAll(diagnostics, files.reviewGateResolutionAutomation, sources.reviewGateResolutionAutomation, [
    'pub(crate) enum ReviewAutomationPolicy',
    'pub(crate) struct ReviewAutomationContext',
    'pub(crate) fn evaluate_review_automation',
    'pub(crate) fn stamp_review_automation_payload',
  ]);
  requireAll(diagnostics, files.reviewGateResolutionEnvelope, sources.reviewGateResolutionEnvelope, [
    'pub(crate) struct ParsedReviewQuestionId',
    'pub(crate) enum ReviewIdParseError',
    'pub(crate) fn parse_review_question_id_struct',
    'pub(crate) enum ResolutionValidationError',
    'pub(crate) const WAVE14_SUPPORTED_SCOPES',
    'pub(crate) fn validate_review_resolution_envelope',
  ]);
  requireAll(diagnostics, files.reviewGateResolutionPayload, sources.reviewGateResolutionPayload, [
    'pub(crate) enum ResolutionOutcome',
    'pub(crate) fn stamp_resolution_payload',
    'pub(crate) fn stamp_needs_changes_next_step',
    'pub(crate) fn resolution_wire_string',
  ]);
  requireAll(diagnostics, files.reviewGateResolutionSubscriber, sources.reviewGateResolutionSubscriber, [
    'pub(crate) enum ReviewResolvedDispatch',
    'pub(crate) fn parse_subscriber_resolution_string',
    'pub(crate) fn plan_review_resolved_dispatch',
  ]);
  requireAll(diagnostics, files.reviewGateAutoAnswer, sources.reviewGateAutoAnswer, [
    'pub(crate) enum AutoAnswerPolicy',
    'pub(crate) fn parse_auto_answer_policy',
    'pub(crate) fn auto_answer_policy_was_explicit',
    'pub(crate) fn evaluate_auto_answer_policy',
    'pub(crate) fn stamp_auto_answer_payload',
    'pub(crate) fn is_destructive_review_action',
    'NEVER auto-reject',
  ]);
  requireAll(diagnostics, files.reviewGateLlmApproval, sources.reviewGateLlmApproval, [
    'mod apply_gate;',
    'mod proposal;',
    'pub(crate) use apply_gate::{',
    'pub(crate) use proposal::{',
  ]);
  requireAll(diagnostics, files.reviewGateLlmProposal, sources.reviewGateLlmProposal, [
    'pub(crate) enum LlmAutoApproveProposalMode',
    'pub(crate) fn parse_llm_auto_approve_proposal_mode',
    'pub(crate) fn parse_llm_auto_approve_proposal',
    'pub(crate) fn enforce_proposal_invariants',
    'pub(crate) fn build_llm_auto_approve_proposal_system_prompt',
    'pub(crate) fn stamp_llm_auto_approve_proposal_payload',
    'never auto-approve',
  ]);
  requireAll(diagnostics, files.reviewGateLlmApplyGate, sources.reviewGateLlmApplyGate, [
    'mod evaluate;',
    'mod hash;',
    'mod input;',
    'mod outcome;',
    'mod payload;',
    'mod preflight;',
    'pub(crate) use self::evaluate::*;',
    'pub(crate) use self::hash::*;',
    'pub(crate) use self::input::*;',
    'pub(crate) use self::outcome::*;',
    'pub(crate) use self::payload::*;',
    'pub(crate) use self::preflight::*;',
    'never auto-approve',
  ]);
  requireAll(diagnostics, files.reviewGateLlmApplyGateInput, sources.reviewGateLlmApplyGateInput, [
    'pub(crate) const APPLY_GATE_MISSING_PROPOSAL_HASH',
    'pub(crate) const APPLY_GATE_PROPOSAL_HASH_MISMATCH',
    'pub(crate) const APPLY_GATE_INVALID_PARAM',
    'pub(crate) enum LlmApproveApplyStatus',
    'pub(crate) enum ProposalHashStatus',
    'pub(crate) struct LlmApproveApplyGateInput',
    'pub(crate) fn parse_llm_approve_apply_gate_input',
  ]);
  requireAll(diagnostics, files.reviewGateLlmApplyGateHash, sources.reviewGateLlmApplyGateHash, [
    'pub(crate) fn compute_proposal_hash',
  ]);
  requireAll(
    diagnostics,
    files.reviewGateLlmApplyGateOutcome,
    sources.reviewGateLlmApplyGateOutcome,
    [
      'pub(crate) struct LlmApproveApplyGateOutcome',
      'pub(crate) fn to_response_json',
    ],
  );
  requireAll(
    diagnostics,
    files.reviewGateLlmApplyGateEvaluate,
    sources.reviewGateLlmApplyGateEvaluate,
    [
      'pub(crate) fn evaluate_llm_approve_apply_gate',
      'is_destructive_review_action',
      'never auto-reject',
    ],
  );
  requireAll(
    diagnostics,
    files.reviewGateLlmApplyGatePreflight,
    sources.reviewGateLlmApplyGatePreflight,
    [
    'pub(crate) fn enforce_apply_gate_preflight',
      'APPLY_GATE_MISSING_PROPOSAL_HASH',
      'APPLY_GATE_PROPOSAL_HASH_MISMATCH',
    ],
  );
  requireAll(
    diagnostics,
    files.reviewGateLlmApplyGatePayload,
    sources.reviewGateLlmApplyGatePayload,
    [
    'pub(crate) fn stamp_llm_approve_apply_gate_payload',
      'pub(crate) fn stamp_proposal_hash_payload',
    ],
  );
  requireAll(diagnostics, files.reviewGateTests, sources.reviewGateTests, [
    'use super::*;',
    'smoke_wave22_07_review_apply_gate_rejects_missing_hash_accepts_fixture_hash',
    'smoke_wave22_07_review_apply_gate_pins_wave21_06_five_invariants',
    'proposal_invariants_round_trip_never_surface_rejected',
  ]);
  const directiveCallerSurface = `${sources.directive}\n${sources.directiveCompileAuthoring}\n${sources.directiveApprovalReview}\n${sources.directiveApprovalApprove}\n${sources.directiveApprovalArchive}\n${sources.directiveApprovalProposer}\n${sources.directiveApprovalSubscriber}`;
  const directiveCallerLabel = `${files.directive} + ${files.directiveCompileAuthoring} + ${files.directiveApprovalReview} + ${files.directiveApprovalApprove} + ${files.directiveApprovalArchive} + ${files.directiveApprovalProposer} + ${files.directiveApprovalSubscriber}`;
  requireAll(diagnostics, directiveCallerLabel, directiveCallerSurface, DIRECTIVE_RS_NEEDLES);
  requireAll(
    diagnostics,
    files.directiveApprovalReview,
    sources.directiveApprovalReview,
    DIRECTIVE_APPROVAL_REVIEW_RS_NEEDLES,
  );
  requireAll(
    diagnostics,
    files.directiveApprovalApprove,
    sources.directiveApprovalApprove,
    DIRECTIVE_APPROVAL_APPROVE_RS_NEEDLES,
  );
  requireAll(
    diagnostics,
    files.directiveApprovalArchive,
    sources.directiveApprovalArchive,
    DIRECTIVE_APPROVAL_ARCHIVE_RS_NEEDLES,
  );
  requireAll(
    diagnostics,
    files.directiveApprovalProposer,
    sources.directiveApprovalProposer,
    DIRECTIVE_APPROVAL_PROPOSER_RS_NEEDLES,
  );
  requireAll(
    diagnostics,
    files.directiveApprovalSubscriber,
    sources.directiveApprovalSubscriber,
    DIRECTIVE_APPROVAL_SUBSCRIBER_RS_NEEDLES,
  );
  requireAll(diagnostics, files.plan, sources.plan, PLAN_RS_NEEDLES);
  requireAll(diagnostics, files.planCompileAuthoring, sources.planCompileAuthoring, PLAN_COMPILE_AUTHORING_RS_NEEDLES);
  requireAll(diagnostics, files.planApprovalReview, sources.planApprovalReview, PLAN_APPROVAL_REVIEW_RS_NEEDLES);
  requireAll(
    diagnostics,
    files.planApprovalApprove,
    sources.planApprovalApprove,
    PLAN_APPROVAL_APPROVE_RS_NEEDLES,
  );
  requireAll(
    diagnostics,
    files.planApprovalMark,
    sources.planApprovalMark,
    PLAN_APPROVAL_MARK_RS_NEEDLES,
  );
  requireAll(
    diagnostics,
    files.planApprovalSupersede,
    sources.planApprovalSupersede,
    PLAN_APPROVAL_SUPERSEDE_RS_NEEDLES,
  );
  requireAll(
    diagnostics,
    files.planApprovalProposer,
    sources.planApprovalProposer,
    PLAN_APPROVAL_PROPOSER_RS_NEEDLES,
  );
  requireAll(
    diagnostics,
    files.planApprovalSubscriber,
    sources.planApprovalSubscriber,
    PLAN_APPROVAL_SUBSCRIBER_RS_NEEDLES,
  );
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
    [DEFAULT_FILES.reviewGateCreated]: buildGoodReviewGateCreated(),
    [DEFAULT_FILES.reviewGateResolution]: buildGoodReviewGateResolution(),
    [DEFAULT_FILES.reviewGateResolutionAutomation]: buildGoodReviewGateResolutionAutomation(),
    [DEFAULT_FILES.reviewGateResolutionEmitter]: buildGoodReviewGateResolutionEmitter(),
    [DEFAULT_FILES.reviewGateResolutionEnvelope]: buildGoodReviewGateResolutionEnvelope(),
    [DEFAULT_FILES.reviewGateResolutionInput]: buildGoodReviewGateResolutionInput(),
    [DEFAULT_FILES.reviewGateResolutionPayload]: buildGoodReviewGateResolutionPayload(),
    [DEFAULT_FILES.reviewGateResolutionSubscriber]: buildGoodReviewGateResolutionSubscriber(),
    [DEFAULT_FILES.reviewGateAutoAnswer]: buildGoodReviewGateAutoAnswer(),
    [DEFAULT_FILES.reviewGateLlmApproval]: buildGoodReviewGateLlmApproval(),
    [DEFAULT_FILES.reviewGateLlmProposal]: buildGoodReviewGateLlmProposal(),
    [DEFAULT_FILES.reviewGateLlmApplyGate]: buildGoodReviewGateLlmApplyGate(),
    [DEFAULT_FILES.reviewGateLlmApplyGateEvaluate]: buildGoodReviewGateLlmApplyGateEvaluate(),
    [DEFAULT_FILES.reviewGateLlmApplyGateHash]: buildGoodReviewGateLlmApplyGateHash(),
    [DEFAULT_FILES.reviewGateLlmApplyGateInput]: buildGoodReviewGateLlmApplyGateInput(),
    [DEFAULT_FILES.reviewGateLlmApplyGateOutcome]: buildGoodReviewGateLlmApplyGateOutcome(),
    [DEFAULT_FILES.reviewGateLlmApplyGatePayload]: buildGoodReviewGateLlmApplyGatePayload(),
    [DEFAULT_FILES.reviewGateLlmApplyGatePreflight]: buildGoodReviewGateLlmApplyGatePreflight(),
    [DEFAULT_FILES.reviewGateTests]: buildGoodReviewGateTests(),
    [DEFAULT_FILES.directive]: buildGoodDirectiveFacadeRs(),
    [DEFAULT_FILES.directiveCompileAuthoring]: buildGoodCallerRs(),
    [DEFAULT_FILES.directiveApprovalReview]: buildGoodDirectiveApprovalReviewRs(),
    [DEFAULT_FILES.directiveApprovalApprove]: buildGoodDirectiveApprovalApproveRs(),
    [DEFAULT_FILES.directiveApprovalArchive]: buildGoodDirectiveApprovalArchiveRs(),
    [DEFAULT_FILES.directiveApprovalProposer]: buildGoodDirectiveApprovalProposerRs(),
    [DEFAULT_FILES.directiveApprovalSubscriber]: buildGoodDirectiveApprovalSubscriberRs(),
    [DEFAULT_FILES.plan]: buildGoodPlanFacadeRs(),
    [DEFAULT_FILES.planCompileAuthoring]: buildGoodCallerRs(),
    [DEFAULT_FILES.planApprovalReview]: buildGoodPlanApprovalReviewRs(),
    [DEFAULT_FILES.planApprovalApprove]: buildGoodPlanApprovalApproveRs(),
    [DEFAULT_FILES.planApprovalMark]: buildGoodPlanApprovalMarkRs(),
    [DEFAULT_FILES.planApprovalProposer]: buildGoodPlanApprovalProposerRs(),
    [DEFAULT_FILES.planApprovalSubscriber]: buildGoodPlanApprovalSubscriberRs(),
    [DEFAULT_FILES.planApprovalSupersede]: buildGoodPlanApprovalSupersedeRs(),
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
  missingPolicy[DEFAULT_FILES.reviewGateCreated] = goodFiles[DEFAULT_FILES.reviewGateCreated].replace(
    'pub(crate) enum ReviewGatePolicy',
    'pub(crate) enum ReviewGateGHOST',
  );
  cases.push({
    name: 'fail: review_gate.rs lost the ReviewGatePolicy enum',
    expectOk: false,
    expectMessage: /pub\(crate\) enum ReviewGatePolicy/,
    files: missingPolicy,
  });

  // ── Fail: directive compile authoring stops calling apply_compile_review_gates. ─
  const directiveBypass = { ...goodFiles };
  directiveBypass[DEFAULT_FILES.directiveCompileAuthoring] = goodFiles[
    DEFAULT_FILES.directiveCompileAuthoring
  ].replace(
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
             "crates/missiond-daemon/src/handlers/knowledge/review_gate/created.rs"
             "crates/missiond-daemon/src/handlers/knowledge/review_gate/resolution.rs"
             "crates/missiond-daemon/src/handlers/knowledge/review_gate/resolution/automation.rs"
             "crates/missiond-daemon/src/handlers/knowledge/review_gate/resolution/emitter.rs"
             "crates/missiond-daemon/src/handlers/knowledge/review_gate/resolution/envelope.rs"
             "crates/missiond-daemon/src/handlers/knowledge/review_gate/resolution/input.rs"
             "crates/missiond-daemon/src/handlers/knowledge/review_gate/resolution/payload.rs"
             "crates/missiond-daemon/src/handlers/knowledge/review_gate/resolution/subscriber.rs"
             "crates/missiond-daemon/src/handlers/knowledge/review_gate/auto_answer.rs"
             "crates/missiond-daemon/src/handlers/knowledge/review_gate/llm_approval.rs"
             "crates/missiond-daemon/src/handlers/knowledge/review_gate/llm_approval/proposal.rs"
             "crates/missiond-daemon/src/handlers/knowledge/review_gate/llm_approval/apply_gate.rs"
             "crates/missiond-daemon/src/handlers/knowledge/review_gate/llm_approval/apply_gate/evaluate.rs"
             "crates/missiond-daemon/src/handlers/knowledge/review_gate/llm_approval/apply_gate/hash.rs"
             "crates/missiond-daemon/src/handlers/knowledge/review_gate/llm_approval/apply_gate/input.rs"
             "crates/missiond-daemon/src/handlers/knowledge/review_gate/llm_approval/apply_gate/outcome.rs"
             "crates/missiond-daemon/src/handlers/knowledge/review_gate/llm_approval/apply_gate/payload.rs"
             "crates/missiond-daemon/src/handlers/knowledge/review_gate/llm_approval/apply_gate/preflight.rs"
             "crates/missiond-daemon/src/handlers/knowledge/review_gate/tests.rs"
             "crates/missiond-daemon/src/handlers/knowledge/directive.rs"
             "crates/missiond-daemon/src/handlers/knowledge/directive/approval_review.rs"
             "crates/missiond-daemon/src/handlers/knowledge/directive/approval_review/approve.rs"
             "crates/missiond-daemon/src/handlers/knowledge/directive/approval_review/archive.rs"
             "crates/missiond-daemon/src/handlers/knowledge/directive/approval_review/proposer.rs"
             "crates/missiond-daemon/src/handlers/knowledge/directive/approval_review/subscriber.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan/compile_authoring.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan/approval_review.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan/approval_review/approve.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan/approval_review/mark.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan/approval_review/proposer.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan/approval_review/subscriber.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan/approval_review/supersede.rs"
             "crates/missiond-daemon/src/handlers/knowledge/workflow.rs"
             "crates/missiond-mcp/src/tools/knowledge/directive.rs"
             "crates/missiond-mcp/src/tools/knowledge/plan.rs"
             "crates/missiond-mcp/src/tools/knowledge/workflow.rs"]
      :note "alignment-review-gate + plan-review-gate event emission for the file-first directive / plan / workflow surfaces. directive/approval_review.rs owns the directive review facade; directive/approval_review/approve.rs owns directive approve transitions; directive/approval_review/archive.rs owns directive archive transitions. ReviewGatePolicy is a closed enum manual | emit_question | off; the dispatcher apply_compile_review_gates fans out to maybe_emit_review_question_created (legacy explicit) or auto_emit_review_question_after_artifact_write (wave-14 auto). review_gate/resolution.rs is the review-resolution facade; resolution/input.rs owns ReviewDecision approved | rejected | needs_changes and explicit resume inputs; resolution/emitter.rs owns maybe_emit_review_question_resolved; resolution/envelope.rs owns review_question_id parsing/validation; resolution/automation.rs owns deterministic review automation; resolution/payload.rs owns response stamping; resolution/subscriber.rs owns Resolved-event dispatch. llm_approval/apply_gate.rs is the apply-gate facade; apply_gate/input.rs owns status/input/error-code vocabulary; apply_gate/hash.rs owns proposal hash derivation; apply_gate/outcome.rs owns response JSON; apply_gate/evaluate.rs owns the six-gate pure evaluator; apply_gate/preflight.rs owns fail-fast hash validation; apply_gate/payload.rs owns response stamping. The gate never auto-approves, never blocks the primary DB / file-write outcome, and never waits for a human; bus failures surface review_question_warning + the deterministic review_question_id so callers can retry or resolve manually with the same id. directive / plan / workflow approve / reject paths route through maybe_emit_review_question_resolved without bypassing the gate."))
  (compression-contract
    :checks ["node scripts/check-v3-review-gate-isomorphism.mjs"]))
`;
}

function buildGoodReviewGate() {
  return `// fixture
mod created;
mod resolution;
mod auto_answer;
mod llm_approval;
#[cfg(test)]
mod tests;
pub(crate) use created::{ReviewGatePolicy, apply_compile_review_gates};
pub(crate) use resolution::{ReviewDecision, maybe_emit_review_question_resolved};
pub(crate) use auto_answer::{AutoAnswerPolicy, evaluate_auto_answer_policy};
pub(crate) use llm_approval::{LlmAutoApproveProposalMode, LlmApproveApplyStatus};
// payload markers: review_question_warning + BUS_PUBLISH_FAILED
// "persisted artifact remains intact" / "DB action already committed"
`;
}

function buildGoodReviewGateCreated() {
  return `// fixture
pub(crate) enum ReviewGatePolicy { Manual, EmitQuestion, Off }
const POLICIES: &[&str] = &["ReviewGatePolicy::Manual", "ReviewGatePolicy::EmitQuestion", "ReviewGatePolicy::Off"];
pub(crate) fn parse_compile_review_gate() {}
pub(crate) fn parse_review_gate_policy() {}
pub(crate) fn review_gate_policy_was_explicit() -> bool { false }
pub(crate) fn derive_review_question_id() -> String { String::new() }
pub(crate) fn derive_review_question_id_for_artifact() -> String { String::new() }
pub(crate) async fn apply_compile_review_gates() {}
pub(crate) async fn maybe_emit_review_question_created() {}
pub(crate) async fn auto_emit_review_question_after_artifact_write() {}
pub(crate) const PLAN_NODE_REVIEW_DEFAULT_ACTION: &str = "plan-node";
// payload markers: review_question_warning + BUS_PUBLISH_FAILED
// "persisted artifact remains intact"
`;
}

function buildGoodReviewGateResolution() {
  return `// fixture
mod automation;
mod emitter;
mod envelope;
mod input;
mod payload;
mod subscriber;
pub(crate) use self::automation::*;
pub(crate) use self::emitter::*;
pub(crate) use self::envelope::*;
pub(crate) use self::input::*;
pub(crate) use self::payload::*;
pub(crate) use self::subscriber::*;
`;
}

function buildGoodReviewGateResolutionAutomation() {
  return `// fixture
pub(crate) enum ReviewAutomationPolicy { Manual, Suggest, AutoSafe }
pub(crate) struct ReviewAutomationContext;
pub(crate) fn evaluate_review_automation() {}
pub(crate) fn stamp_review_automation_payload() {}
`;
}

function buildGoodReviewGateResolutionEmitter() {
  return `// fixture
pub(crate) fn parse_resolution_review_question_id() {}
pub(crate) struct ResolutionDecisionMeta;
pub(crate) fn build_resolution_event() {}
pub(crate) async fn maybe_emit_review_question_resolved() {}
// payload markers: review_question_warning + BUS_PUBLISH_FAILED
// "DB action already committed"
`;
}

function buildGoodReviewGateResolutionEnvelope() {
  return `// fixture
pub(crate) struct ParsedReviewQuestionId;
pub(crate) enum ReviewIdParseError { MissingPrefix }
pub(crate) fn parse_review_question_id_struct() {}
pub(crate) enum ResolutionValidationError { ScopeMismatch }
pub(crate) const WAVE14_SUPPORTED_SCOPES: &[&str] = &["directive", "plan", "workflow"];
pub(crate) fn validate_review_resolution_envelope() {}
`;
}

function buildGoodReviewGateResolutionInput() {
  return `// fixture
pub(crate) enum ReviewDecision { Approved, Rejected, NeedsChanges }
const DECISIONS: &[&str] = &["ReviewDecision::Approved", "ReviewDecision::Rejected", "ReviewDecision::NeedsChanges"];
pub(crate) fn parse_review_resolution_input() {}
pub(crate) fn parse_plan_node_resume_input() {}
`;
}

function buildGoodReviewGateResolutionPayload() {
  return `// fixture
pub(crate) enum ResolutionOutcome { PerformTransition, KeepArtifact, RequestChanges }
pub(crate) fn stamp_resolution_payload() {}
pub(crate) fn stamp_needs_changes_next_step() {}
pub(crate) fn resolution_wire_string() {}
`;
}

function buildGoodReviewGateResolutionSubscriber() {
  return `// fixture
pub(crate) enum ReviewResolvedDispatch { IgnoreNonReviewId }
pub(crate) fn parse_subscriber_resolution_string() {}
pub(crate) fn plan_review_resolved_dispatch() {}
`;
}

function buildGoodReviewGateAutoAnswer() {
  return `// fixture
pub(crate) enum AutoAnswerPolicy { Off, DeterministicSafe, DryRun }
pub(crate) fn parse_auto_answer_policy() {}
pub(crate) fn auto_answer_policy_was_explicit() -> bool { false }
pub(crate) fn evaluate_auto_answer_policy() {}
pub(crate) fn stamp_auto_answer_payload() {}
pub(crate) fn is_destructive_review_action() {}
// NEVER auto-reject
`;
}

function buildGoodReviewGateLlmApproval() {
  return `// fixture
mod apply_gate;
mod proposal;
pub(crate) use apply_gate::{LlmApproveApplyStatus};
pub(crate) use proposal::{LlmAutoApproveProposalMode};
// never auto-approve
`;
}

function buildGoodReviewGateLlmProposal() {
  return `// fixture
pub(crate) enum LlmAutoApproveProposalMode { Off, SonnetSuggest }
pub(crate) fn parse_llm_auto_approve_proposal_mode() {}
pub(crate) fn parse_llm_auto_approve_proposal() {}
pub(crate) fn enforce_proposal_invariants() {}
pub(crate) fn build_llm_auto_approve_proposal_system_prompt() {}
pub(crate) fn stamp_llm_auto_approve_proposal_payload() {}
// never auto-approve
`;
}

function buildGoodReviewGateLlmApplyGate() {
  return `// fixture
mod evaluate;
mod hash;
mod input;
mod outcome;
mod payload;
mod preflight;
pub(crate) use self::evaluate::*;
pub(crate) use self::hash::*;
pub(crate) use self::input::*;
pub(crate) use self::outcome::*;
pub(crate) use self::payload::*;
pub(crate) use self::preflight::*;
// never auto-approve
`;
}

function buildGoodReviewGateLlmApplyGateInput() {
  return `// fixture
pub(crate) const APPLY_GATE_MISSING_PROPOSAL_HASH: &str = "APPLY_GATE_MISSING_PROPOSAL_HASH";
pub(crate) const APPLY_GATE_PROPOSAL_HASH_MISMATCH: &str = "APPLY_GATE_PROPOSAL_HASH_MISMATCH";
pub(crate) const APPLY_GATE_INVALID_PARAM: &str = "APPLY_GATE_INVALID_PARAM";
pub(crate) enum LlmApproveApplyStatus { NotRequested, Applied }
pub(crate) fn parse_llm_approve_apply_gate_input() {}
pub(crate) enum ProposalHashStatus { Matches }
pub(crate) struct LlmApproveApplyGateInput;
`;
}

function buildGoodReviewGateLlmApplyGateHash() {
  return `// fixture
pub(crate) fn compute_proposal_hash() {}
`;
}

function buildGoodReviewGateLlmApplyGateOutcome() {
  return `// fixture
pub(crate) struct LlmApproveApplyGateOutcome;
impl LlmApproveApplyGateOutcome {
    pub(crate) fn to_response_json() {}
}
`;
}

function buildGoodReviewGateLlmApplyGateEvaluate() {
  return `// fixture
pub(crate) fn evaluate_llm_approve_apply_gate() {}
fn guard() { let _ = is_destructive_review_action; }
// never auto-reject
`;
}

function buildGoodReviewGateLlmApplyGatePreflight() {
  return `// fixture
pub(crate) fn enforce_apply_gate_preflight() {}
const CODES: &[&str] = &[APPLY_GATE_MISSING_PROPOSAL_HASH, APPLY_GATE_PROPOSAL_HASH_MISMATCH];
`;
}

function buildGoodReviewGateLlmApplyGatePayload() {
  return `// fixture
pub(crate) fn stamp_llm_approve_apply_gate_payload() {}
pub(crate) fn stamp_proposal_hash_payload() {}
`;
}

function buildGoodReviewGateTests() {
  return `// fixture
use super::*;
fn smoke_wave22_07_review_apply_gate_rejects_missing_hash_accepts_fixture_hash() {}
fn smoke_wave22_07_review_apply_gate_pins_wave21_06_five_invariants() {}
fn proposal_invariants_round_trip_never_surface_rejected() {}
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

function buildGoodDirectiveFacadeRs() {
  return `// fixture
use crate::handlers::knowledge::review_gate::maybe_emit_review_question_resolved;

fn directive_facade() {
    maybe_emit_review_question_resolved();
}
`;
}

function buildGoodDirectiveApprovalReviewRs() {
  return `// fixture
mod approve;
mod archive;
mod proposer;
mod subscriber;
pub(super) use self::approve::action_approve;
pub(super) use self::archive::action_archive;
use self::proposer::{
    attach_directive_apply_gate_block, attach_directive_proposal_block,
    directive_proposer_summary, parse_proposer_mode_or_error,
    request_directive_auto_approve_proposal,
};
pub(crate) use self::subscriber::{handle_review_resolved_event, DirectiveSubscriberOutcome};
`;
}

function buildGoodDirectiveApprovalApproveRs() {
  return `// fixture
use super::*;
pub(in crate::handlers::knowledge::directive) async fn action_approve() {
    parse_review_resolution_input(args);
    maybe_emit_review_question_resolved();
    directive_approve;
}
async fn action_approve_with_resolution() {
    parse_review_resolution_input(args);
    maybe_emit_review_question_resolved();
}
async fn action_approve_with_policy_only() {}
`;
}

function buildGoodDirectiveApprovalArchiveRs() {
  return `// fixture
use super::*;
pub(in crate::handlers::knowledge::directive) async fn action_archive() {
    parse_review_resolution_input(args);
    maybe_emit_review_question_resolved();
    DirectiveStatus::Archived;
}
async fn action_archive_with_resolution() {
    parse_review_resolution_input(args);
    maybe_emit_review_question_resolved();
}
async fn action_archive_with_policy_only() {}
`;
}

function buildGoodDirectiveApprovalProposerRs() {
  return `// fixture
use super::*;
const DIRECTIVE_REVIEW_PROPOSER_CALLER: &str = "directive_review_proposer";
const SONNET_PROPOSER_MAX_TOKENS: u32 = 1024;
pub(super) async fn request_directive_auto_approve_proposal() {}
pub(super) fn attach_directive_proposal_block() {}
pub(super) fn attach_directive_apply_gate_block() {}
pub(super) fn parse_proposer_mode_or_error() {}
pub(super) fn directive_proposer_summary() {}
`;
}

function buildGoodDirectiveApprovalSubscriberRs() {
  return `// fixture
use super::*;
pub(crate) enum DirectiveSubscriberOutcome {}
pub(crate) async fn handle_review_resolved_event() {
    validate_review_resolution_envelope;
    DIRECTIVE_REVIEW_ACTIONS;
    DirectiveStatus::Archived;
}
`;
}

function buildGoodPlanFacadeRs() {
  return `// fixture
mod approval_review;
use approval_review::{action_approve, action_mark, action_supersede};
pub(crate) use approval_review::{handle_review_resolved_event, PlanSubscriberOutcome};
`;
}

function buildGoodPlanApprovalReviewRs() {
  return `// fixture
mod approve;
mod mark;
mod proposer;
mod subscriber;
mod supersede;
pub(super) use self::approve::action_approve;
pub(super) use self::mark::action_mark;
use self::proposer::{request_plan_auto_approve_proposal};
pub(crate) use self::subscriber::{handle_review_resolved_event, PlanSubscriberOutcome};
pub(super) use self::supersede::action_supersede;
fn caller(args: &serde_json::Value) {
    request_plan_auto_approve_proposal();
}
`;
}

function buildGoodPlanApprovalApproveRs() {
  return `// fixture
use super::*;
pub(super) async fn action_approve() {
    parse_review_resolution_input(args);
    maybe_emit_review_question_resolved();
    PlanStatus::Approved;
}
async fn action_approve_with_resolution() {}
async fn plan_action_approve_with_policy_only() {}
`;
}

function buildGoodPlanApprovalMarkRs() {
  return `// fixture
use super::*;
pub(super) async fn action_mark() {
    parse_review_resolution_input(args);
    maybe_emit_review_question_resolved();
    target_raw;
    PlanStatus::Approved;
}
async fn action_mark_with_resolution() {}
async fn plan_action_mark_with_policy_only() {}
`;
}

function buildGoodPlanApprovalSupersedeRs() {
  return `// fixture
use super::*;
pub(in crate::handlers::knowledge::plan) async fn action_supersede() {
    parse_review_resolution_input(args);
    maybe_emit_review_question_resolved();
    PlanStatus::Superseded;
    destructive;
}
async fn action_supersede_with_resolution() {}
async fn plan_action_supersede_with_policy_only() {}
`;
}

function buildGoodPlanApprovalProposerRs() {
  return `// fixture
use super::*;
const PLAN_REVIEW_PROPOSER_CALLER: &str = "plan_review_proposer";
const SONNET_PLAN_PROPOSER_MAX_TOKENS: u32 = 1024;
pub(super) fn build_plan_automation_ctx() {}
pub(super) async fn request_plan_auto_approve_proposal() {}
pub(super) fn attach_plan_proposal_block() {}
pub(super) fn attach_plan_apply_gate_block() {}
`;
}

function buildGoodPlanApprovalSubscriberRs() {
  return `// fixture
use super::*;
pub(crate) enum PlanSubscriberOutcome {}
pub(crate) async fn handle_review_resolved_event() {
    validate_review_resolution_envelope;
    PLAN_REVIEW_ACTIONS;
    PlanStatus::Approved;
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
