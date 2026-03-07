import { NextRequest, NextResponse } from 'next/server';
import { callTool } from '@/lib/missiond';

export async function GET(req: NextRequest) {
  try {
    const eventType = req.nextUrl.searchParams.get('eventType') || undefined;
    const traceId = req.nextUrl.searchParams.get('traceId') || undefined;
    const window = req.nextUrl.searchParams.get('window') || '24h';
    const query = req.nextUrl.searchParams.get('query') || undefined;
    const limitStr = req.nextUrl.searchParams.get('limit');
    const limit = limitStr ? parseInt(limitStr, 10) : 200;

    // Use search if query provided, otherwise use query tool
    if (query) {
      const results = await callTool('mission_timeline_search', { query, limit });
      return NextResponse.json({ events: results });
    }

    const args: Record<string, unknown> = { window, limit };
    if (eventType) args.eventType = eventType;
    if (traceId) args.traceId = traceId;

    const events = await callTool('mission_timeline_query', args);
    return NextResponse.json({ events });
  } catch (err) {
    return NextResponse.json({ error: String(err) }, { status: 502 });
  }
}
