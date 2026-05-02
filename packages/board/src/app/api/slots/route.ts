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
  task_class?: string;
  taskClass?: string;
  accepts_boardtask?: boolean;
  acceptsBoardTask?: boolean;
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
  latest_conversation?: {
    id?: string;
    source?: string;
    title?: string;
    updated_at?: string;
    updatedAt?: string;
  } | null;
  latestConversation?: {
    id?: string;
    source?: string;
    title?: string;
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

function labelForSlot(slot: SlotInfo) {
  return slot.description || slot.id.replace(/^slot-/, '').replace(/-\d+$/, '');
}

function latestConversation(status: PtyStatus | null) {
  const latest = status?.latestConversation ?? status?.latest_conversation ?? null;
  if (!latest) return null;
  return {
    id: latest.id,
    source: latest.source,
    title: latest.title,
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
      const state = recognition?.state ?? status?.state;
      const running = status?.running ?? (!!state && state !== 'exited' && state !== 'not_running');
      return {
        id: s.id,
        role: s.role,
        label: labelForSlot(s),
        running,
        state,
        provider: recognition?.provider ?? status?.provider ?? s.provider,
        engine: status?.engine ?? s.engine,
        modelProfile: s.modelProfile ?? s.model_profile,
        taskClass: s.taskClass ?? s.task_class,
        acceptsBoardTask: s.acceptsBoardTask ?? s.accepts_boardtask,
        confidence: recognition?.confidence ?? status?.confidence,
        reason: recognition?.reason ?? status?.reason,
        activeTool: recognition?.activeTool ?? recognition?.active_tool ?? status?.activeTool ?? status?.active_tool,
        blockedKind: recognition?.blockedKind ?? recognition?.blocked_kind ?? status?.blockedKind ?? status?.blocked_kind,
        latestConversation: latestConversation(status),
      };
    });

    // Running slots first
    slots.sort((a, b) => (a.running === b.running ? 0 : a.running ? -1 : 1));
    return NextResponse.json(slots);
  } catch (err) {
    return NextResponse.json({ error: String(err) }, { status: 502 });
  }
}
