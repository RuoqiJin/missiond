import { NextRequest, NextResponse } from 'next/server';
import { callTool } from '@/lib/missiond';

export async function GET(req: NextRequest) {
  try {
    const messageId = req.nextUrl.searchParams.get('message_id');
    if (!messageId) {
      return NextResponse.json({ error: 'missing message_id' }, { status: 400 });
    }

    const result = await callTool('mission_conversation_message', { message_id: parseInt(messageId, 10) });
    return NextResponse.json(result);
  } catch (err) {
    return NextResponse.json({ error: String(err) }, { status: 502 });
  }
}
