import { NextRequest, NextResponse } from 'next/server';
import { callTool } from '@/lib/missiond';

export async function GET(req: NextRequest) {
  try {
    const sessionId = req.nextUrl.searchParams.get('sessionId');
    const search = req.nextUrl.searchParams.get('search');

    // Get user message index (lightweight, for minimap sidebar)
    if (sessionId && req.nextUrl.searchParams.get('userIndex') === '1') {
      const result = await callTool('mission_user_message_index', { sessionId });
      return NextResponse.json(result);
    }

    // Get messages for a specific conversation
    if (sessionId) {
      const tail = req.nextUrl.searchParams.get('tail') || '200';
      const sinceId = req.nextUrl.searchParams.get('sinceId');
      const includeLabels = req.nextUrl.searchParams.get('labels') === '1';
      // Fetch messages and events in parallel
      const [msgResult, eventsResult] = await Promise.all([
        callTool('mission_conversation_get', {
          sessionId,
          tail: Number(tail),
          includeRaw: true,
          ...(sinceId != null && { sinceId: Number(sinceId) }),
          ...(includeLabels && { includeLabels: true }),
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
      const conversationType = req.nextUrl.searchParams.get('conversationType') || undefined;
      const args: Record<string, unknown> = { query: search, limit: Number(limit) };
      if (conversationType && conversationType !== 'all') args.conversationType = conversationType;
      const result = await callTool('mission_conversation_search', args);
      return NextResponse.json(result);
    }

    // List conversations — server-side filtering by conversationType + source
    const status = req.nextUrl.searchParams.get('status') || undefined;
    const limit = req.nextUrl.searchParams.get('limit') || '50';
    const conversationType = req.nextUrl.searchParams.get('conversationType') || undefined;
    const source = req.nextUrl.searchParams.get('source') || undefined;
    const args: Record<string, unknown> = { limit: Number(limit) };
    if (status) args.status = status;
    if (conversationType) args.conversationType = conversationType;
    if (source) args.source = source;
    const conversations = await callTool('mission_conversation_list', args);
    return NextResponse.json(conversations);
  } catch (err) {
    return NextResponse.json({ error: String(err) }, { status: 502 });
  }
}
