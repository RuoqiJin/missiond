import { NextRequest, NextResponse } from 'next/server';
import { callTool } from '@/lib/missiond';

export async function GET(req: NextRequest) {
  try {
    const slotId = req.nextUrl.searchParams.get('slotId');
    if (!slotId) return NextResponse.json({ error: 'Missing slotId' }, { status: 400 });
    const result = await callTool('mission_pty_status', { slotId }) as Record<string, unknown> | null;
    if (result && result.state && result.state !== 'exited') {
      return NextResponse.json({ running: true, ...result });
    }
    return NextResponse.json({ running: false, slotId, state: result?.state || null });
  } catch (err) {
    return NextResponse.json({ error: String(err) }, { status: 502 });
  }
}
