import { NextRequest, NextResponse } from 'next/server';
import { callTool } from '@/lib/missiond';

export async function GET(req: NextRequest) {
  try {
    const sessionId = req.nextUrl.searchParams.get('sessionId');
    const search = req.nextUrl.searchParams.get('search');

    // Get messages for a specific conversation (userIndex embedded in response)
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
          includeUserIndex: true,
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

export async function POST(req: NextRequest) {
  try {
    const body = await req.json();
    const { action, sessionId, label, value } = body;

    if (action === 'set_label' && sessionId && label) {
      const result = await callTool('mission_conversation_set_label', { sessionId, label, value: value ?? '1' });
      return NextResponse.json(result);
    }
    if (action === 'delete_label' && sessionId && label) {
      const result = await callTool('mission_conversation_delete_label', { sessionId, label });
      return NextResponse.json(result);
    }
    return NextResponse.json({ error: 'Unknown action' }, { status: 400 });
  } catch (err) {
    return NextResponse.json({ error: String(err) }, { status: 502 });
  }
}
