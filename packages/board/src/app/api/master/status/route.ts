import { NextResponse } from 'next/server';
import { callTool } from '@/lib/missiond';

export const runtime = 'nodejs';
export const dynamic = 'force-dynamic';
export const revalidate = 0;

export async function GET() {
  try {
    const status = await callTool('mission_master_status');
    return NextResponse.json(status);
  } catch (err) {
    return NextResponse.json({ error: String(err) }, { status: 502 });
  }
}
