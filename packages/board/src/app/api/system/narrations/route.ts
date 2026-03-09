import { NextRequest, NextResponse } from 'next/server';
import { callTool } from '@/lib/missiond';

export async function GET(req: NextRequest) {
  try {
    const sessionId = req.nextUrl.searchParams.get('session_id');
    if (!sessionId) {
      return NextResponse.json({ error: 'missing session_id' }, { status: 400 });
    }

    const result = await callTool('mission_session_narrations', { session_id: sessionId });
    return NextResponse.json(result);
  } catch (err) {
    return NextResponse.json({ error: String(err) }, { status: 502 });
  }
}
