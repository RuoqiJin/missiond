import { NextResponse } from 'next/server';
import { callTool } from '@/lib/missiond';

export async function GET() {
  try {
    const result = await callTool('mission_health');
    return NextResponse.json(result);
  } catch (err) {
    return NextResponse.json({ error: String(err) }, { status: 502 });
  }
}
