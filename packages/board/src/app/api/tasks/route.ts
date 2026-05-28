import { NextRequest, NextResponse } from 'next/server';
import { BOARD_TASK_FIELD_MAP } from '@/generated/board-frontend-config';
import { boardClient } from '@/lib/missiondBoardClient';
import { MissiondError } from '@/lib/missiond';

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

function taskIdFromMissiondBody(body: Record<string, unknown>): string {
  if (typeof body.taskId === 'string') return body.taskId;
  if (typeof body.task_id === 'string') return body.task_id;
  const details = body.details;
  if (
    details
    && typeof details === 'object'
    && 'task_id' in details
    && typeof (details as { task_id?: unknown }).task_id === 'string'
  ) {
    return (details as { task_id: string }).task_id;
  }
  return '';
}

function structuredErrorStatus(code?: string): number {
  if (code === 'CAPABILITY_DENIED' || code === 'FEATURE_DISABLED') return 403;
  if (
    code === 'COMPLETION_ARTIFACT_INVALID'
    || code === 'SANDBOX_POLICY_UNSUPPORTED'
    || code === 'WRITE_SCOPE_VIOLATION'
  ) {
    return 422;
  }
  if (
    code === 'EVIDENCE_REQUIRED'
    || code === 'CLAIM_CONFLICT'
    || code === 'RUNTIME_METADATA_REQUIRED'
    || code === 'TASK_CONTRACT_REQUIRED'
  ) {
    return 409;
  }
  if (code === 'COMPLETION_ARTIFACT_WRITE_FAILED') return 502;
  return 502;
}

function structuredMissiondErrorResponse(err: unknown) {
  const missiondBody = err instanceof MissiondError ? err.body : null;
  if (!missiondBody) return null;
  const code = missiondBody?.code ?? missiondBody?.error_code;
  const message = missiondBody?.message ?? missiondBody?.reason ?? errorText(err);
  const suggestion = missiondBody?.suggestedAction ?? missiondBody?.suggestion;
  const taskId = taskIdFromMissiondBody(missiondBody);
  const responseBody: Record<string, unknown> = {
    ...missiondBody,
    code,
    message,
    suggestedAction: suggestion,
    error: message,
  };
  if (code === 'EVIDENCE_REQUIRED') {
    responseBody.taskId = taskId;
    responseBody.message = taskId
      ? `Task ${taskId} cannot be marked done until canonical completion evidence exists.`
      : 'Task cannot be marked done until canonical completion evidence exists.';
    responseBody.suggestedAction = taskId
      ? `mission_shared_memory(action="task_result_put", task_id="${taskId}", result_status="completed", ...)`
      : 'mission_shared_memory(action="task_result_put", task_id=..., result_status="completed", ...)';
    responseBody.error = responseBody.message;
  }
  return NextResponse.json(responseBody, { status: structuredErrorStatus(code) });
}

function taskApiError(err: unknown) {
  return structuredMissiondErrorResponse(err)
    ?? NextResponse.json(
      {
        code: 'BOARD_API_ERROR',
        message: errorText(err),
        error: errorText(err),
      },
      { status: 502 },
    );
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
