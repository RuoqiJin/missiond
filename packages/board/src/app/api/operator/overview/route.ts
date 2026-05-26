import { NextResponse } from 'next/server';
import { callTool } from '@/lib/missiond';

type AnyRecord = Record<string, unknown>;
type OverviewErrorSource = 'tasks' | 'slots' | 'slotStatus' | 'questions' | 'health';
type OverviewError = {
  source: OverviewErrorSource;
  slotId?: string;
  message: string;
};

const RUNNING_SLOT_STATES = new Set(['running', 'thinking', 'responding', 'tool_running', 'confirming', 'blocked', 'starting']);

function str(value: unknown): string {
  return typeof value === 'string' ? value : '';
}

function statusOf(task: AnyRecord): string {
  return str(task.status).toLowerCase();
}

function isBlockedTask(task: AnyRecord): boolean {
  const status = statusOf(task);
  return status === 'blocked' || Boolean(task.blockedReason ?? task.blocked_reason ?? task.blockedBy ?? task.blocked_by);
}

function taskSummary(task: AnyRecord) {
  return {
    id: str(task.id),
    title: str(task.title),
    status: str(task.status),
    priority: task.priority ?? null,
    assignee: task.assignee ?? task.claimExecutorId ?? task.claim_executor_id ?? null,
    claimExecutorId: task.claimExecutorId ?? task.claim_executor_id ?? null,
    blockedReason: task.blockedReason ?? task.blocked_reason ?? null,
    updatedAt: task.updatedAt ?? task.updated_at ?? null,
  };
}

function slotSummary(slot: AnyRecord, status: AnyRecord | null) {
  const recognition = (status?.recognition as AnyRecord | undefined) ?? {};
  const state = str(recognition.state ?? status?.state).toLowerCase();
  return {
    id: str(slot.id),
    role: slot.role ?? null,
    provider: recognition.provider ?? status?.provider ?? slot.provider ?? null,
    engine: status?.engine ?? slot.engine ?? null,
    state: state || null,
    running: RUNNING_SLOT_STATES.has(state),
    activeTool: recognition.activeTool ?? recognition.active_tool ?? status?.activeTool ?? status?.active_tool ?? null,
    blockedKind: recognition.blockedKind ?? recognition.blocked_kind ?? status?.blockedKind ?? status?.blocked_kind ?? null,
    activeBoardTaskId: status?.activeBoardTaskId ?? status?.active_board_task_id ?? status?.currentTaskId ?? status?.current_task_id ?? null,
  };
}

function evidenceSummary(tasks: AnyRecord[]) {
  const done = tasks.filter((task) => statusOf(task) === 'done');
  const verified = done.filter((task) => task.verified === true || task.taskRunVerifierStatus === 'pass' || task.task_run_verifier_status === 'pass');
  return {
    completed: done.length,
    verified: verified.length,
    missing: Math.max(0, done.length - verified.length),
  };
}

function errorMessage(value: unknown): string {
  return value instanceof Error ? value.message : String(value);
}

function collectError<T>(
  errors: OverviewError[],
  source: OverviewErrorSource,
  result: PromiseSettledResult<T>,
) {
  if (result.status === 'rejected') {
    errors.push({ source, message: errorMessage(result.reason) });
  }
}

export async function GET() {
  try {
    const errors: OverviewError[] = [];
    const [tasksResult, slotsResult, questionsResult, healthResult] = await Promise.allSettled([
      callTool('mission_board_list', { includeHidden: true }) as Promise<AnyRecord[]>,
      callTool('mission_slots') as Promise<AnyRecord[]>,
      callTool('mission_question_list', { status: 'pending', limit: 20 }) as Promise<AnyRecord[]>,
      callTool('mission_health') as Promise<AnyRecord>,
    ]);

    collectError(errors, 'tasks', tasksResult);
    collectError(errors, 'slots', slotsResult);
    collectError(errors, 'questions', questionsResult);
    collectError(errors, 'health', healthResult);

    const tasks = tasksResult.status === 'fulfilled' && Array.isArray(tasksResult.value) ? tasksResult.value : [];
    const rawSlots = slotsResult.status === 'fulfilled' && Array.isArray(slotsResult.value) ? slotsResult.value : [];
    const pendingQuestions = questionsResult.status === 'fulfilled' && Array.isArray(questionsResult.value) ? questionsResult.value : [];
    const health = healthResult.status === 'fulfilled' ? healthResult.value : null;

    const slotStatuses = await Promise.allSettled(
      rawSlots.map((slot) =>
        callTool('mission_pty_status', { slotId: slot.id }) as Promise<AnyRecord>,
      ),
    );

    const slots = rawSlots.map((slot, index) => {
      const settled = slotStatuses[index];
      if (settled?.status === 'rejected') {
        errors.push({
          source: 'slotStatus',
          slotId: str(slot.id),
          message: errorMessage(settled.reason),
        });
      }
      const status = settled?.status === 'fulfilled' ? settled.value : null;
      return slotSummary(slot, status);
    });

    const openTasks = tasks.filter((task) => statusOf(task) === 'open');
    const runningTasks = tasks.filter((task) => statusOf(task) === 'running');
    const blockedTasks = tasks.filter(isBlockedTask);
    const runningSlots = slots.filter((slot) => slot.running);

    return NextResponse.json({
      partial: errors.length > 0,
      errors,
      tasks: {
        open: openTasks.length,
        running: runningTasks.length,
        blocked: blockedTasks.length,
        total: tasks.length,
        runningItems: runningTasks.slice(0, 6).map(taskSummary),
      },
      slots: {
        total: slots.length,
        running: runningSlots.length,
        blocked: slots.filter((slot) => slot.blockedKind).length,
        items: slots.slice(0, 12),
      },
      blockers: {
        pendingQuestions: pendingQuestions.length,
        questions: pendingQuestions.slice(0, 5),
        tasks: blockedTasks.slice(0, 5).map(taskSummary),
      },
      evidence: evidenceSummary(tasks),
      eventHealth: {
        backend: health,
      },
      generatedAt: new Date().toISOString(),
    });
  } catch (err) {
    return NextResponse.json({ error: String(err) }, { status: 502 });
  }
}
