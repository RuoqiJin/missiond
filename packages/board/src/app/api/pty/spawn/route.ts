import { NextRequest, NextResponse } from 'next/server';
import { callTool } from '@/lib/missiond';

export async function POST(req: NextRequest) {
  try {
    const slotId = req.nextUrl.searchParams.get('slotId');
    if (!slotId) return NextResponse.json({ error: 'Missing slotId' }, { status: 400 });
    const result = await callTool('mission_pty_spawn', { slotId });
    return NextResponse.json(result);
  } catch (err) {
    return NextResponse.json({ error: String(err) }, { status: 502 });
  }
}
