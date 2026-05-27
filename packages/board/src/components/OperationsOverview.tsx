'use client';

import { useEffect, useMemo, useState } from 'react';
import { Activity, AlertTriangle, ClipboardCheck, GitBranch, Radio, RefreshCw, Server, Wifi } from 'lucide-react';
import { cn } from '@/lib/utils';
import { useEventHealth, useEventInvalidation } from '../hooks/useEventStream';

interface OverviewTask {
  id: string;
  title: string;
  status: string;
  assignee?: unknown;
  claimExecutorId?: unknown;
  blockedReason?: unknown;
}

interface OverviewSlot {
  id: string;
  role?: unknown;
  provider?: unknown;
  engine?: unknown;
  state?: string | null;
  running: boolean;
  activeBoardTaskId?: unknown;
  blockedKind?: unknown;
}

interface OperatorOverview {
  partial: boolean;
  errors: Array<{
    source: string;
    slotId?: string;
    message: string;
  }>;
  tasks: {
    open: number;
    running: number;
    blocked: number;
    total: number;
    runningItems: OverviewTask[];
  };
  slots: {
    total: number;
    running: number;
    blocked: number;
    items: OverviewSlot[];
  };
  workers: {
    total: number;
    running: number;
    blocked: number;
    stale: number;
    failed: number;
    items: Array<{
      name: string;
      lifecycle: string;
      stale: boolean;
      staleReason?: unknown;
      currentTaskId?: unknown;
      currentSlotId?: unknown;
      lastError?: unknown;
      status?: unknown;
      heartbeatAgeSecs: number;
      tasksFailed: number;
    }>;
  };
  blockers: {
    pendingQuestions: number;
    questions: Array<Record<string, unknown>>;
    tasks: OverviewTask[];
  };
  evidence: {
    completed: number;
    verified: number;
    missing: number;
    degraded: boolean;
    items: Array<Record<string, unknown>>;
  };
  workflowRuns: {
    running: number;
    blocked: number;
    failed: number;
    stale: number;
    oldestUpdatedAgeSecs: number;
    degraded: boolean;
    items: Array<Record<string, unknown>>;
  };
  eventBus: Record<string, unknown>;
  trends: {
    points?: Array<Record<string, unknown>>;
    windows?: Record<string, Record<string, unknown>>;
  };
  runbook: Array<{
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
  }>;
  generatedAt: string;
}

function compactId(value: unknown): string {
  const raw = typeof value === 'string' ? value : '';
  return raw.length > 10 ? raw.slice(0, 8) : raw;
}

function text(value: unknown, fallback = '—'): string {
  if (typeof value === 'string' && value.trim()) return value;
  if (typeof value === 'number') return String(value);
  return fallback;
}

function formatRelative(ts: number | null) {
  if (!ts) return '—';
  const diff = Math.max(0, Date.now() - ts);
  if (diff < 1000) return 'now';
  if (diff < 60_000) return `${Math.floor(diff / 1000)}s`;
  if (diff < 3_600_000) return `${Math.floor(diff / 60_000)}m`;
  return `${Math.floor(diff / 3_600_000)}h`;
}

function formatAgeMs(ageMs: number | null) {
  if (ageMs === null) return '—';
  if (ageMs < 1000) return 'now';
  if (ageMs < 60_000) return `${Math.floor(ageMs / 1000)}s`;
  if (ageMs < 3_600_000) return `${Math.floor(ageMs / 60_000)}m`;
  return `${Math.floor(ageMs / 3_600_000)}h`;
}

function num(value: unknown): number {
  return typeof value === 'number' && Number.isFinite(value) ? value : 0;
}

function Stat({ label, value, tone }: { label: string; value: number; tone?: 'warn' | 'good' }) {
  return (
    <div className="min-w-0 rounded-md border border-neutral-800 bg-neutral-950/70 px-3 py-2">
      <div className="text-[10px] uppercase tracking-wide text-neutral-600">{label}</div>
      <div className={cn('mt-1 font-mono text-lg leading-none text-neutral-100', tone === 'warn' && 'text-amber-300', tone === 'good' && 'text-emerald-300')}>
        {value}
      </div>
    </div>
  );
}

export function OperationsOverview() {
  const [overview, setOverview] = useState<OperatorOverview | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [actionError, setActionError] = useState<string | null>(null);
  const [actionVersion, setActionVersion] = useState(0);
  const eventHealth = useEventHealth();
  const taskVersion = useEventInvalidation('task');
  const slotVersion = useEventInvalidation('slot');
  const questionVersion = useEventInvalidation('question');
  const engineVersion = useEventInvalidation('engine');

  useEffect(() => {
    let cancelled = false;
    fetch('/api/operator/overview')
      .then((res) => {
        if (!res.ok) throw new Error(`HTTP ${res.status}`);
        return res.json();
      })
      .then((data: OperatorOverview) => {
        if (cancelled) return;
        setOverview(data);
        setError(null);
      })
      .catch((err) => {
        if (!cancelled) setError(err instanceof Error ? err.message : String(err));
      });
    return () => {
      cancelled = true;
    };
  }, [taskVersion, slotVersion, questionVersion, engineVersion, actionVersion]);

  const currentObjective = useMemo(() => {
    const task = overview?.tasks.runningItems[0];
    if (!task) return null;
    return `${compactId(task.id)} ${task.title}`;
  }, [overview]);

  const runningSlots = useMemo(() => overview?.slots.items.filter((slot) => slot.running).slice(0, 4) ?? [], [overview]);
  const activeWorkers = useMemo(() => overview?.workers.items.filter((worker) => ['running', 'blocked', 'retrying', 'failed'].includes(worker.lifecycle) || worker.stale).slice(0, 5) ?? [], [overview]);
  const blockedTasks = overview?.blockers.tasks ?? [];
  const runbook = overview?.runbook ?? [];
  const eventBus = overview?.eventBus ?? {};
  const trend24h = overview?.trends?.windows?.['24h'] ?? {};
  const eventDlq = num((eventBus.dlq as Record<string, unknown> | undefined)?.count);
  const eventLag = num(eventBus.dispatchLag);
  const lagMax24h = num(trend24h.eventDispatchLagMax);
  const evidenceGapMax24h = num(trend24h.evidenceMissingMax);
  const workflowBlockedMax24h = num(trend24h.workflowBlockedMax);
  const workflowRuns = overview?.workflowRuns;
  const eventTone = eventHealth.severity === 'good'
    ? 'text-emerald-300'
    : eventHealth.severity === 'warn'
      ? 'text-amber-300'
      : 'text-red-300';
  const degradedSummary = overview?.partial
    ? overview.errors.slice(0, 2).map((item) => `${item.source}${item.slotId ? `:${item.slotId}` : ''}`).join(', ')
    : null;
  const runRunbookAction = async (item: OperatorOverview['runbook'][number]) => {
    const action = item.action;
    if (!action) return;
    setActionError(null);
    if (action.kind === 'refresh') {
      setActionVersion((value) => value + 1);
      return;
    }
    if (action.kind === 'navigate') {
      const href = typeof action.args?.href === 'string' ? action.args.href : '#';
      window.location.hash = href.replace(/^#/, '');
      return;
    }
    if (action.requiresConfirm && !window.confirm(`Run ${action.label}?`)) return;
    try {
      const res = await fetch('/api/operator/runbook/action', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ actionId: action.id, tool: action.tool, args: action.args, confirm: Boolean(action.requiresConfirm) }),
      });
      const body = await res.json().catch(() => ({}));
      if (!res.ok || body?.ok === false) throw new Error(body?.error ?? `HTTP ${res.status}`);
      setActionVersion((value) => value + 1);
    } catch (err) {
      setActionError(err instanceof Error ? err.message : String(err));
    }
  };

  return (
    <section className="px-4 pb-2 pt-1 sm:px-8">
      <div className="grid gap-3 border-y border-neutral-900 bg-neutral-950/35 py-3 xl:grid-cols-[1.15fr_1fr_1fr_1fr]">
        <div className="min-w-0">
          <div className="mb-2 flex items-center gap-2 text-xs font-medium text-neutral-300">
            <Activity className="h-4 w-4 text-orange-400" />
            Operations Overview
            {overview?.partial ? (
              <span className="rounded-sm border border-amber-900/60 px-1.5 py-0.5 font-mono text-[10px] text-amber-300">
                degraded
              </span>
            ) : null}
          </div>
          <div className="grid grid-cols-2 gap-2 sm:grid-cols-4">
            <Stat label="Open" value={overview?.tasks.open ?? 0} />
            <Stat label="Running" value={overview?.tasks.running ?? 0} tone="good" />
            <Stat label="Blocked" value={(overview?.tasks.blocked ?? 0) + (overview?.blockers.pendingQuestions ?? 0)} tone="warn" />
            <Stat label="Evidence" value={overview?.evidence.verified ?? 0} tone={overview?.evidence.missing || overview?.evidence.degraded ? 'warn' : 'good'} />
          </div>
          <div className="mt-2 min-w-0 truncate text-xs text-neutral-500">
            <span className="text-neutral-400">Current:</span> {currentObjective ?? 'No running BoardTask'}
          </div>
          {degradedSummary ? (
            <div className="mt-1 min-w-0 truncate text-[11px] text-amber-300">
              Partial data: {degradedSummary}
            </div>
          ) : null}
        </div>

        <div className="min-w-0 rounded-md border border-neutral-800 bg-neutral-950/70 p-3">
          <div className="mb-2 flex items-center justify-between gap-2">
            <div className="flex items-center gap-2 text-xs font-medium text-neutral-300">
              <Server className="h-4 w-4 text-sky-400" />
              Workers
            </div>
            <span className="font-mono text-[11px] text-neutral-500">{overview?.workers.running ?? 0}/{overview?.workers.total ?? 0}</span>
          </div>
          <div className="space-y-1">
            {activeWorkers.length ? activeWorkers.map((worker) => (
              <div key={worker.name} className="grid min-w-0 grid-cols-[1fr_auto] gap-2 text-xs">
                <span className="min-w-0 truncate text-neutral-300">{worker.name}</span>
                <span className={cn('shrink-0 font-mono', worker.lifecycle === 'failed' || worker.stale ? 'text-red-300' : worker.lifecycle === 'blocked' ? 'text-amber-300' : 'text-emerald-300')}>
                  {worker.stale ? 'stale' : worker.lifecycle}
                </span>
                <span className="col-span-2 min-w-0 truncate text-[11px] text-neutral-600">
                  {text(worker.currentTaskId ?? worker.currentSlotId ?? worker.status, 'idle')}
                </span>
              </div>
            )) : runningSlots.length ? runningSlots.map((slot) => (
              <div key={slot.id} className="flex min-w-0 items-center justify-between gap-3 text-xs">
                <span className="min-w-0 truncate text-neutral-300">{slot.id}</span>
                <span className="shrink-0 font-mono text-emerald-300">{slot.state ?? 'running'}</span>
              </div>
            )) : <div className="text-xs text-neutral-600">No active workers</div>}
          </div>
        </div>

        <div className="min-w-0 rounded-md border border-neutral-800 bg-neutral-950/70 p-3">
          <div className="mb-2 flex items-center justify-between gap-2">
            <div className="flex items-center gap-2 text-xs font-medium text-neutral-300">
              <ClipboardCheck className="h-4 w-4 text-emerald-400" />
              Evidence
            </div>
            <span className={cn('font-mono text-[11px]', overview?.evidence.missing ? 'text-amber-300' : 'text-neutral-500')}>
              {overview?.evidence.missing ?? 0} gaps
            </span>
          </div>
          <div className="space-y-1 text-xs">
            {(overview?.evidence.items ?? []).slice(0, 4).map((item, index) => (
              <div key={`${text(item.taskId, 'task')}-${index}`} className="grid min-w-0 grid-cols-[1fr_auto] gap-2">
                <span className="min-w-0 truncate text-neutral-300">{text(item.taskId, 'task')}</span>
                <span className={cn('font-mono', item.complete ? 'text-emerald-300' : 'text-amber-300')}>{item.complete ? 'ok' : 'gap'}</span>
                <span className="col-span-2 min-w-0 truncate text-[11px] text-neutral-600">{text(item.summary ?? item.resultStatus)}</span>
              </div>
            ))}
            {overview && overview.evidence.items.length === 0 ? <div className="text-xs text-neutral-600">No evidence items</div> : null}
          </div>
        </div>

        <div className="grid gap-3 sm:grid-cols-2 xl:grid-cols-1">
          <div className="min-w-0 rounded-md border border-neutral-800 bg-neutral-950/70 p-3">
            <div className="mb-2 flex items-center justify-between gap-2">
              <div className="flex items-center gap-2 text-xs font-medium text-neutral-300">
                <GitBranch className="h-4 w-4 text-violet-300" />
                Workflow Runs
              </div>
              <span className={cn('font-mono text-[11px]', (workflowRuns?.blocked || workflowRuns?.failed || workflowRuns?.stale) ? 'text-amber-300' : 'text-neutral-500')}>
                {workflowRuns?.running ?? 0} running
              </span>
            </div>
            <div className="grid grid-cols-2 gap-x-3 gap-y-1 text-[11px] text-neutral-500">
              <span>blocked</span><span className={cn('text-right font-mono text-neutral-300', (workflowRuns?.blocked ?? 0) > 0 && 'text-amber-300')}>{workflowRuns?.blocked ?? 0}</span>
              <span>failed</span><span className={cn('text-right font-mono text-neutral-300', (workflowRuns?.failed ?? 0) > 0 && 'text-red-300')}>{workflowRuns?.failed ?? 0}</span>
              <span>stale</span><span className={cn('text-right font-mono text-neutral-300', (workflowRuns?.stale ?? 0) > 0 && 'text-amber-300')}>{workflowRuns?.stale ?? 0}</span>
              <span>oldest</span><span className="text-right font-mono text-neutral-300">{workflowRuns ? formatAgeMs(workflowRuns.oldestUpdatedAgeSecs * 1000) : '—'}</span>
              <span>24h blocked</span><span className={cn('text-right font-mono text-neutral-300', workflowBlockedMax24h > 0 && 'text-amber-300')}>{workflowBlockedMax24h}</span>
            </div>
            <div className="mt-2 space-y-1">
              {(workflowRuns?.items ?? []).slice(0, 2).map((item, index) => (
                <div key={`${text(item.id, 'workflow')}-${index}`} className="grid min-w-0 grid-cols-[1fr_auto] gap-2 text-[11px]">
                  <span className="min-w-0 truncate text-neutral-300">{text(item.workflow_id ?? item.id, 'workflow')}</span>
                  <span className={cn('font-mono', text(item.status) === 'failed' ? 'text-red-300' : text(item.status) === 'blocked' ? 'text-amber-300' : 'text-emerald-300')}>{text(item.status)}</span>
                  <span className="col-span-2 min-w-0 truncate text-neutral-600">{text(item.parent_task_id ?? item.checkpointExcerpt)}</span>
                </div>
              ))}
            </div>
          </div>

          <div className="min-w-0 rounded-md border border-neutral-800 bg-neutral-950/70 p-3">
            <div className="mb-2 flex items-center gap-2 text-xs font-medium text-neutral-300">
              <AlertTriangle className="h-4 w-4 text-amber-400" />
              Blockers
            </div>
            <div className="space-y-1 text-xs">
              <div className="flex justify-between gap-3 text-neutral-400"><span>Questions</span><span className="font-mono text-amber-300">{overview?.blockers.pendingQuestions ?? 0}</span></div>
              <div className="flex justify-between gap-3 text-neutral-400"><span>Tasks</span><span className="font-mono text-amber-300">{blockedTasks.length}</span></div>
            </div>
          </div>

          <div className="min-w-0 rounded-md border border-neutral-800 bg-neutral-950/70 p-3">
            <div className="mb-2 flex items-center justify-between gap-2">
              <div className="flex items-center gap-2 text-xs font-medium text-neutral-300">
                <Wifi className={cn('h-4 w-4', eventTone)} />
                Event Health
              </div>
              <span className={cn('font-mono text-[11px]', eventTone)}>{eventHealth.status}</span>
            </div>
            <div className="grid grid-cols-2 gap-x-3 gap-y-1 text-[11px] text-neutral-500">
              <span>seq</span><span className="truncate text-right font-mono text-neutral-300">{eventHealth.lastSeq}</span>
              <span>last</span><span className="text-right font-mono text-neutral-300">{formatRelative(eventHealth.lastMessageAt)}</span>
              <span>age</span><span className={cn('text-right font-mono text-neutral-300', eventHealth.isStale && 'text-amber-300')}>{formatAgeMs(eventHealth.ageMs)}</span>
              <span>reconnects</span><span className="text-right font-mono text-neutral-300">{eventHealth.reconnectAttempts}</span>
              <span>malformed</span><span className="text-right font-mono text-neutral-300">{eventHealth.malformedCount}</span>
              <span>bus lag</span><span className={cn('text-right font-mono text-neutral-300', eventLag > 100 && 'text-amber-300')}>{eventLag}</span>
              <span>24h max</span><span className={cn('text-right font-mono text-neutral-300', lagMax24h > 100 && 'text-amber-300')}>{lagMax24h}</span>
              <span>dlq</span><span className={cn('text-right font-mono text-neutral-300', eventDlq > 0 && 'text-amber-300')}>{eventDlq}</span>
              <span>evidence max</span><span className={cn('text-right font-mono text-neutral-300', evidenceGapMax24h > 0 && 'text-amber-300')}>{evidenceGapMax24h}</span>
              <span className="flex items-center gap-1"><RefreshCw className="h-3 w-3" /> resync</span><span className="truncate text-right font-mono text-neutral-300">{eventHealth.lastResyncReason ?? '—'}</span>
              <span>cursor lag</span><span className={cn('text-right font-mono text-neutral-300', (eventHealth.lastLagDiagnostic?.cursorLag ?? 0) > 100 && 'text-amber-300')}>{eventHealth.lastLagDiagnostic?.cursorLag ?? '—'}</span>
              <span>subscriber</span><span className="truncate text-right font-mono text-neutral-300">{eventHealth.lastLagDiagnostic?.subscriberId ?? '—'}</span>
              <span className="flex items-center gap-1"><Radio className="h-3 w-3" /> error</span><span className="truncate text-right font-mono text-neutral-300">{eventHealth.lastError ?? error ?? '—'}</span>
            </div>
          </div>
        </div>

        <div className="min-w-0 rounded-md border border-neutral-800 bg-neutral-950/70 p-3 xl:col-span-4">
          <div className="mb-2 flex items-center justify-between gap-2">
            <div className="flex items-center gap-2 text-xs font-medium text-neutral-300">
              <AlertTriangle className="h-4 w-4 text-amber-400" />
              Runbook
            </div>
            <span className="font-mono text-[11px] text-neutral-500">{runbook.length}</span>
          </div>
          {actionError ? <div className="mb-2 text-[11px] text-red-300">{actionError}</div> : null}
          <div className="grid gap-2 lg:grid-cols-2">
            {runbook.length ? runbook.slice(0, 4).map((item, index) => (
              <div key={`${item.source}-${index}`} className="min-w-0 border-l border-neutral-800 pl-3 text-xs">
                <div className={cn('truncate font-medium', item.severity === 'bad' ? 'text-red-300' : item.severity === 'warn' ? 'text-amber-300' : 'text-neutral-300')}>
                  {item.title}
                </div>
                <div className="mt-1 min-w-0 truncate text-neutral-500">{item.cause}</div>
                <div className="mt-1 flex min-w-0 items-center justify-between gap-2">
                  <span className="min-w-0 truncate text-neutral-400">{item.nextAction}</span>
                  {item.action ? (
                    <button
                      type="button"
                      onClick={() => runRunbookAction(item)}
                      className="shrink-0 rounded-sm border border-neutral-700 px-2 py-1 text-[11px] text-neutral-200 hover:border-neutral-500 hover:bg-neutral-900"
                    >
                      {item.action.label}
                    </button>
                  ) : null}
                </div>
              </div>
            )) : <div className="text-xs text-neutral-600">No runbook actions</div>}
          </div>
        </div>
      </div>
    </section>
  );
}
