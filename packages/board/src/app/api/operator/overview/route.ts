import { NextResponse } from 'next/server';
import { callTool } from '@/lib/missiond';
import { buildOperatorOverview } from '@/lib/operatorOverview';

export async function GET() {
  try {
    const overview = await buildOperatorOverview(callTool);
    return NextResponse.json(overview);
  } catch (err) {
    return NextResponse.json({ error: String(err) }, { status: 502 });
  }
}
