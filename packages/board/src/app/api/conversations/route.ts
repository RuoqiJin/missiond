import { NextRequest, NextResponse } from 'next/server';
import { callTool } from '@/lib/missiond';

export async function GET(req: NextRequest) {
  try {
    const sessionId = req.nextUrl.searchParams.get('sessionId');
    const search = req.nextUrl.searchParams.get('search');

    // Get messages for a specific conversation
    if (sessionId) {
      const tail = req.nextUrl.searchParams.get('tail') || '200';
      // Fetch messages and events in parallel
      const [msgResult, eventsResult] = await Promise.all([
        callTool('mission_conversation_get', {
          sessionId,
          tail: Number(tail),
          includeRaw: true,
        }),
        callTool('mission_conversation_events', {
          sessionId,
          limit: 500,
        }).catch(() => ({ events: [] })),
      ]);
      const result = { ...(msgResult as Record<string, unknown>), events: (eventsResult as Record<string, unknown>)?.events || [] };
      return NextResponse.json(result);
    }

    // Search messages
    if (search) {
      const limit = req.nextUrl.searchParams.get('limit') || '30';
      const result = await callTool('mission_conversation_search', {
        query: search,
        limit: Number(limit),
      });
      return NextResponse.json(result);
    }

    // List conversations (filtering by conversation_type is done client-side)
    const status = req.nextUrl.searchParams.get('status') || undefined;
    const limit = req.nextUrl.searchParams.get('limit') || '50';
    const args: Record<string, unknown> = { limit: Number(limit) };
    if (status) args.status = status;
    const conversations = await callTool('mission_conversation_list', args);
    return NextResponse.json(conversations);
  } catch (err) {
    return NextResponse.json({ error: String(err) }, { status: 502 });
  }
}
