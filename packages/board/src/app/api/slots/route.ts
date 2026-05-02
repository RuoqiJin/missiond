import { NextResponse } from 'next/server';
import { callTool } from '@/lib/missiond';

interface SlotInfo {
  id: string;
  role: string;
  description?: string;
  provider?: string;
  engine?: string;
  model_profile?: string;
  modelProfile?: string;
  reasoning_effort?: string;
  reasoningEffort?: string;
  search_enabled?: boolean;
  searchEnabled?: boolean;
  sandbox?: string;
  approval_policy?: string;
  approvalPolicy?: string;
  task_class?: string;
  taskClass?: string;
  accepts_boardtask?: boolean;
  acceptsBoardTask?: boolean;
  mcpReady?: boolean;
  mcpEnabled?: boolean;
  mcpApprovalReady?: boolean;
  mcpApprovalMissingTools?: string[];
  mcpSource?: string;
  providerConversationId?: string;
  latest_conversation?: PtyStatus['latest_conversation'];
  latestConversation?: PtyStatus['latestConversation'];
}

interface PtyStatus {
  state?: string;
  running?: boolean;
  provider?: string;
  engine?: string;
  confidence?: number;
  reason?: string;
  active_tool?: string;
  activeTool?: string;
  blocked_kind?: string;
  blockedKind?: string;
  current_task_id?: string;
  currentTaskId?: string;
  active_board_task_id?: string;
  activeBoardTaskId?: string;
  latest_conversation?: {
    id?: string;
    source?: string;
    title?: string;
    messageCount?: number;
    status?: string;
    updated_at?: string;
    updatedAt?: string;
  } | null;
  latestConversation?: {
    id?: string;
    source?: string;
    title?: string;
    messageCount?: number;
    status?: string;
    updated_at?: string;
    updatedAt?: string;
  } | null;
  recognition?: {
    state?: string;
    confidence?: number;
    reason?: string;
    provider?: string;
    active_tool?: string;
    activeTool?: string;
    blocked_kind?: string;
    blockedKind?: string;
  };
}

// State buckets — keep aligned with PtyCanonicalState (running/idle/blocked/complete/unknown)
// and SessionState (starting/idle/slash_menu/thinking/responding/tool_running/confirming/error/exited).
// `running` is reserved for states where a worker is actively producing output or waiting on a
// confirmation it owns. `idle` is an active-but-quiescent session and must NOT count as running.
const RUNNING_STATES = new Set([
  'running',
  'thinking',
  'responding',
  'tool_running',
  'confirming',
  'blocked',
  'starting',
]);

const INACTIVE_STATES = new Set([
  'complete',
  'completed',
  'exited',
  'not_running',
  'stopped',
  'dead',
  'missing',
  'error',
]);

function normalizeState(state?: string | null) {
  return typeof state === 'string' && state.trim()
    ? state.trim().toLowerCase()
    : null;
}

function labelForSlot(slot: SlotInfo) {
  return slot.description || slot.id.replace(/^slot-/, '').replace(/-\d+$/, '');
}

function latestConversation(status: PtyStatus | null, slot: SlotInfo) {
  const latest = status?.latestConversation
    ?? status?.latest_conversation
    ?? slot.latestConversation
    ?? slot.latest_conversation
    ?? null;
  if (!latest) return null;
  return {
    id: latest.id,
    source: latest.source,
    title: latest.title,
    messageCount: latest.messageCount,
    status: latest.status,
    updatedAt: latest.updatedAt ?? latest.updated_at,
  };
}

export async function GET() {
  try {
    const result = await callTool('mission_slots') as SlotInfo[];
    const filtered = result || [];

    // Check PTY status for all slots in parallel
    const statuses = await Promise.allSettled(
      filtered.map((s) =>
        callTool('mission_pty_status', { slotId: s.id })
          .then((r) => r as PtyStatus)
          .catch(() => null)
      )
    );

    const slots = filtered.map((s, i) => {
      const status = statuses[i].status === 'fulfilled' ? (statuses[i] as PromiseFulfilledResult<PtyStatus | null>).value : null;
      const recognition = status?.recognition;
      const state = normalizeState(recognition?.state ?? status?.state);
      const running = !!state && RUNNING_STATES.has(state);
      const active = !!state && !INACTIVE_STATES.has(state);
      return {
        id: s.id,
        role: s.role,
        label: labelForSlot(s),
        running,
        state,
        provider: recognition?.provider ?? status?.provider ?? s.provider,
        engine: status?.engine ?? s.engine,
        modelProfile: s.modelProfile ?? s.model_profile,
        reasoningEffort: s.reasoningEffort ?? s.reasoning_effort,
        searchEnabled: s.searchEnabled ?? s.search_enabled,
        sandbox: s.sandbox,
        approvalPolicy: s.approvalPolicy ?? s.approval_policy,
        taskClass: s.taskClass ?? s.task_class,
        acceptsBoardTask: s.acceptsBoardTask ?? s.accepts_boardtask,
        mcpReady: s.mcpReady,
        mcpEnabled: s.mcpEnabled,
        mcpApprovalReady: s.mcpApprovalReady,
        mcpApprovalMissingTools: s.mcpApprovalMissingTools,
        mcpSource: s.mcpSource,
        providerConversationId: s.providerConversationId,
        confidence: recognition?.confidence ?? status?.confidence,
        reason: recognition?.reason ?? status?.reason,
        activeTool: recognition?.activeTool ?? recognition?.active_tool ?? status?.activeTool ?? status?.active_tool,
        blockedKind: recognition?.blockedKind ?? recognition?.blocked_kind ?? status?.blockedKind ?? status?.blocked_kind,
        currentTaskId: status?.currentTaskId ?? status?.current_task_id,
        activeBoardTaskId: status?.activeBoardTaskId ?? status?.active_board_task_id ?? status?.currentTaskId ?? status?.current_task_id,
        latestConversation: latestConversation(status, s),
        // Internal sort hint; consumers may ignore.
        __activeRank: active ? 1 : 2,
      };
    });

    // Running first, then active sessions (idle), then inactive (complete/exited/etc).
    slots.sort((a, b) => {
      const rank = (x: typeof a) => (x.running ? 0 : x.__activeRank);
      return rank(a) - rank(b);
    });
    // Drop the internal sort hint before serialising.
    const cleaned = slots.map(({ __activeRank: _drop, ...rest }) => rest);
    return NextResponse.json(cleaned);
  } catch (err) {
    return NextResponse.json({ error: String(err) }, { status: 502 });
  }
}
