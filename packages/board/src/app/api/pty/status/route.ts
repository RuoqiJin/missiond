import { NextRequest, NextResponse } from 'next/server';
import { callTool } from '@/lib/missiond';

const RUNNING_STATES = new Set([
  'running',
  'thinking',
  'responding',
  'tool_running',
  'confirming',
  'blocked',
  'starting',
  'idle',
  'slash_menu',
]);

function normalizeState(state: unknown) {
  return typeof state === 'string' && state.trim()
    ? state.trim().toLowerCase()
    : null;
}

function normalizeLiveState(state: string | null, result: Record<string, unknown> | null) {
  if (state === 'unknown' && result?.pid != null) return 'starting';
  return state;
}

export async function GET(req: NextRequest) {
  try {
    const slotId = req.nextUrl.searchParams.get('slotId');
    if (!slotId) return NextResponse.json({ error: 'Missing slotId' }, { status: 400 });
    const result = await callTool('mission_pty_status', { slotId }) as Record<string, unknown> | null;
    const recognition = result?.recognition as Record<string, unknown> | undefined;
    const state = normalizeLiveState(normalizeState(recognition?.state ?? result?.state), result);
    if (result && state) {
      return NextResponse.json({ running: RUNNING_STATES.has(state), ...result, state });
    }
    return NextResponse.json({ running: false, slotId, state });
  } catch (err) {
    return NextResponse.json({ error: String(err) }, { status: 502 });
  }
}
