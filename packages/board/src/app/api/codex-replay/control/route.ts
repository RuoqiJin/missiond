import { NextResponse } from 'next/server';
import { callTool } from '@/lib/missiond';

export const runtime = 'nodejs';
export const dynamic = 'force-dynamic';
export const revalidate = 0;

export async function POST(request: Request) {
  try {
    const body = await request.json().catch(() => ({}));
    const action = typeof body.action === 'string' ? body.action : '';
    if (!action) return NextResponse.json({ error: 'action is required' }, { status: 400 });
    const result = await callTool('mission_codex_replay', {
      action,
      campaignId: body.campaignId,
      projectRoot: body.projectRoot,
      maxCycles: body.maxCycles,
      intervalSeconds: body.intervalSeconds,
    });
    return NextResponse.json(result);
  } catch (err) {
    return NextResponse.json({ error: String(err) }, { status: 502 });
  }
}
