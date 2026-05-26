import { NextResponse } from 'next/server';
import { callTool } from '@/lib/missiond';

const ALLOWED_TOOLS = new Set([
  'mission_worker',
  'mission_event_bus',
  'mission_shared_memory',
  'mission_health',
]);

function record(value: unknown): Record<string, unknown> {
  return value && typeof value === 'object' && !Array.isArray(value) ? value as Record<string, unknown> : {};
}

export async function POST(req: Request) {
  try {
    const body = record(await req.json());
    const actionId = typeof body.actionId === 'string' ? body.actionId : '';
    const args = record(body.args);
    const tool = typeof args.tool === 'string' ? args.tool : typeof body.tool === 'string' ? body.tool : undefined;
    const mcpTool = tool ?? (typeof args.action === 'string' ? String(body.tool ?? '') : '');
    const targetTool = typeof body.tool === 'string' ? body.tool : typeof args.tool === 'string' ? args.tool : undefined;
    const inferredTool = targetTool
      ?? (actionId.startsWith('worker_') ? 'mission_worker' : undefined)
      ?? (actionId.startsWith('dlq_') || actionId === 'event_bus_health' ? 'mission_event_bus' : undefined)
      ?? (actionId === 'open_evidence' ? 'mission_shared_memory' : undefined)
      ?? (actionId === 'startup_health' ? 'mission_health' : undefined);
    const finalTool = inferredTool ?? mcpTool;
    if (!actionId || !finalTool || !ALLOWED_TOOLS.has(finalTool)) {
      return NextResponse.json({ ok: false, error: 'runbook action is not allowed' }, { status: 400 });
    }
    const finalArgs = { ...args };
    delete finalArgs.tool;
    if (actionId === 'dlq_replay' || finalArgs.action === 'dlq_replay') {
      if (body.confirm !== true) {
        return NextResponse.json({ ok: false, error: 'dlq_replay requires confirm=true' }, { status: 400 });
      }
      finalArgs.confirm = true;
    }
    const result = await callTool(finalTool, finalArgs);
    return NextResponse.json({ ok: true, result });
  } catch (err) {
    return NextResponse.json({ ok: false, error: err instanceof Error ? err.message : String(err) }, { status: 502 });
  }
}
