import { NextRequest, NextResponse } from 'next/server';
import { callTool } from '@/lib/missiond';

export const runtime = 'nodejs';
export const dynamic = 'force-dynamic';
export const revalidate = 0;

export async function POST(req: NextRequest) {
  try {
    const body = await req.json().catch(() => ({}));
    const slotId = typeof body.slotId === 'string' ? body.slotId.trim() : '';
    const key = typeof body.key === 'string' ? body.key.trim().toLowerCase() : '';
    const text = typeof body.text === 'string' ? body.text : '';

    if (!slotId) return NextResponse.json({ error: 'Missing slotId' }, { status: 400 });

    if (key) {
      const result = await callTool('mission_pty_key', { slotId, key });
      const byteLength =
        result && typeof result === 'object' && 'byteLength' in result
          ? Number((result as { byteLength?: unknown }).byteLength)
          : undefined;
      return NextResponse.json({ ok: true, slotId, key, bytes: byteLength, result });
    }

    const data = text;
    if (!data) return NextResponse.json({ error: 'Missing text or key' }, { status: 400 });

    const result = await callTool('mission_pty_text', { slotId, text: data });
    const byteLength =
      result && typeof result === 'object' && 'byteLength' in result
        ? Number((result as { byteLength?: unknown }).byteLength)
        : data.length;
    return NextResponse.json({ ok: true, slotId, key: null, bytes: byteLength, result });
  } catch (err) {
    return NextResponse.json({ error: String(err) }, { status: 502 });
  }
}
