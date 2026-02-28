import { NextRequest, NextResponse } from 'next/server';
import { callTool } from '@/lib/missiond';

export async function GET(req: NextRequest) {
  try {
    const status = req.nextUrl.searchParams.get('status') || undefined;
    const args: Record<string, unknown> = {};
    if (status) args.status = status;
    const questions = await callTool('mission_question_list', args);
    return NextResponse.json(questions);
  } catch (err) {
    return NextResponse.json({ error: String(err) }, { status: 502 });
  }
}

export async function POST(req: NextRequest) {
  try {
    const action = req.nextUrl.searchParams.get('action');
    const id = req.nextUrl.searchParams.get('id');

    if (action === 'answer' && id) {
      const { answer } = await req.json();
      const result = await callTool('mission_question_answer', { id, answer });
      return NextResponse.json(result);
    }

    if (action === 'dismiss' && id) {
      const result = await callTool('mission_question_dismiss', { id });
      return NextResponse.json(result);
    }

    // Default: create question
    const body = await req.json();
    const question = await callTool('mission_question_create', body);
    return NextResponse.json(question);
  } catch (err) {
    return NextResponse.json({ error: String(err) }, { status: 502 });
  }
}
