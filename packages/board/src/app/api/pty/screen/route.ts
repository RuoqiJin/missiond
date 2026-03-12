import { NextRequest, NextResponse } from 'next/server';
import { callMissiond } from '@/lib/missiond';

export async function GET(req: NextRequest) {
  try {
    const slotId = req.nextUrl.searchParams.get('slotId');
    if (!slotId) return NextResponse.json({ error: 'Missing slotId' }, { status: 400 });
    const lines = Number(req.nextUrl.searchParams.get('lines')) || 50;

    // Use callMissiond directly — pty_screen returns ToolResult::text() (not JSON),
    // so callTool's JSON.parse would fail on raw terminal text.
    const result = await callMissiond('tools/call', {
      name: 'mission_pty_screen',
      arguments: { slotId, lines },
    }) as { content?: Array<{ text?: string }> };

    const screen = result?.content?.[0]?.text || '';
    return NextResponse.json({ screen, slotId });
  } catch (err) {
    return NextResponse.json({ error: String(err) }, { status: 502 });
  }
}
