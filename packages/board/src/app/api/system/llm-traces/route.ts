import { NextRequest, NextResponse } from 'next/server';
import { callTool } from '@/lib/missiond';

export async function GET(req: NextRequest) {
  try {
    const caller = req.nextUrl.searchParams.get('caller') || undefined;
    const status = req.nextUrl.searchParams.get('status') || undefined;
    const limitStr = req.nextUrl.searchParams.get('limit');
    const limit = limitStr ? parseInt(limitStr, 10) : 30;

    const args: Record<string, unknown> = { limit };
    if (caller) args.caller = caller;
    if (status) args.status = status;

    const [traceRes, statsRes] = await Promise.allSettled([
      callTool('mission_gemini_trace', args),
      callTool('mission_gemini_stats'),
    ]);

    return NextResponse.json({
      traces: traceRes.status === 'fulfilled' ? traceRes.value : null,
      stats: statsRes.status === 'fulfilled' ? statsRes.value : null,
    });
  } catch (err) {
    return NextResponse.json({ error: String(err) }, { status: 502 });
  }
}
