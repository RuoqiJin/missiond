import { NextRequest, NextResponse } from 'next/server';
import { callTool } from '@/lib/missiond';

interface ConversationListItem {
  id: string;
  project: string | null;
  slotId: string | null;
  source: string;
  model: string | null;
  messageCount: number;
  startedAt: string;
  status: string;
  conversationType: string;
  sessionTimeline: string | null;
}

interface ConversationMessage {
  id: number;
  sessionId: string;
  role: string;
  content: string;
  timestamp: string;
}

// GET — list conversations or get specific conversation messages
export async function GET(req: NextRequest) {
  const id = req.nextUrl.searchParams.get('id');

  if (id) {
    // Get messages for a specific conversation
    try {
      const result = await callTool('mission_conversation_get', {
        sessionId: id,
        tail: 200,
      }) as { messages?: ConversationMessage[] };
      return NextResponse.json({
        messages: result?.messages || [],
      });
    } catch (err) {
      return NextResponse.json({ error: String(err) }, { status: 502 });
    }
  }

  // List recent jarvis conversations (filtered by source=jarvis_ui)
  try {
    const result = await callTool('mission_conversation_list', {
      source: 'jarvis_ui',
      limit: 50,
      conversationType: 'all',
    }) as ConversationListItem[];

    const convs = (result || []).map((c) => ({
      id: c.id,
      title: c.sessionTimeline || `Chat ${c.startedAt?.slice(0, 16) || ''}`,
      updatedAt: c.startedAt || '',
      messageCount: c.messageCount || 0,
    }));
    return NextResponse.json(convs);
  } catch {
    // Fallback: return empty list if tool not available
    return NextResponse.json([]);
  }
}
