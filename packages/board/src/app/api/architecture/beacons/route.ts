import { NextResponse } from 'next/server';
import { callTool } from '@/lib/missiond';

export async function GET() {
  try {
    const result = await callTool('mission_beacon_list') as { name: string }[];
    return NextResponse.json(Array.isArray(result) ? result : []);
  } catch (err) {
    return NextResponse.json({ error: String(err) }, { status: 502 });
  }
}
