import { NextResponse } from 'next/server';
import { callTool } from '@/lib/missiond';

export const runtime = 'nodejs';
export const dynamic = 'force-dynamic';
export const revalidate = 0;

export async function GET(request: Request) {
  try {
    const { searchParams } = new URL(request.url);
    const result = await callTool('mission_codex_replay', {
      action: 'list_runs',
      campaignId: searchParams.get('campaignId') || undefined,
      limit: Number(searchParams.get('limit') || 50),
    });
    return NextResponse.json(result);
  } catch (err) {
    return NextResponse.json({ error: String(err) }, { status: 502 });
  }
}
