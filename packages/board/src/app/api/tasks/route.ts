import { NextRequest, NextResponse } from 'next/server';
import { BOARD_TASK_FIELD_MAP } from '@/generated/board-frontend-config';
import { boardClient } from '@/lib/missiondBoardClient';

function mapToFrontend(task: Record<string, unknown>): Record<string, unknown> {
  const out = { ...task };
  for (const { frontend, backend, defaultValue } of BOARD_TASK_FIELD_MAP) {
    out[frontend] = out[backend] ?? defaultValue;
    delete out[backend];
  }
  return out;
}

function mapToBackend(data: Record<string, unknown>): Record<string, unknown> {
  const out = { ...data };
  for (const { frontend, backend } of BOARD_TASK_FIELD_MAP) {
    if (out[frontend] !== undefined) {
      out[backend] = out[frontend];
      delete out[frontend];
    }
  }
  return out;
}

function errorText(err: unknown): string {
  return err instanceof Error ? err.message : String(err);
}

function evidenceErrorResponse(err: unknown) {
  const message = errorText(err);
  if (!message.includes('EVIDENCE_REQUIRED')) return null;
  const taskId = message.match(/task_id=([^\s:]+)/)?.[1]
    ?? message.match(/BoardTask\s+([^\s]+)/)?.[1]
    ?? '';
  const body = {
    code: 'EVIDENCE_REQUIRED',
    taskId,
    message: taskId
      ? `Task ${taskId} cannot be marked done until canonical completion evidence exists.`
      : 'Task cannot be marked done until canonical completion evidence exists.',
    suggestedAction: taskId
      ? `mission_shared_memory(action="task_result_put", task_id="${taskId}", result_status="completed", ...)`
      : 'mission_shared_memory(action="task_result_put", task_id=..., result_status="completed", ...)',
    error: message,
  };
  return NextResponse.json(body, { status: 409 });
}

function taskApiError(err: unknown) {
  return evidenceErrorResponse(err)
    ?? NextResponse.json({ error: errorText(err) }, { status: 502 });
}

export async function GET(req: NextRequest) {
  try {
    const id = req.nextUrl.searchParams.get('id');
    if (id) {
      // Get single task with notes
      const task = await boardClient.get(id);
      if (!task) return NextResponse.json({ error: 'Not found' }, { status: 404 });
      const mapped = task as Record<string, unknown>;
      const { notes, ...rest } = mapped;
      return NextResponse.json({ ...mapToFrontend(rest), notes });
    }
    const status = req.nextUrl.searchParams.get('status') || undefined;
    const args: Record<string, unknown> = { includeHidden: true };
    if (status) args.status = status;
    const tasks = await boardClient.list(args) as Record<string, unknown>[];
    return NextResponse.json(tasks.map(mapToFrontend));
  } catch (err) {
    return taskApiError(err);
  }
}

export async function POST(req: NextRequest) {
  try {
    const action = req.nextUrl.searchParams.get('action');
    const id = req.nextUrl.searchParams.get('id');

    if (action === 'toggle' && id) {
      const result = await boardClient.toggle(id);
      return NextResponse.json(mapToFrontend(result as Record<string, unknown>));
    }

    if (action === 'add-note' && id) {
      const body = await req.json();
      const note = await boardClient.addNote(id, { content: body.content, noteType: body.noteType, author: body.author });
      return NextResponse.json(note);
    }

    if (action === 'clear-done') {
      const allTasks = await boardClient.list({ includeHidden: true }) as Record<string, unknown>[];
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
        await boardClient.delete(task.id as string);
      }
      return NextResponse.json({ deleted: safeTasks.length, skipped: doneTasks.length - safeTasks.length });
    }

    const body = await req.json();
    const backendData = mapToBackend(body);
    const task = await boardClient.create(backendData);
    return NextResponse.json(mapToFrontend(task as Record<string, unknown>));
  } catch (err) {
    return taskApiError(err);
  }
}

export async function PATCH(req: NextRequest) {
  try {
    const id = req.nextUrl.searchParams.get('id');
    if (!id) return NextResponse.json({ error: 'Missing id' }, { status: 400 });
    const body = await req.json();
    const backendData = mapToBackend(body);
    const task = await boardClient.update(id, backendData);
    return NextResponse.json(mapToFrontend(task as Record<string, unknown>));
  } catch (err) {
    return taskApiError(err);
  }
}

export async function DELETE(req: NextRequest) {
  try {
    const id = req.nextUrl.searchParams.get('id');
    if (!id) return NextResponse.json({ error: 'Missing id' }, { status: 400 });
    const result = await boardClient.delete(id);
    return NextResponse.json(result);
  } catch (err) {
    return taskApiError(err);
  }
}
