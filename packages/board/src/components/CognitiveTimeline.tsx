'use client';

import { useState, useEffect, useCallback, useMemo, useRef } from 'react';
import {
  Search, RefreshCw, Sparkles, AlertTriangle,
  Zap, Brain,
  MessageSquare, GitBranch, Activity, Cpu, Settings2,
} from 'lucide-react';
import { cn } from '@/lib/utils';
import { useEventInvalidation } from '../hooks/useEventStream';

// ── Types ──────────────────────────────────────────────────

interface TimelineEvent {
  seq: number;
  event_type: string;
  trace_id: string | null;
  span_id: string | null;
  parent_span_id: string | null;
  summary: string | null;
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  payload: any;
  created_at: string;
}

interface TimelineStats {
  total_events: number;
  by_type: Array<[string, number]>; // [event_type, count] tuples
  traced_events: number;
  unique_traces: number;
  gemini_latency: { p50_ms: number; p90_ms: number; p99_ms: number } | null;
}

// ── Constants ──────────────────────────────────────────────

const EVENT_COLORS: Record<string, { dot: string; bg: string; text: string; label: string; icon: React.ReactNode }> = {
  slot_state_changed:       { dot: 'bg-slate-400',   bg: 'bg-slate-400/10',   text: 'text-slate-400',   label: 'Slot',     icon: <Settings2 className="w-3 h-3" /> },
  task_lifecycle:           { dot: 'bg-blue-400',    bg: 'bg-blue-400/10',    text: 'text-blue-400',    label: 'Task',     icon: <Activity className="w-3 h-3" /> },
  question_created:         { dot: 'bg-amber-400',   bg: 'bg-amber-400/10',   text: 'text-amber-400',   label: 'Question', icon: <MessageSquare className="w-3 h-3" /> },
  gemini_request_completed: { dot: 'bg-purple-400',  bg: 'bg-purple-400/10',  text: 'text-purple-400',  label: 'Gemini',   icon: <Cpu className="w-3 h-3" /> },
  decision_made:            { dot: 'bg-orange-400',  bg: 'bg-orange-400/10',  text: 'text-orange-400',  label: 'Decision', icon: <GitBranch className="w-3 h-3" /> },
  question_resolved:        { dot: 'bg-amber-300',   bg: 'bg-amber-300/10',   text: 'text-amber-300',   label: 'Resolved', icon: <MessageSquare className="w-3 h-3" /> },
  memory_phase_changed:     { dot: 'bg-cyan-400',    bg: 'bg-cyan-400/10',    text: 'text-cyan-400',    label: 'Memory',   icon: <Brain className="w-3 h-3" /> },
  board_task_updated:       { dot: 'bg-blue-300',    bg: 'bg-blue-300/10',    text: 'text-blue-300',    label: 'Board',    icon: <Activity className="w-3 h-3" /> },
  insight_generated:        { dot: 'bg-emerald-400', bg: 'bg-emerald-400/10', text: 'text-emerald-400', label: 'Insight',  icon: <Sparkles className="w-3 h-3" /> },
};

const SWIMLANES = [
  { id: 'ai',   label: 'AI / LLM',  types: ['gemini_request_completed', 'decision_made', 'insight_generated'] },
  { id: 'flow', label: 'Flow',       types: ['task_lifecycle', 'question_created', 'question_resolved', 'board_task_updated'] },
  { id: 'sys',  label: 'System',     types: ['slot_state_changed', 'memory_phase_changed'] },
];

const QUICK_FILTERS = [
  { label: 'All', value: 'all' },
  { label: 'Errors', value: 'errors' },
  { label: 'Insights', value: 'insights' },
  { label: 'Gemini', value: 'gemini' },
];

const WINDOW_OPTIONS = [
  { label: '1h', value: '1h' },
  { label: '6h', value: '6h' },
  { label: '24h', value: '24h' },
  { label: '7d', value: '7d' },
];

// ── Helpers ────────────────────────────────────────────────

function formatTime(dateStr: string): string {
  return new Date(dateStr).toLocaleTimeString('en-US', { hour12: false, hour: '2-digit', minute: '2-digit', second: '2-digit' });
}

function shortTrace(id: string | null): string {
  if (!id) return '';
  return id.slice(0, 7);
}

function getEventColor(type: string) {
  return EVENT_COLORS[type] || { dot: 'bg-neutral-500', bg: 'bg-neutral-500/10', text: 'text-neutral-400', label: type, icon: <Zap className="w-3 h-3" /> };
}

function getSwimlane(type: string): number {
  for (let i = 0; i < SWIMLANES.length; i++) {
    if (SWIMLANES[i].types.includes(type)) return i;
  }
  return 2; // default to system
}

function eventSummary(ev: TimelineEvent): string {
  if (ev.summary) return ev.summary;
  const p = ev.payload;
  if (!p) return ev.event_type;
  switch (ev.event_type) {
    case 'slot_state_changed': return `${p.slot_id || ''} → ${p.new_state || ''}`;
    case 'task_lifecycle': return `${p.action || ''}: ${p.task_title || p.task_id || ''}`;
    case 'gemini_request_completed': return `${p.caller || ''} ${p.duration_ms ? p.duration_ms + 'ms' : ''} ${p.error ? '❌' : ''}`;
    case 'decision_made': return `${p.tier || ''}: ${p.question?.slice(0, 60) || ''}`;
    case 'insight_generated': return `${p.title || ''}`;
    case 'board_task_updated': return `${p.title || ''} → ${p.status || ''}`;
    default: return ev.event_type;
  }
}

function hasError(ev: TimelineEvent): boolean {
  if (!ev.payload) return false;
  return !!ev.payload.error || !!ev.payload.error_msg || ev.payload.status === 'error';
}

// ── Main Component ─────────────────────────────────────────

export function CognitiveTimeline() {
  const timelineVersion = useEventInvalidation('timeline');

  const [events, setEvents] = useState<TimelineEvent[]>([]);
  const [stats, setStats] = useState<TimelineStats | null>(null);
  const [selectedEvent, setSelectedEvent] = useState<TimelineEvent | null>(null);
  const [traceEvents, setTraceEvents] = useState<TimelineEvent[]>([]);
  const [quickFilter, setQuickFilter] = useState('all');
  const [searchQuery, setSearchQuery] = useState('');
  const [window, setWindow] = useState('24h');
  const [loading, setLoading] = useState(false);
  const timelineRef = useRef<HTMLDivElement>(null);

  // Fetch data
  const fetchData = useCallback(async () => {
    setLoading(true);
    try {
      const params = new URLSearchParams({ window, limit: '500' });
      if (searchQuery) params.set('query', searchQuery);

      const [evRes, stRes] = await Promise.allSettled([
        fetch(`/api/timeline/events?${params}`).then(r => r.json()),
        fetch(`/api/timeline/stats?${params.get('window') ? `window=${window}` : ''}`).then(r => r.json()),
      ]);

      if (evRes.status === 'fulfilled') {
        const raw = evRes.value.events;
        setEvents(Array.isArray(raw) ? raw : (raw?.events || []));
      }
      if (stRes.status === 'fulfilled' && !stRes.value.error) {
        setStats(stRes.value);
      }
    } catch { /* ignore */ }
    setLoading(false);
  }, [window, searchQuery]);

  useEffect(() => { fetchData(); }, [fetchData, timelineVersion]);

  // Load trace events when clicking an event with trace_id
  const selectEvent = useCallback(async (ev: TimelineEvent) => {
    setSelectedEvent(ev);
    if (ev.trace_id) {
      try {
        const res = await fetch(`/api/timeline/events?traceId=${ev.trace_id}&limit=50`);
        const data = await res.json();
        const raw = data.events;
        setTraceEvents(Array.isArray(raw) ? raw : (raw?.events || []));
      } catch { setTraceEvents([]); }
    } else {
      setTraceEvents([]);
    }
  }, []);

  // Filtered events
  const filtered = useMemo(() => {
    let result = events;
    if (quickFilter === 'errors') result = result.filter(hasError);
    else if (quickFilter === 'insights') result = result.filter(e => e.event_type === 'insight_generated');
    else if (quickFilter === 'gemini') result = result.filter(e => e.event_type === 'gemini_request_completed');
    return result;
  }, [events, quickFilter]);

  // Compute time range for horizontal axis
  const timeRange = useMemo(() => {
    if (filtered.length === 0) return { min: Date.now() - 3600000, max: Date.now() };
    const times = filtered.map(e => new Date(e.created_at).getTime());
    const min = Math.min(...times);
    const max = Math.max(...times);
    const padding = Math.max((max - min) * 0.05, 60000); // 5% padding, min 1 min
    return { min: min - padding, max: max + padding };
  }, [filtered]);

  // Position calculation for horizontal timeline
  const getX = useCallback((dateStr: string) => {
    const t = new Date(dateStr).getTime();
    const { min, max } = timeRange;
    const range = max - min;
    if (range <= 0) return 50;
    return ((t - min) / range) * 100;
  }, [timeRange]);

  // Time axis ticks
  const timeTicks = useMemo(() => {
    const { min, max } = timeRange;
    const range = max - min;
    const count = 6;
    return Array.from({ length: count + 1 }, (_, i) => {
      const t = min + (range * i) / count;
      return { pos: (i / count) * 100, label: formatTime(new Date(t).toISOString()) };
    });
  }, [timeRange]);

  // Density histogram for minimap
  const histogram = useMemo(() => {
    const buckets = 60;
    const { min, max } = timeRange;
    const range = max - min;
    if (range <= 0) return [];
    const counts = new Array(buckets).fill(0);
    const errorCounts = new Array(buckets).fill(0);
    for (const ev of events) {
      const t = new Date(ev.created_at).getTime();
      const idx = Math.min(Math.floor(((t - min) / range) * buckets), buckets - 1);
      if (idx >= 0) {
        counts[idx]++;
        if (hasError(ev)) errorCounts[idx]++;
      }
    }
    const maxCount = Math.max(...counts, 1);
    return counts.map((c, i) => ({ height: (c / maxCount) * 100, errors: errorCounts[i], idx: i }));
  }, [events, timeRange]);

  return (
    <div className="flex-1 flex flex-col min-h-0 px-4 sm:px-8 pb-4 gap-3">
      {/* ── Top Bar: Stats + Controls ── */}
      <div className="flex items-center justify-between gap-4 flex-wrap">
        {/* Stats cards */}
        <div className="flex items-center gap-3">
          <StatCard label="Events" value={stats?.total_events ?? '-'} />
          <StatCard label="Traces" value={stats?.unique_traces ?? '-'} />
          <StatCard
            label="Gemini P90"
            value={stats?.gemini_latency ? `${stats.gemini_latency.p90_ms}ms` : '-'}
            color={stats?.gemini_latency && stats.gemini_latency.p90_ms > 10000 ? 'text-red-400' : undefined}
          />
          <StatCard
            label="Insights"
            value={stats?.by_type?.find(t => t[0] === 'insight_generated')?.[1] ?? 0}
            color="text-emerald-400"
          />
        </div>

        {/* Controls */}
        <div className="flex items-center gap-2">
          {/* Search */}
          <div className="relative">
            <Search className="absolute left-2 top-1/2 -translate-y-1/2 w-3 h-3 text-neutral-500" />
            <input
              type="text"
              placeholder="Search..."
              value={searchQuery}
              onChange={(e) => setSearchQuery(e.target.value)}
              onKeyDown={(e) => e.key === 'Enter' && fetchData()}
              className="pl-7 pr-3 py-1.5 text-xs bg-neutral-900 border border-neutral-800 rounded-md text-neutral-300 placeholder-neutral-600 w-44 focus:outline-none focus:border-neutral-600"
            />
          </div>

          {/* Quick filters */}
          <div className="flex items-center gap-0.5 bg-neutral-900 rounded-md p-0.5">
            {QUICK_FILTERS.map(f => (
              <button
                key={f.value}
                onClick={() => setQuickFilter(f.value)}
                className={cn(
                  'px-2 py-1 text-[10px] font-medium rounded transition-colors',
                  quickFilter === f.value ? 'bg-neutral-800 text-white' : 'text-neutral-500 hover:text-neutral-300',
                )}
              >
                {f.label}
              </button>
            ))}
          </div>

          {/* Window */}
          <div className="flex items-center gap-0.5 bg-neutral-900 rounded-md p-0.5">
            {WINDOW_OPTIONS.map(w => (
              <button
                key={w.value}
                onClick={() => setWindow(w.value)}
                className={cn(
                  'px-2 py-1 text-[10px] font-medium rounded transition-colors',
                  window === w.value ? 'bg-neutral-800 text-white' : 'text-neutral-500 hover:text-neutral-300',
                )}
              >
                {w.label}
              </button>
            ))}
          </div>

          <button onClick={fetchData} className="p-1.5 rounded hover:bg-neutral-800 text-neutral-500 hover:text-neutral-300 transition-colors">
            <RefreshCw className={cn('w-3.5 h-3.5', loading && 'animate-spin')} />
          </button>
        </div>
      </div>

      {/* ── Horizontal Timeline (upper section) ── */}
      <div className="border border-neutral-800 rounded-lg bg-neutral-950/50 flex flex-col" style={{ minHeight: 200 }}>
        {/* Swimlane labels + timeline area */}
        <div className="flex flex-1 min-h-0">
          {/* Swimlane labels */}
          <div className="w-16 shrink-0 border-r border-neutral-800 flex flex-col">
            {SWIMLANES.map((lane, i) => (
              <div key={lane.id} className={cn(
                'flex-1 flex items-center justify-center text-[10px] text-neutral-500 font-medium',
                i < SWIMLANES.length - 1 && 'border-b border-neutral-800/50',
              )}>
                {lane.label}
              </div>
            ))}
          </div>

          {/* Timeline canvas */}
          <div ref={timelineRef} className="flex-1 relative overflow-x-auto" style={{ minHeight: 140 }}>
            {/* Swimlane backgrounds */}
            {SWIMLANES.map((lane, i) => (
              <div
                key={lane.id}
                className={cn(
                  'absolute left-0 right-0',
                  i < SWIMLANES.length - 1 && 'border-b border-neutral-800/30',
                )}
                style={{ top: `${(i / SWIMLANES.length) * 100}%`, height: `${100 / SWIMLANES.length}%` }}
              />
            ))}

            {/* Event dots */}
            {filtered.map((ev) => {
              const x = getX(ev.created_at);
              const laneIdx = getSwimlane(ev.event_type);
              const laneY = ((laneIdx + 0.5) / SWIMLANES.length) * 100;
              const ec = getEventColor(ev.event_type);
              const isSelected = selectedEvent?.seq === ev.seq;
              const isInsight = ev.event_type === 'insight_generated';
              const isError = hasError(ev);

              return (
                <button
                  key={ev.seq}
                  onClick={() => selectEvent(ev)}
                  className={cn(
                    'absolute -translate-x-1/2 -translate-y-1/2 rounded-full transition-all z-10',
                    isInsight ? 'w-4 h-4 ring-2 ring-emerald-400/40' :
                    isError ? 'w-3 h-3 ring-2 ring-red-500/40' :
                    'w-2.5 h-2.5 hover:w-3.5 hover:h-3.5',
                    ec.dot,
                    isSelected && 'ring-2 ring-white/60 w-4 h-4',
                  )}
                  style={{ left: `${x}%`, top: `${laneY}%` }}
                  title={`${ec.label}: ${eventSummary(ev)}\n${formatTime(ev.created_at)}`}
                />
              );
            })}

            {/* Trace connecting lines — show when event selected */}
            {selectedEvent?.trace_id && traceEvents.length > 1 && (() => {
              const sorted = [...traceEvents].sort((a, b) => new Date(a.created_at).getTime() - new Date(b.created_at).getTime());
              return sorted.slice(0, -1).map((ev, i) => {
                const next = sorted[i + 1];
                const x1 = getX(ev.created_at);
                const x2 = getX(next.created_at);
                const y1 = ((getSwimlane(ev.event_type) + 0.5) / SWIMLANES.length) * 100;
                const y2 = ((getSwimlane(next.event_type) + 0.5) / SWIMLANES.length) * 100;
                return (
                  <svg key={`line-${i}`} className="absolute inset-0 w-full h-full pointer-events-none z-0" preserveAspectRatio="none">
                    <line
                      x1={`${x1}%`} y1={`${y1}%`}
                      x2={`${x2}%`} y2={`${y2}%`}
                      stroke="rgba(255,255,255,0.15)"
                      strokeWidth="1"
                      strokeDasharray="4 2"
                    />
                  </svg>
                );
              });
            })()}
          </div>
        </div>

        {/* Time axis */}
        <div className="h-5 border-t border-neutral-800 flex items-center relative ml-16">
          {timeTicks.map((tick, i) => (
            <span
              key={i}
              className="absolute text-[9px] text-neutral-600 -translate-x-1/2"
              style={{ left: `${tick.pos}%` }}
            >
              {tick.label}
            </span>
          ))}
        </div>

        {/* Minimap density histogram */}
        <div className="h-6 border-t border-neutral-800 flex items-end px-0 ml-16">
          {histogram.map((bar, i) => (
            <div
              key={i}
              className="flex-1"
              style={{ height: `${Math.max(bar.height, 2)}%` }}
            >
              <div
                className={cn('w-full h-full', bar.errors > 0 ? 'bg-red-500/40' : 'bg-neutral-700/40')}
              />
            </div>
          ))}
        </div>
      </div>

      {/* ── Detail Panel (lower section) ── */}
      <div className="flex-1 min-h-0 border border-neutral-800 rounded-lg bg-neutral-950/50 overflow-auto">
        {selectedEvent ? (
          <div className="flex h-full min-h-0">
            {/* Left: metadata + trace tree */}
            <div className="w-72 shrink-0 border-r border-neutral-800 p-3 overflow-y-auto">
              <EventMeta event={selectedEvent} />
              {traceEvents.length > 1 && (
                <div className="mt-4">
                  <h4 className="text-[10px] text-neutral-500 uppercase tracking-wider mb-2">Trace Chain</h4>
                  <div className="space-y-1">
                    {[...traceEvents]
                      .sort((a, b) => new Date(a.created_at).getTime() - new Date(b.created_at).getTime())
                      .map(tev => {
                        const ec = getEventColor(tev.event_type);
                        const isActive = tev.seq === selectedEvent.seq;
                        return (
                          <button
                            key={tev.seq}
                            onClick={() => selectEvent(tev)}
                            className={cn(
                              'w-full text-left px-2 py-1.5 rounded text-xs flex items-center gap-2 transition-colors',
                              isActive ? 'bg-neutral-800 text-white' : 'text-neutral-400 hover:bg-neutral-800/50',
                            )}
                          >
                            <div className={cn('w-2 h-2 rounded-full shrink-0', ec.dot)} />
                            <span className={cn('truncate', ec.text)}>{ec.label}</span>
                            <span className="text-neutral-600 text-[10px] ml-auto">{formatTime(tev.created_at)}</span>
                          </button>
                        );
                      })}
                  </div>
                </div>
              )}
            </div>

            {/* Right: payload */}
            <div className="flex-1 p-3 overflow-y-auto min-w-0">
              <EventPayload event={selectedEvent} />
            </div>
          </div>
        ) : (
          <div className="flex items-center justify-center h-full text-neutral-600 text-xs">
            Click an event on the timeline to view details
          </div>
        )}
      </div>
    </div>
  );
}

// ── Sub-components ─────────────────────────────────────────

function StatCard({ label, value, color }: { label: string; value: string | number; color?: string }) {
  return (
    <div className="px-3 py-1.5 bg-neutral-900 border border-neutral-800 rounded-md">
      <div className="text-[10px] text-neutral-500">{label}</div>
      <div className={cn('text-sm font-mono font-medium', color || 'text-neutral-200')}>{value}</div>
    </div>
  );
}

function EventMeta({ event }: { event: TimelineEvent }) {
  const ec = getEventColor(event.event_type);
  return (
    <div className="space-y-3">
      {/* Header */}
      <div className="flex items-center gap-2">
        <div className={cn('w-3 h-3 rounded-full', ec.dot)} />
        <span className={cn('text-sm font-medium', ec.text)}>{ec.label}</span>
        {hasError(event) && <AlertTriangle className="w-3 h-3 text-red-400" />}
      </div>

      {/* Summary */}
      {event.event_type === 'insight_generated' ? (
        <div className="p-2 rounded border border-emerald-500/30 bg-emerald-950/20">
          <div className="flex items-center gap-1.5 mb-1">
            <Sparkles className="w-3 h-3 text-emerald-400" />
            <span className="text-[10px] text-emerald-400 uppercase tracking-wider">Insight</span>
          </div>
          <p className="text-xs text-emerald-200">{eventSummary(event)}</p>
        </div>
      ) : (
        <p className="text-xs text-neutral-300">{eventSummary(event)}</p>
      )}

      {/* Meta fields */}
      <div className="space-y-1.5 text-[11px]">
        <MetaRow label="Time" value={new Date(event.created_at).toLocaleString()} />
        <MetaRow label="Seq" value={`#${event.seq}`} />
        {event.trace_id && <MetaRow label="Trace" value={shortTrace(event.trace_id)} mono />}
        {event.span_id && <MetaRow label="Span" value={shortTrace(event.span_id)} mono />}
        {event.parent_span_id && <MetaRow label="Parent" value={shortTrace(event.parent_span_id)} mono />}
      </div>
    </div>
  );
}

function MetaRow({ label, value, mono }: { label: string; value: string; mono?: boolean }) {
  return (
    <div className="flex justify-between">
      <span className="text-neutral-500">{label}</span>
      <span className={cn('text-neutral-300', mono && 'font-mono')}>{value}</span>
    </div>
  );
}

function EventPayload({ event }: { event: TimelineEvent }) {
  const [tab, setTab] = useState<'summary' | 'payload'>('summary');

  return (
    <div>
      {/* Tabs */}
      <div className="flex items-center gap-1 mb-3">
        <button
          onClick={() => setTab('summary')}
          className={cn(
            'px-2.5 py-1 text-[10px] font-medium rounded transition-colors',
            tab === 'summary' ? 'bg-neutral-800 text-white' : 'text-neutral-500 hover:text-neutral-300',
          )}
        >
          Summary
        </button>
        <button
          onClick={() => setTab('payload')}
          className={cn(
            'px-2.5 py-1 text-[10px] font-medium rounded transition-colors',
            tab === 'payload' ? 'bg-neutral-800 text-white' : 'text-neutral-500 hover:text-neutral-300',
          )}
        >
          Payload
        </button>
      </div>

      {tab === 'summary' ? (
        <div className="space-y-2">
          <p className="text-xs text-neutral-300 leading-relaxed">{eventSummary(event)}</p>
          {/* Quick payload fields for common event types */}
          {event.event_type === 'gemini_request_completed' && event.payload && (
            <div className="grid grid-cols-2 gap-2 mt-3">
              <MiniStat label="Duration" value={`${event.payload.duration_ms || 0}ms`} />
              <MiniStat label="Model" value={event.payload.model || '-'} />
              <MiniStat label="Prompt" value={`${event.payload.prompt_chars || 0} chars`} />
              <MiniStat label="Response" value={`${event.payload.response_chars || 0} chars`} />
              <MiniStat label="Caller" value={event.payload.caller || '-'} />
              <MiniStat label="Status" value={event.payload.status || event.payload.error ? 'error' : 'ok'} />
            </div>
          )}
        </div>
      ) : (
        <pre className="text-[11px] text-neutral-400 font-mono bg-neutral-900 rounded p-3 overflow-auto max-h-80 whitespace-pre-wrap break-all">
          {JSON.stringify(event.payload, null, 2)}
        </pre>
      )}
    </div>
  );
}

function MiniStat({ label, value }: { label: string; value: string }) {
  return (
    <div className="px-2 py-1.5 bg-neutral-900 rounded border border-neutral-800">
      <div className="text-[9px] text-neutral-500">{label}</div>
      <div className="text-[11px] text-neutral-300 font-mono truncate">{value}</div>
    </div>
  );
}
