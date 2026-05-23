'use client';

import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { CheckCircle2, Circle, Pause, Play, RefreshCw, Square, XCircle } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { cn } from '@/lib/utils';

type ReplayStatus = {
  activeCampaign?: ReplayCampaign | null;
  runs?: ReplayRun[];
  events?: ReplayEvent[];
  activeRuntimeCampaignIds?: string[];
  codexBin?: string;
  prompts?: {
    review?: string;
    plan?: string;
    implementPrefix?: string;
  };
  error?: string;
};

type ReplayCampaign = {
  id: string;
  status: string;
  current_phase?: string;
  project_root?: string;
  completed_cycles?: number;
  max_cycles?: number | null;
  interval_seconds?: number;
  last_error?: string | null;
  updated_at?: string;
};

type ReplayRun = {
  id: string;
  campaign_id: string;
  cycle_no: number;
  status: string;
  phase: string;
  thread_id?: string | null;
  review_turn_id?: string | null;
  plan_turn_id?: string | null;
  implement_turn_id?: string | null;
  plan_text?: string | null;
  selected_options_json?: unknown;
  blocked_reason?: string | null;
  last_error?: string | null;
  final_message?: string | null;
  updated_at?: string;
};

type ReplayEvent = {
  id: number;
  run_id?: string | null;
  cycle_no?: number | null;
  event_kind: string;
  phase: string;
  message: string;
  created_at?: string;
  payload?: unknown;
};

const PHASES = [
  ['review_turn_default', 'Review'],
  ['plan_turn_plan_mode', 'Plan'],
  ['awaiting_plan', 'Capture'],
  ['implement_turn_default', 'Execute'],
  ['completed', 'Done'],
] as const;

function statusTone(status?: string) {
  switch (status) {
    case 'running':
      return 'border-emerald-500/25 bg-emerald-500/10 text-emerald-300';
    case 'paused':
      return 'border-amber-500/25 bg-amber-500/10 text-amber-300';
    case 'completed':
      return 'border-sky-500/25 bg-sky-500/10 text-sky-300';
    case 'blocked':
    case 'failed':
      return 'border-red-500/25 bg-red-500/10 text-red-300';
    default:
      return 'border-neutral-800 bg-neutral-900/70 text-neutral-400';
  }
}

function formatTime(value?: string | null) {
  if (!value) return '—';
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return value;
  return date.toLocaleString();
}

function normalizeList<T>(value: T[] | undefined): T[] {
  return Array.isArray(value) ? value : [];
}

function eventPayload(event: ReplayEvent): Record<string, unknown> {
  return event.payload && typeof event.payload === 'object' && !Array.isArray(event.payload)
    ? event.payload as Record<string, unknown>
    : {};
}

function isPtyEvent(event: ReplayEvent) {
  return event.event_kind.startsWith('codex_replay_pty_');
}

function ptyDirection(event: ReplayEvent) {
  const direction = eventPayload(event).direction;
  return typeof direction === 'string' ? direction : 'output';
}

function ptyLine(event: ReplayEvent) {
  const line = eventPayload(event).line;
  return typeof line === 'string' ? line : event.message;
}

export function CodexReplayDashboard() {
  const [status, setStatus] = useState<ReplayStatus>({});
  const [selectedRunId, setSelectedRunId] = useState<string>('');
  const [loading, setLoading] = useState(false);
  const [busyAction, setBusyAction] = useState<string>('');
  const ptyScrollRef = useRef<HTMLDivElement | null>(null);

  const runs = normalizeList(status.runs);
  const events = normalizeList(status.events);
  const campaign = status.activeCampaign ?? null;
  const selectedRun = useMemo(
    () => runs.find((run) => run.id === selectedRunId) ?? runs[0] ?? null,
    [runs, selectedRunId],
  );

  const visibleEvents = useMemo(
    () => events.filter((event) => !selectedRun || !event.run_id || event.run_id === selectedRun.id),
    [events, selectedRun],
  );
  const ptyEvents = useMemo(
    () => visibleEvents.filter(isPtyEvent).slice().reverse(),
    [visibleEvents],
  );
  const timelineEvents = useMemo(
    () => visibleEvents.filter((event) => !isPtyEvent(event)),
    [visibleEvents],
  );
  const ptyLastId = ptyEvents.at(-1)?.id;

  const load = useCallback(() => {
    setLoading(true);
    fetch('/api/codex-replay/status?limit=200')
      .then((res) => res.json())
      .then((data: ReplayStatus) => {
        setStatus(data);
        setSelectedRunId((prev) => {
          if (prev && data.runs?.some((run) => run.id === prev)) return prev;
          return data.runs?.[0]?.id ?? '';
        });
      })
      .catch((err) => setStatus({ error: String(err) }))
      .finally(() => setLoading(false));
  }, []);

  useEffect(() => {
    load();
    const id = setInterval(load, 1500);
    return () => clearInterval(id);
  }, [load]);

  useEffect(() => {
    const el = ptyScrollRef.current;
    if (el) el.scrollTop = el.scrollHeight;
  }, [ptyLastId]);

  const control = useCallback(
    async (action: string) => {
      setBusyAction(action);
      try {
        const res = await fetch('/api/codex-replay/control', {
          method: 'POST',
          headers: { 'content-type': 'application/json' },
          body: JSON.stringify({
            action,
            campaignId: campaign?.id,
          }),
        });
        const data = await res.json();
        setStatus(data);
      } finally {
        setBusyAction('');
        load();
      }
    },
    [campaign?.id, load],
  );

  const currentPhase = selectedRun?.phase || campaign?.current_phase || 'queued';
  const selectedOptions = Array.isArray(selectedRun?.selected_options_json)
    ? selectedRun?.selected_options_json
    : [];

  return (
    <div className="mx-4 mb-4 min-h-0 flex-1 overflow-hidden rounded-lg border border-neutral-800 bg-neutral-950/60 sm:mx-8">
      <div className="flex flex-wrap items-center justify-between gap-3 border-b border-neutral-800 px-4 py-3">
        <div className="min-w-0">
          <div className="flex items-center gap-2">
            <h2 className="text-sm font-semibold text-neutral-100">Codex Loop</h2>
            <span className={cn('rounded-md border px-2 py-0.5 text-[10px] font-medium', statusTone(campaign?.status))}>
              {campaign?.status || 'idle'}
            </span>
            {loading ? <RefreshCw className="h-3.5 w-3.5 animate-spin text-neutral-500" /> : null}
          </div>
          <div className="mt-1 truncate text-[11px] text-neutral-600" title={campaign?.project_root}>
            {campaign?.project_root || '/Users/jinchen/Projects/missiond'}
          </div>
        </div>
        <div className="flex flex-wrap items-center gap-2">
          <Button size="sm" variant="outline" onClick={() => control('run_once')} disabled={!!busyAction} className="gap-1 border-neutral-800 text-neutral-300">
            <Play className="h-3.5 w-3.5" />
            Run once
          </Button>
          <Button size="sm" variant="outline" onClick={() => control('start_campaign')} disabled={!!busyAction} className="gap-1 border-neutral-800 text-neutral-300">
            <RefreshCw className="h-3.5 w-3.5" />
            Start loop
          </Button>
          <Button size="sm" variant="outline" onClick={() => control(campaign?.status === 'paused' ? 'resume_campaign' : 'pause_campaign')} disabled={!campaign || !!busyAction} className="gap-1 border-neutral-800 text-neutral-300">
            {campaign?.status === 'paused' ? <Play className="h-3.5 w-3.5" /> : <Pause className="h-3.5 w-3.5" />}
            {campaign?.status === 'paused' ? 'Resume' : 'Pause'}
          </Button>
          <Button size="sm" variant="outline" onClick={() => control('stop_campaign')} disabled={!campaign || !!busyAction} className="gap-1 border-neutral-800 text-neutral-300">
            <Square className="h-3.5 w-3.5" />
            Stop
          </Button>
        </div>
      </div>

      {status.error ? (
        <div className="m-4 rounded-md border border-red-500/20 bg-red-500/10 p-3 text-sm text-red-200">{status.error}</div>
      ) : null}

      <div className="grid h-[calc(100vh-9.5rem)] min-h-0 grid-cols-1 lg:grid-cols-[18rem_minmax(0,1fr)_22rem]">
        <aside className="min-h-0 border-b border-neutral-800 lg:border-b-0 lg:border-r">
          <div className="border-b border-neutral-800 px-3 py-2 text-xs font-medium text-neutral-300">Cycles</div>
          <div className="min-h-0 space-y-1 overflow-auto p-2">
            {runs.length ? runs.map((run) => (
              <button
                key={run.id}
                onClick={() => setSelectedRunId(run.id)}
                className={cn(
                  'w-full rounded-md border px-2 py-2 text-left transition-colors',
                  selectedRun?.id === run.id
                    ? 'border-orange-500/40 bg-orange-500/10'
                    : 'border-neutral-900 bg-neutral-900/40 hover:border-neutral-700',
                )}
              >
                <div className="flex items-center justify-between gap-2">
                  <span className="text-xs font-medium text-neutral-200">Cycle {run.cycle_no}</span>
                  <span className={cn('rounded border px-1.5 py-0.5 text-[10px]', statusTone(run.status))}>{run.status}</span>
                </div>
                <div className="mt-1 truncate font-mono text-[10px] text-neutral-600">{run.phase}</div>
                <div className="mt-1 text-[10px] text-neutral-700">{formatTime(run.updated_at)}</div>
              </button>
            )) : (
              <div className="px-1 py-2 text-xs text-neutral-600">No replay cycles yet.</div>
            )}
          </div>
        </aside>

        <main className="min-h-0 overflow-auto p-4">
          <div className="grid gap-3 xl:grid-cols-5">
            {PHASES.map(([phase, label]) => {
              const active = currentPhase === phase;
              const done = selectedRun?.status === 'completed' || PHASES.findIndex((item) => item[0] === currentPhase) > PHASES.findIndex((item) => item[0] === phase);
              return (
                <div key={phase} className={cn('rounded-md border p-3', active ? 'border-orange-500/40 bg-orange-500/10' : 'border-neutral-800 bg-neutral-900/40')}>
                  <div className="flex items-center gap-2">
                    {done ? <CheckCircle2 className="h-4 w-4 text-emerald-400" /> : active ? <Circle className="h-4 w-4 fill-orange-400 text-orange-400" /> : <Circle className="h-4 w-4 text-neutral-700" />}
                    <span className="text-xs font-medium text-neutral-200">{label}</span>
                  </div>
                  <div className="mt-1 truncate font-mono text-[10px] text-neutral-600">{phase}</div>
                </div>
              );
            })}
          </div>

          <div className="mt-4 grid gap-4 xl:grid-cols-2">
            <section className="rounded-md border border-neutral-800 bg-neutral-900/35">
              <div className="border-b border-neutral-800 px-3 py-2 text-xs font-medium text-neutral-300">Exact Inputs</div>
              <div className="space-y-3 p-3">
                <PromptBlock label="1 default" text={status.prompts?.review || ''} />
                <PromptBlock label="2 plan" text={status.prompts?.plan || ''} />
                <PromptBlock label="3 execute prefix" text={status.prompts?.implementPrefix || ''} />
              </div>
            </section>

            <section className="rounded-md border border-neutral-800 bg-neutral-900/35">
              <div className="border-b border-neutral-800 px-3 py-2 text-xs font-medium text-neutral-300">Thread</div>
              <dl className="grid grid-cols-[7rem_minmax(0,1fr)] gap-x-3 gap-y-2 p-3 text-xs">
                <dt className="text-neutral-600">Thread</dt>
                <dd className="truncate font-mono text-neutral-300" title={selectedRun?.thread_id || undefined}>{selectedRun?.thread_id || '—'}</dd>
                <dt className="text-neutral-600">Codex bin</dt>
                <dd className="truncate font-mono text-neutral-300" title={status.codexBin || undefined}>{status.codexBin || '—'}</dd>
                <dt className="text-neutral-600">Review turn</dt>
                <dd className="truncate font-mono text-neutral-300">{selectedRun?.review_turn_id || '—'}</dd>
                <dt className="text-neutral-600">Plan turn</dt>
                <dd className="truncate font-mono text-neutral-300">{selectedRun?.plan_turn_id || '—'}</dd>
                <dt className="text-neutral-600">Execute turn</dt>
                <dd className="truncate font-mono text-neutral-300">{selectedRun?.implement_turn_id || '—'}</dd>
                <dt className="text-neutral-600">Issue</dt>
                <dd className="text-red-300">{selectedRun?.blocked_reason || selectedRun?.last_error || campaign?.last_error || '—'}</dd>
              </dl>
            </section>
          </div>

          <section className="mt-4 rounded-md border border-neutral-800 bg-neutral-900/35">
            <div className="flex items-center justify-between border-b border-neutral-800 px-3 py-2">
              <div className="text-xs font-medium text-neutral-300">PTY I/O</div>
              <div className="font-mono text-[10px] text-neutral-600">{ptyEvents.length} lines</div>
            </div>
            <div ref={ptyScrollRef} className="max-h-[30rem] overflow-auto bg-neutral-950/80 p-2 font-mono text-[11px] leading-relaxed">
              {ptyEvents.length ? ptyEvents.map((event) => {
                const direction = ptyDirection(event);
                const payload = eventPayload(event);
                const method = typeof payload.method === 'string' ? payload.method : '';
                return (
                  <div key={event.id} className="grid grid-cols-[4.5rem_minmax(0,1fr)] gap-2 border-b border-neutral-900/80 py-1.5 last:border-b-0">
                    <div className="space-y-1">
                      <span className={cn(
                        'inline-flex rounded border px-1.5 py-0.5 text-[10px] uppercase',
                        direction === 'stdin'
                          ? 'border-sky-500/25 bg-sky-500/10 text-sky-300'
                          : direction === 'stderr'
                            ? 'border-red-500/25 bg-red-500/10 text-red-300'
                            : 'border-emerald-500/25 bg-emerald-500/10 text-emerald-300',
                      )}>
                        {direction}
                      </span>
                      <div className="text-[10px] text-neutral-700">{formatTime(event.created_at)}</div>
                    </div>
                    <div className="min-w-0">
                      <div className="mb-1 truncate text-[10px] text-neutral-600" title={event.message}>
                        {method || event.message}
                      </div>
                      <pre className="whitespace-pre-wrap break-words text-neutral-300">{ptyLine(event)}</pre>
                    </div>
                  </div>
                );
              }) : (
                <div className="p-2 text-xs text-neutral-600">No PTY I/O yet.</div>
              )}
            </div>
          </section>

          <section className="mt-4 rounded-md border border-neutral-800 bg-neutral-900/35">
            <div className="flex items-center justify-between border-b border-neutral-800 px-3 py-2">
              <div className="text-xs font-medium text-neutral-300">Captured Plan</div>
              <div className="font-mono text-[10px] text-neutral-600">{selectedRun?.plan_text?.length || 0} chars</div>
            </div>
            <pre className="max-h-80 overflow-auto whitespace-pre-wrap p-3 text-xs leading-relaxed text-neutral-300">
              {selectedRun?.plan_text || 'No proposed plan captured yet.'}
            </pre>
          </section>

          <section className="mt-4 rounded-md border border-neutral-800 bg-neutral-900/35">
            <div className="border-b border-neutral-800 px-3 py-2 text-xs font-medium text-neutral-300">Recommended Selections</div>
            <pre className="max-h-52 overflow-auto whitespace-pre-wrap p-3 text-xs text-neutral-400">
              {selectedOptions.length ? JSON.stringify(selectedOptions, null, 2) : 'No Plan Mode selections recorded.'}
            </pre>
          </section>
        </main>

        <aside className="min-h-0 border-t border-neutral-800 lg:border-l lg:border-t-0">
          <div className="border-b border-neutral-800 px-3 py-2 text-xs font-medium text-neutral-300">Events</div>
          <div className="min-h-0 space-y-2 overflow-auto p-3">
            {timelineEvents.length ? timelineEvents.map((event) => (
              <div key={event.id} className="rounded-md border border-neutral-800 bg-neutral-900/45 p-2">
                <div className="flex items-start gap-2">
                  {event.event_kind.includes('failed') || event.event_kind.includes('blocked') ? <XCircle className="mt-0.5 h-3.5 w-3.5 text-red-400" /> : <Circle className="mt-0.5 h-3.5 w-3.5 text-neutral-600" />}
                  <div className="min-w-0 flex-1">
                    <div className="truncate text-[11px] font-medium text-neutral-300" title={event.event_kind}>{event.event_kind}</div>
                    <div className="mt-0.5 text-[11px] leading-snug text-neutral-500">{event.message}</div>
                    <div className="mt-1 flex items-center justify-between gap-2 font-mono text-[10px] text-neutral-700">
                      <span className="truncate">{event.phase}</span>
                      <span className="shrink-0">{formatTime(event.created_at)}</span>
                    </div>
                  </div>
                </div>
              </div>
            )) : (
              <div className="text-xs text-neutral-600">No replay events yet.</div>
            )}
          </div>
        </aside>
      </div>
    </div>
  );
}

function PromptBlock({ label, text }: { label: string; text: string }) {
  return (
    <div>
      <div className="mb-1 text-[10px] uppercase tracking-wide text-neutral-600">{label}</div>
      <pre className="overflow-auto rounded-md border border-neutral-800 bg-neutral-950/70 p-2 text-xs leading-relaxed text-neutral-300">
        {text || '—'}
      </pre>
    </div>
  );
}
