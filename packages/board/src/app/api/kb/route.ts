import { NextRequest, NextResponse } from 'next/server';
import { callTool } from '@/lib/missiond';

export async function GET(req: NextRequest) {
  try {
    const category = req.nextUrl.searchParams.get('category') || undefined;
    const args: Record<string, unknown> = {};
    if (category) args.category = category;
    const entries = await callTool('mission_kb_list', args);
    return NextResponse.json(entries);
  } catch (err) {
    return NextResponse.json({ error: String(err) }, { status: 502 });
  }
}

export async function DELETE(req: NextRequest) {
  try {
    // Prefer `key` (matches daemon contract), keep legacy `id` as fallback.
    const key = req.nextUrl.searchParams.get('key') || req.nextUrl.searchParams.get('id');
    if (!key) return NextResponse.json({ error: 'Missing key' }, { status: 400 });
    const result = await callTool('mission_kb_forget', { key });
    return NextResponse.json(result);
  } catch (err) {
    return NextResponse.json({ error: String(err) }, { status: 502 });
  }
}
