import { NextRequest, NextResponse } from 'next/server';
import { callTool } from '@/lib/missiond';

export async function POST(req: NextRequest) {
  try {
    const { paused } = await req.json();
    const result = await callTool('mission_memory_pause', { paused });
    return NextResponse.json(result);
  } catch (err) {
    return NextResponse.json({ error: String(err) }, { status: 502 });
  }
}
