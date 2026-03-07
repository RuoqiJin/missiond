import { NextRequest, NextResponse } from 'next/server';
import { callTool } from '@/lib/missiond';

export async function GET(req: NextRequest) {
  try {
    const window = req.nextUrl.searchParams.get('window') || '24h';
    const limitStr = req.nextUrl.searchParams.get('limit');
    const limit = limitStr ? parseInt(limitStr, 10) : 50;
    const traces = await callTool('mission_timeline_traces', { window, limit });
    return NextResponse.json({ traces });
  } catch (err) {
    return NextResponse.json({ error: String(err) }, { status: 502 });
  }
}
