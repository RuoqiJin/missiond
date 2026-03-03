import { NextRequest, NextResponse } from 'next/server';
import { callTool } from '@/lib/missiond';

export async function GET(req: NextRequest) {
  try {
    const hoursStr = req.nextUrl.searchParams.get('hours');
    const hours = hoursStr ? parseInt(hoursStr, 10) : 24;
    const result = await callTool('mission_decision_stats', { hours });
    return NextResponse.json(result);
  } catch (err) {
    return NextResponse.json({ error: String(err) }, { status: 502 });
  }
}
