import { NextRequest, NextResponse } from 'next/server';
import { callTool } from '@/lib/missiond';

export async function GET(req: NextRequest) {
  try {
    const query = req.nextUrl.searchParams.get('query');
    const category = req.nextUrl.searchParams.get('category') || undefined;
    const project = req.nextUrl.searchParams.get('project') || undefined;

    // Hybrid search via mission_kb_query
    if (query) {
      const limit = req.nextUrl.searchParams.get('limit') || '20';
      const args: Record<string, unknown> = { action: 'search', query, limit: Number(limit) };
      if (category) args.category = category;
      if (project) args.project = project;
      const result = await callTool('mission_kb_query', args);
      return NextResponse.json(result);
    }

    // List all entries
    const args: Record<string, unknown> = {};
    if (category) args.category = category;
    if (project) args.project = project;
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
