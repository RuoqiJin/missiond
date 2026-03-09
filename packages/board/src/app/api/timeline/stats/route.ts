import { NextRequest, NextResponse } from 'next/server';
import { callTool } from '@/lib/missiond';

/** Convert window string to absolute ISO datetime */
function windowToSince(w: string): string {
  let ms = 3600_000;
  const minMatch = w.match(/^(\d+)min$/);
  if (minMatch) ms = parseInt(minMatch[1], 10) * 60_000;
  const hMatch = w.match(/^(\d+)h$/);
  if (hMatch) ms = parseInt(hMatch[1], 10) * 3600_000;
  const dMatch = w.match(/^(\d+)d$/);
  if (dMatch) ms = parseInt(dMatch[1], 10) * 86400_000;
  return new Date(Date.now() - ms).toISOString().replace('T', ' ').slice(0, 19);
}

export async function GET(req: NextRequest) {
  try {
    const directSince = req.nextUrl.searchParams.get('since');
    const windowStr = req.nextUrl.searchParams.get('window') || '24h';
    const since = directSince || windowToSince(windowStr);
    const stats = await callTool('mission_timeline_stats', { since });
    return NextResponse.json(stats);
  } catch (err) {
    return NextResponse.json({ error: String(err) }, { status: 502 });
  }
}
