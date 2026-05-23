import { NextRequest, NextResponse } from 'next/server';
import { callTool } from '@/lib/missiond';

export async function GET(req: NextRequest) {
  try {
    const action = req.nextUrl.searchParams.get('action') || 'catalog';
    const intent = req.nextUrl.searchParams.get('intent') || undefined;
    const project = req.nextUrl.searchParams.get('project') || undefined;
    const entryId = req.nextUrl.searchParams.get('entryId') || req.nextUrl.searchParams.get('entry_id') || undefined;
    const surface = req.nextUrl.searchParams.get('surface') || undefined;
    const args = Object.fromEntries(
      Object.entries({ action, intent, project, entryId, surface })
        .filter(([, value]) => value !== undefined && value !== ''),
    );
    const result = action === 'guide'
      ? await callTool('mission_tool_directory', { ...args, action: 'guide' })
      : await callTool('mission_agent_navigation', args);
    return NextResponse.json(result);
  } catch (err) {
    return NextResponse.json({ error: String(err) }, { status: 502 });
  }
}

export async function POST(req: NextRequest) {
  try {
    const body = await req.json();
    const result = await callTool('mission_agent_navigation', {
      action: 'feedback',
      ...body,
    });
    return NextResponse.json(result);
  } catch (err) {
    return NextResponse.json({ error: String(err) }, { status: 502 });
  }
}
