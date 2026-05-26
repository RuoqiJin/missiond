type AnyRecord = Record<string, unknown>;

export type OverviewErrorSource = 'tasks' | 'slots' | 'slotStatus' | 'questions' | 'health' | 'workers';
export type OverviewError = {
  source: OverviewErrorSource;
  slotId?: string;
  message: string;
};

export type OperatorRunbookItem = {
  severity: 'info' | 'warn' | 'bad';
  title: string;
  cause: string;
  nextAction: string;
  source: string;
  command?: string;
  action?: {
    id: string;
    label: string;
    kind: 'refresh' | 'mcp' | 'navigate';
    tool?: string;
    args?: Record<string, unknown>;
    requiresConfirm?: boolean;
  };
};

export type OperatorOverviewCallTool = (
  name: string,
  args?: Record<string, unknown>,
) => Promise<unknown>;

const RUNNING_SLOT_STATES = new Set(['running', 'thinking', 'responding', 'tool_running', 'confirming', 'blocked', 'starting']);

function str(value: unknown): string {
  return typeof value === 'string' ? value : '';
}

function num(value: unknown): number {
  return typeof value === 'number' && Number.isFinite(value) ? value : 0;
}

function record(value: unknown): AnyRecord {
  return value && typeof value === 'object' && !Array.isArray(value) ? value as AnyRecord : {};
}

function arrayOfRecords(value: unknown): AnyRecord[] {
  return Array.isArray(value) ? value.filter((item): item is AnyRecord => Boolean(item) && typeof item === 'object' && !Array.isArray(item)) : [];
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
  const recognition = record(status?.recognition);
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

function workerSummary(worker: AnyRecord) {
  const health = record(worker.health);
  return {
    name: str(worker.name ?? health.name),
    state: worker.state ?? null,
    lifecycle: str(health.lifecycle ?? worker.lifecycle ?? worker.state).toLowerCase() || 'unknown',
    paused: Boolean(health.effectivePaused ?? health.effective_paused ?? worker.paused),
    stale: Boolean(health.stale),
    staleReason: health.staleReason ?? health.stale_reason ?? null,
    heartbeatAgeSecs: num(health.heartbeatAgeSecs ?? health.heartbeat_age_secs),
    currentTaskId: health.currentTaskId ?? health.current_task_id ?? null,
    currentSlotId: health.currentSlotId ?? health.current_slot_id ?? null,
    lastError: health.lastError ?? health.last_error ?? null,
    status: health.status ?? null,
    tasksProcessed: num(health.tasksProcessed ?? worker.tasksProcessed ?? worker.tasks_processed),
    tasksFailed: num(health.tasksFailed ?? worker.tasksFailed ?? worker.tasks_failed),
  };
}

function evidenceSummary(tasks: AnyRecord[], health: AnyRecord | null) {
  const healthEvidence = record(health?.evidence);
  const items = arrayOfRecords(healthEvidence.items);
  const done = tasks.filter((task) => statusOf(task) === 'done');
  const verified = done.filter((task) => task.verified === true || task.taskRunVerifierStatus === 'pass' || task.task_run_verifier_status === 'pass');
  const completed = Math.max(num(healthEvidence.completed), done.length);
  const missing = Math.max(num(healthEvidence.missing), Math.max(0, done.length - verified.length));
  return {
    completed,
    verified: Math.max(num(healthEvidence.tasksWithEvidence), verified.length),
    missing,
    degraded: Boolean(healthEvidence.degraded),
    items: items.slice(0, 8),
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

function buildRunbook(input: {
  errors: OverviewError[];
  workers: ReturnType<typeof workerSummary>[];
  health: AnyRecord | null;
  eventBus: AnyRecord;
  evidence: ReturnType<typeof evidenceSummary>;
  pendingQuestions: number;
}) {
  const items: OperatorRunbookItem[] = [];
  for (const error of input.errors) {
    items.push({
      severity: error.source === 'slotStatus' ? 'warn' : 'bad',
      title: `Overview source degraded: ${error.source}`,
      cause: error.slotId ? `${error.slotId}: ${error.message}` : error.message,
      nextAction: 'Check the MCP tool response and daemon logs for this source.',
      source: error.source,
      action: {
        id: 'refresh_overview',
        label: 'Refresh',
        kind: 'refresh',
      },
    });
  }

  for (const worker of input.workers) {
    if (worker.stale || worker.lifecycle === 'failed') {
      items.push({
        severity: worker.lifecycle === 'failed' ? 'bad' : 'warn',
        title: `Worker ${worker.name || 'unknown'} needs attention`,
        cause: String(worker.lastError ?? worker.staleReason ?? worker.lifecycle),
        nextAction: 'Inspect mission_worker list and the worker-specific daemon logs before restarting the worker.',
        source: 'worker',
        command: 'mission_worker(action="list")',
        action: {
          id: `worker_resume:${worker.name || 'unknown'}`,
          label: 'Resume',
          kind: 'mcp',
          tool: 'mission_worker',
          args: { action: 'control', target: worker.name, control_action: 'resume' },
          requiresConfirm: worker.lifecycle === 'failed',
        },
      });
    } else if (worker.lifecycle === 'blocked') {
      items.push({
        severity: 'warn',
        title: `Worker ${worker.name || 'unknown'} is blocked`,
        cause: String(worker.status ?? 'blocked'),
        nextAction: 'Resolve the referenced blocker or pending operator question.',
        source: 'worker',
        action: {
          id: `worker_list:${worker.name || 'unknown'}`,
          label: 'Inspect',
          kind: 'mcp',
          tool: 'mission_worker',
          args: { action: 'list' },
        },
      });
    }
  }

  const dlqCount = num(record(input.eventBus.dlq).count);
  if (dlqCount > 0) {
    items.push({
      severity: 'warn',
      title: 'EventBus DLQ has entries',
      cause: `${dlqCount} event(s) are in dead_letter_queue.`,
      nextAction: 'Inspect the affected subscription and replay or acknowledge the failed event.',
      source: 'eventBus',
      action: {
        id: 'dlq_list',
        label: 'Inspect DLQ',
        kind: 'mcp',
        tool: 'mission_event_bus',
        args: { action: 'dlq_list', limit: 20 },
      },
    });
  }

  if (num(input.eventBus.dispatchLag) > 100 || num(input.eventBus.lagged) > 0) {
    items.push({
      severity: 'warn',
      title: 'EventBus lag detected',
      cause: `dispatchLag=${num(input.eventBus.dispatchLag)}, lagged=${num(input.eventBus.lagged)}`,
      nextAction: 'Check slow subscriptions and dispatcher logs before scaling worker consumers.',
      source: 'eventBus',
      action: {
        id: 'event_bus_health',
        label: 'Inspect',
        kind: 'mcp',
        tool: 'mission_event_bus',
        args: { action: 'health' },
      },
    });
  }

  if (input.evidence.missing > 0 || input.evidence.degraded) {
    items.push({
      severity: 'warn',
      title: 'Execution evidence is incomplete',
      cause: `${input.evidence.missing} completed task(s) lack verified task-result artifacts.`,
      nextAction: 'Open the evidence items and attach or regenerate canonical task result artifacts.',
      source: 'evidence',
      action: {
        id: 'open_evidence',
        label: 'Open Evidence',
        kind: 'mcp',
        tool: 'mission_shared_memory',
        args: { action: 'task_evidence_summary', limit: 20 },
      },
    });
  }

  if (input.pendingQuestions > 0) {
    items.push({
      severity: 'info',
      title: 'Pending operator questions',
      cause: `${input.pendingQuestions} question(s) are waiting for input.`,
      nextAction: 'Answer or route the pending questions to unblock workers.',
      source: 'questions',
      action: {
        id: 'open_questions',
        label: 'Open Questions',
        kind: 'navigate',
        args: { href: '#questions' },
      },
    });
  }

  const startup = record(input.health?.startupPreflight);
  const checks = arrayOfRecords(startup.checks);
  for (const check of checks.filter((item) => ['warning', 'warn', 'fatal', 'error'].includes(str(item.status).toLowerCase())).slice(0, 4)) {
    items.push({
      severity: ['fatal', 'error'].includes(str(check.status).toLowerCase()) ? 'bad' : 'warn',
      title: `Startup preflight: ${str(check.name) || 'check'}`,
      cause: str(check.message ?? check.detail ?? check.status),
      nextAction: 'Fix the reported startup dependency, then restart the daemon if the check is fatal.',
      source: 'startupPreflight',
      action: {
        id: 'startup_health',
        label: 'Inspect Health',
        kind: 'mcp',
        tool: 'mission_health',
        args: {},
      },
    });
  }

  return items.slice(0, 10);
}

export async function buildOperatorOverview(callTool: OperatorOverviewCallTool) {
  const errors: OverviewError[] = [];
  const [tasksResult, slotsResult, questionsResult, healthResult, workersResult] = await Promise.allSettled([
    callTool('mission_board_list', { includeHidden: true }) as Promise<unknown>,
    callTool('mission_slots') as Promise<unknown>,
    callTool('mission_question_list', { status: 'pending', limit: 20 }) as Promise<unknown>,
    callTool('mission_health') as Promise<unknown>,
    callTool('mission_worker', { action: 'list' }) as Promise<unknown>,
  ]);

  collectError(errors, 'tasks', tasksResult);
  collectError(errors, 'slots', slotsResult);
  collectError(errors, 'questions', questionsResult);
  collectError(errors, 'health', healthResult);
  collectError(errors, 'workers', workersResult);

  const tasks = tasksResult.status === 'fulfilled' ? arrayOfRecords(tasksResult.value) : [];
  const rawSlots = slotsResult.status === 'fulfilled' ? arrayOfRecords(slotsResult.value) : [];
  const pendingQuestions = questionsResult.status === 'fulfilled' ? arrayOfRecords(questionsResult.value) : [];
  const health = healthResult.status === 'fulfilled' ? record(healthResult.value) : null;
  const workerPayload = workersResult.status === 'fulfilled' ? record(workersResult.value) : {};
  const workers = arrayOfRecords(workerPayload.workers ?? health?.workers).map(workerSummary);

  const slotStatuses = await Promise.allSettled(
    rawSlots.map((slot) =>
      callTool('mission_pty_status', { slotId: slot.id }) as Promise<unknown>,
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
    const status = settled?.status === 'fulfilled' ? record(settled.value) : null;
    return slotSummary(slot, status);
  });

  const openTasks = tasks.filter((task) => statusOf(task) === 'open');
  const runningTasks = tasks.filter((task) => statusOf(task) === 'running');
  const blockedTasks = tasks.filter(isBlockedTask);
  const runningSlots = slots.filter((slot) => slot.running);
  const eventBus = record(health?.eventBus ?? health?.event_bus);
  const evidence = evidenceSummary(tasks, health);
  const trends = record(health?.operatorTrends ?? health?.operator_trends);

  return {
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
    workers: {
      total: workers.length,
      running: workers.filter((worker) => worker.lifecycle === 'running').length,
      blocked: workers.filter((worker) => worker.lifecycle === 'blocked').length,
      stale: workers.filter((worker) => worker.stale).length,
      failed: workers.filter((worker) => worker.lifecycle === 'failed').length,
      items: workers.slice(0, 12),
    },
    blockers: {
      pendingQuestions: pendingQuestions.length,
      questions: pendingQuestions.slice(0, 5),
      tasks: blockedTasks.slice(0, 5).map(taskSummary),
    },
    evidence,
    eventBus,
    trends,
    eventHealth: {
      backend: health,
    },
    runbook: buildRunbook({
      errors,
      workers,
      health,
      eventBus,
      evidence,
      pendingQuestions: pendingQuestions.length,
    }),
    generatedAt: new Date().toISOString(),
  };
}

export function createFakeOperatorOverviewHarness(fixtures: Record<string, unknown>): OperatorOverviewCallTool {
  return async (name, args) => {
    if (name === 'mission_pty_status') {
      const slotId = typeof args?.slotId === 'string' ? args.slotId : '';
      const statuses = record(fixtures.mission_pty_status);
      if (slotId in statuses) {
        const value = statuses[slotId];
        const maybeError = record(value).__error;
        if (typeof maybeError === 'string') throw new Error(maybeError);
        return value;
      }
    }
    if (name in fixtures) return fixtures[name];
    throw new Error(`fake MCP fixture missing: ${name}`);
  };
}
