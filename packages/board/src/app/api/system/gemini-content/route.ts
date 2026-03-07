import { NextRequest, NextResponse } from 'next/server';
import { callTool } from '@/lib/missiond';

export async function GET(req: NextRequest) {
  try {
    const requestId = req.nextUrl.searchParams.get('request_id');
    if (!requestId) {
      return NextResponse.json({ error: 'missing request_id' }, { status: 400 });
    }

    const result = await callTool('mission_gemini_content', { request_id: requestId });
    return NextResponse.json(result);
  } catch (err) {
    return NextResponse.json({ error: String(err) }, { status: 502 });
  }
}
