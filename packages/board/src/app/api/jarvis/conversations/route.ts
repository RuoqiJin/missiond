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
        includeMessages: true,
      }) as { messages?: ConversationMessage[] };
      return NextResponse.json({
        messages: result?.messages || [],
      });
    } catch (err) {
      return NextResponse.json({ error: String(err) }, { status: 502 });
    }
  }

  // List recent jarvis conversations
  try {
    const result = await callTool('mission_conversation_list', {
      source: 'jarvis_ui',
      limit: 50,
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

// POST — save a conversation exchange (user + assistant messages)
export async function POST(req: NextRequest) {
  try {
    const body = await req.json();
    const { conversationId, userMessage, assistantMessage } = body;

    // Use router_chat append or create a new conversation record
    // For now, we store via conversation_messages through the conversation tool
    if (!userMessage && !assistantMessage) {
      return NextResponse.json({ error: 'No messages to save' }, { status: 400 });
    }

    // Create or get conversation
    let convId = conversationId;
    if (!convId) {
      // Generate a conversation ID
      convId = `jarvis-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`;
    }

    // For now, we'll rely on the PTY session's built-in conversation tracking
    // The /v1/chat/completions endpoint already logs to JarvisTraceStore
    // Future: implement direct DB persistence for jarvis_ui conversations

    return NextResponse.json({ conversationId: convId, ok: true });
  } catch (err) {
    return NextResponse.json({ error: String(err) }, { status: 500 });
  }
}
