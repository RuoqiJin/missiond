import { NextRequest, NextResponse } from 'next/server';
import { callTool } from '@/lib/missiond';

export async function POST(req: NextRequest) {
  try {
    const slotId = req.nextUrl.searchParams.get('slotId');
    const decision = req.nextUrl.searchParams.get('decision');
    const option = req.nextUrl.searchParams.get('option');

    if (!slotId) return NextResponse.json({ error: 'Missing slotId' }, { status: 400 });

    // Map decision/option to mission_pty_confirm semantic response format.
    // The runtime PTY layer performs Up/Down + Enter navigation; the frontend
    // must not write numeric shortcut keys directly into Codex/Claude/Gemini.
    let response: boolean | number | string;
    if (option) {
      // Numeric option selection (1, 2, 3, ...)
      response = parseInt(option, 10);
    } else if (decision === 'allow') {
      response = true;
    } else if (decision === 'deny') {
      response = false;
    } else {
      return NextResponse.json({ error: 'Missing decision or option' }, { status: 400 });
    }

    await callTool('mission_pty_confirm', { slotId, response });
    return NextResponse.json({ ok: true });
  } catch (err) {
    return NextResponse.json({ error: String(err) }, { status: 502 });
  }
}
