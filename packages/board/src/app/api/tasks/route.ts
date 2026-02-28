import { NextRequest, NextResponse } from 'next/server';
import { callTool } from '@/lib/missiond';

function mapToFrontend(task: Record<string, unknown>): Record<string, unknown> {
  const { orderIdx, ...rest } = task;
  return { ...rest, order: orderIdx ?? 0 };
}

function mapToBackend(data: Record<string, unknown>): Record<string, unknown> {
  const { order, ...rest } = data;
  if (order !== undefined) rest.orderIdx = order;
  return rest;
}

export async function GET(req: NextRequest) {
  try {
    const status = req.nextUrl.searchParams.get('status') || undefined;
    const args: Record<string, unknown> = { includeHidden: true };
    if (status) args.status = status;
    const tasks = await callTool('mission_board_list', args) as Record<string, unknown>[];
    return NextResponse.json(tasks.map(mapToFrontend));
  } catch (err) {
    return NextResponse.json({ error: String(err) }, { status: 502 });
  }
}

export async function POST(req: NextRequest) {
  try {
    const action = req.nextUrl.searchParams.get('action');
    const id = req.nextUrl.searchParams.get('id');

    if (action === 'toggle' && id) {
      const result = await callTool('mission_board_toggle', { id });
      return NextResponse.json(mapToFrontend(result as Record<string, unknown>));
    }

    if (action === 'clear-done') {
      const allTasks = await callTool('mission_board_list', { includeHidden: true }) as Record<string, unknown>[];
      const doneTasks = allTasks.filter(t => t.status === 'done');

      // Only delete done tasks whose entire subtree is also done/skipped
      const hasActiveDescendant = (taskId: string): boolean => {
        const children = allTasks.filter(t => t.parentId === taskId);
        return children.some(c => {
          const s = c.status as string;
          if (s !== 'done' && s !== 'skipped') return true;
          return hasActiveDescendant(c.id as string);
        });
      };

      const safeTasks = doneTasks.filter(t => !hasActiveDescendant(t.id as string));
      for (const task of safeTasks) {
        await callTool('mission_board_delete', { id: task.id });
      }
      return NextResponse.json({ deleted: safeTasks.length, skipped: doneTasks.length - safeTasks.length });
    }

    const body = await req.json();
    const backendData = mapToBackend(body);
    const task = await callTool('mission_board_create', backendData);
    return NextResponse.json(mapToFrontend(task as Record<string, unknown>));
  } catch (err) {
    return NextResponse.json({ error: String(err) }, { status: 502 });
  }
}

export async function PATCH(req: NextRequest) {
  try {
    const id = req.nextUrl.searchParams.get('id');
    if (!id) return NextResponse.json({ error: 'Missing id' }, { status: 400 });
    const body = await req.json();
    const backendData = mapToBackend(body);
    const task = await callTool('mission_board_update', { id, ...backendData });
    return NextResponse.json(mapToFrontend(task as Record<string, unknown>));
  } catch (err) {
    return NextResponse.json({ error: String(err) }, { status: 502 });
  }
}

export async function DELETE(req: NextRequest) {
  try {
    const id = req.nextUrl.searchParams.get('id');
    if (!id) return NextResponse.json({ error: 'Missing id' }, { status: 400 });
    const result = await callTool('mission_board_delete', { id });
    return NextResponse.json(result);
  } catch (err) {
    return NextResponse.json({ error: String(err) }, { status: 502 });
  }
}
