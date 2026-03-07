'use client';

import { useState, useEffect, useCallback, useMemo, useRef, memo } from 'react';
import {
  Search, RefreshCw, Sparkles, AlertTriangle,
  Zap, Brain, Wrench, ArrowRight, ChevronRight, ChevronDown,
  MessageSquare, GitBranch, GitCommit, Activity, Cpu, Settings2, User, Clock,
  FileCode, Terminal, Eye, File, ArrowUp, ArrowDown, BookOpen, Loader2, Languages, CheckCheck,
} from 'lucide-react';
import { diffLines } from 'diff';
import ReactMarkdown from 'react-markdown';
import remarkGfm from 'remark-gfm';
import { cn } from '@/lib/utils';
import { utcMs, formatBeijing, formatBeijingTime } from '@/lib/time';
import { useEventInvalidation } from '../hooks/useEventStream';
import { useTimelineGestures } from '../hooks/useTimelineGestures';

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

// Color scheme: each major category has a dedicated hue that never overlaps
// Gemini = purple, GPT/Codex = lime, Commit = yellow, Chat = blue/teal, Board = indigo, System = slate
const EVENT_COLORS: Record<string, { dot: string; bg: string; text: string; label: string; icon: React.ReactNode }> = {
  // ── Chat ──
  user_message:             { dot: 'bg-blue-400',    bg: 'bg-blue-400/10',    text: 'text-blue-400',    label: 'User',      icon: <MessageSquare className="w-3 h-3" /> },
  assistant_message:        { dot: 'bg-teal-400',    bg: 'bg-teal-400/10',    text: 'text-teal-400',    label: 'Assistant',  icon: <Brain className="w-3 h-3" /> },
  thinking_message:         { dot: 'bg-violet-400',  bg: 'bg-violet-400/10',  text: 'text-violet-400',  label: 'Thinking',   icon: <Brain className="w-3 h-3" /> },
  // ── Gemini (purple) ──
  gemini_request_started:   { dot: 'bg-fuchsia-400', bg: 'bg-fuchsia-400/10', text: 'text-fuchsia-400', label: 'Gemini ▸',   icon: <Cpu className="w-3 h-3" /> },
  gemini_request_completed: { dot: 'bg-fuchsia-500', bg: 'bg-fuchsia-500/10', text: 'text-fuchsia-400', label: 'Gemini ◂',   icon: <Cpu className="w-3 h-3" /> },
  // ── GPT / Codex (lime-green) ──
  codex_request_started:    { dot: 'bg-lime-400',    bg: 'bg-lime-400/10',    text: 'text-lime-400',    label: 'GPT ▸',      icon: <Zap className="w-3 h-3" /> },
  codex_request_completed:  { dot: 'bg-lime-500',    bg: 'bg-lime-500/10',    text: 'text-lime-400',    label: 'GPT ◂',      icon: <Zap className="w-3 h-3" /> },
  // ── Code (yellow) ──
  git_commit:               { dot: 'bg-yellow-400',  bg: 'bg-yellow-400/10',  text: 'text-yellow-400',  label: 'Commit',     icon: <GitCommit className="w-3 h-3" /> },
  // ── Flow ──
  task_lifecycle:           { dot: 'bg-sky-400',     bg: 'bg-sky-400/10',     text: 'text-sky-400',     label: 'Task',       icon: <Activity className="w-3 h-3" /> },
  question_created:         { dot: 'bg-amber-400',   bg: 'bg-amber-400/10',   text: 'text-amber-400',   label: 'Question',   icon: <MessageSquare className="w-3 h-3" /> },
  question_resolved:        { dot: 'bg-amber-300',   bg: 'bg-amber-300/10',   text: 'text-amber-300',   label: 'Resolved',   icon: <MessageSquare className="w-3 h-3" /> },
  decision_made:            { dot: 'bg-orange-400',  bg: 'bg-orange-400/10',  text: 'text-orange-400',  label: 'Decision',   icon: <GitBranch className="w-3 h-3" /> },
  insight_generated:        { dot: 'bg-emerald-400', bg: 'bg-emerald-400/10', text: 'text-emerald-400', label: 'Insight',    icon: <Sparkles className="w-3 h-3" /> },
  // ── Board (indigo) ──
  board_task_created:       { dot: 'bg-indigo-400',  bg: 'bg-indigo-400/10',  text: 'text-indigo-400',  label: 'Created',    icon: <Activity className="w-3 h-3" /> },
  board_task_status_changed:{ dot: 'bg-indigo-300',  bg: 'bg-indigo-300/10',  text: 'text-indigo-300',  label: 'Status',     icon: <Activity className="w-3 h-3" /> },
  board_task_note_added:    { dot: 'bg-indigo-200',  bg: 'bg-indigo-200/10',  text: 'text-indigo-200',  label: 'Note',       icon: <MessageSquare className="w-3 h-3" /> },
  board_task_claimed:       { dot: 'bg-indigo-500',  bg: 'bg-indigo-500/10',  text: 'text-indigo-400',  label: 'Claimed',    icon: <Wrench className="w-3 h-3" /> },
  board_task_deleted:       { dot: 'bg-red-400',     bg: 'bg-red-400/10',     text: 'text-red-400',     label: 'Deleted',    icon: <AlertTriangle className="w-3 h-3" /> },
  board_task_updated:       { dot: 'bg-indigo-300',  bg: 'bg-indigo-300/10',  text: 'text-indigo-300',  label: 'Board',      icon: <Activity className="w-3 h-3" /> },
  // ── System (slate/cyan/rose) ──
  slot_state_changed:       { dot: 'bg-slate-400',   bg: 'bg-slate-400/10',   text: 'text-slate-400',   label: 'Slot',       icon: <Settings2 className="w-3 h-3" /> },
  memory_phase_changed:     { dot: 'bg-cyan-400',    bg: 'bg-cyan-400/10',    text: 'text-cyan-400',    label: 'Memory',     icon: <Brain className="w-3 h-3" /> },
  briefing_batch_started:   { dot: 'bg-rose-300',    bg: 'bg-rose-300/10',    text: 'text-rose-300',    label: 'Briefing',   icon: <Sparkles className="w-3 h-3" /> },
  briefing_summary_generated: { dot: 'bg-rose-400',  bg: 'bg-rose-400/10',    text: 'text-rose-400',    label: 'Summary',    icon: <Sparkles className="w-3 h-3" /> },
  system_message:           { dot: 'bg-slate-400',   bg: 'bg-slate-400/10',   text: 'text-slate-400',   label: 'Daemon',     icon: <Terminal className="w-3 h-3" /> },
  slot_task_dispatched:     { dot: 'bg-amber-400',   bg: 'bg-amber-400/10',   text: 'text-amber-400',   label: 'Dispatch',   icon: <ArrowRight className="w-3 h-3" /> },
  // ── Translation Worker (indigo) ──
  translation_started:      { dot: 'bg-indigo-400',  bg: 'bg-indigo-400/10',  text: 'text-indigo-400',  label: 'Translating', icon: <Languages className="w-3 h-3" /> },
  translation_completed:    { dot: 'bg-indigo-500',  bg: 'bg-indigo-500/10',  text: 'text-indigo-400',  label: 'Translated',  icon: <CheckCheck className="w-3 h-3" /> },
  translation_failed:       { dot: 'bg-red-400',     bg: 'bg-red-400/10',     text: 'text-red-400',     label: 'Trans Err',   icon: <AlertTriangle className="w-3 h-3" /> },
};

// Slot color coding — fixed colors per slot for visual consistency
const SLOT_COLORS: Record<string, { badge: string; border: string }> = {
  'slot-coder-1':      { badge: 'bg-blue-500/20 text-blue-300',    border: 'border-l-blue-400' },
  'slot-coder-bypass': { badge: 'bg-indigo-500/20 text-indigo-300', border: 'border-l-indigo-400' },
  'slot-deploy-1':     { badge: 'bg-orange-500/20 text-orange-300', border: 'border-l-orange-400' },
  'slot-memory':       { badge: 'bg-cyan-500/20 text-cyan-300',    border: 'border-l-cyan-400' },
  'slot-memory-slow':  { badge: 'bg-teal-500/20 text-teal-300',    border: 'border-l-teal-400' },
};

function getSlotColor(slotId: string | null) {
  if (!slotId) return null;
  return SLOT_COLORS[slotId] || { badge: 'bg-neutral-500/20 text-neutral-300', border: 'border-l-neutral-400' };
}

const SWIMLANES = [
  { id: 'chat',    label: 'Chat',      types: ['user_message', 'assistant_message', 'thinking_message'] },
  { id: 'slot',    label: 'Slot',      types: ['slot_task_dispatched', 'system_message'] },
  { id: 'ai',      label: 'AI / LLM',  types: ['gemini_request_started', 'gemini_request_completed', 'decision_made', 'insight_generated'] },
  { id: 'gpt',     label: 'GPT',       types: ['codex_request_started', 'codex_request_completed'] },
  { id: 'code',    label: 'Code',      types: ['git_commit'] },
  { id: 'flow',    label: 'Flow',      types: ['task_lifecycle', 'question_created', 'question_resolved'] },
  { id: 'board',   label: 'Board',     types: ['board_task_created', 'board_task_status_changed', 'board_task_note_added', 'board_task_claimed', 'board_task_deleted', 'board_task_updated'] },
  { id: 'sys',     label: 'System',    types: ['slot_state_changed', 'memory_phase_changed', 'briefing_batch_started', 'briefing_summary_generated'] },
];

const QUICK_FILTERS = [
  { label: 'All', value: 'all' },
  { label: 'Chat', value: 'chat' },
  { label: 'Slot', value: 'slot' },
  { label: 'Errors', value: 'errors' },
  { label: 'Insights', value: 'insights' },
  { label: 'Gemini', value: 'gemini' },
  { label: 'GPT', value: 'gpt' },
];

const WINDOW_OPTIONS = [
  { label: '5m', value: '5min' },
  { label: '10m', value: '10min' },
  { label: '30m', value: '30min' },
  { label: '1h', value: '1h' },
  { label: '6h', value: '6h' },
  { label: '24h', value: '24h' },
];

// ── Helpers ────────────────────────────────────────────────

// Time helpers delegated to @/lib/time (single source of truth)

function shortTrace(id: string | null): string {
  if (!id) return '';
  return id.slice(0, 7);
}

function getEventColor(type: string) {
  return EVENT_COLORS[type] || { dot: 'bg-neutral-500', bg: 'bg-neutral-500/10', text: 'text-neutral-400', label: type, icon: <Zap className="w-3 h-3" /> };
}

const SLOT_LANE_IDX = SWIMLANES.findIndex(s => s.id === 'slot');

function getSwimlane(ev: TimelineEvent): number {
  // Any event with slot_id → Slot lane (sub-rowed by slot_id)
  if (ev.payload?.slot_id) return SLOT_LANE_IDX;

  for (let i = 0; i < SWIMLANES.length; i++) {
    if (SWIMLANES[i].types.includes(ev.event_type)) return i;
  }
  return SWIMLANES.length - 1; // default to system
}

/** Convert window string like "10min", "1h", "24h" to milliseconds */
function windowToMs(w: string): number {
  const minMatch = w.match(/^(\d+)min$/);
  if (minMatch) return parseInt(minMatch[1], 10) * 60 * 1000;
  const hMatch = w.match(/^(\d+)h$/);
  if (hMatch) return parseInt(hMatch[1], 10) * 3600 * 1000;
  const dMatch = w.match(/^(\d+)d$/);
  if (dMatch) return parseInt(dMatch[1], 10) * 86400 * 1000;
  return 3600 * 1000; // fallback 1h
}

// Session color palette for Chat lane — visually distinct, dark-theme friendly
const SESSION_COLORS = [
  { dot: 'bg-cyan-400',    line: 'rgba(34,211,238,0.25)',  ring: 'ring-cyan-400/40' },
  { dot: 'bg-green-400',   line: 'rgba(74,222,128,0.25)',  ring: 'ring-green-400/40' },
  { dot: 'bg-amber-400',   line: 'rgba(251,191,36,0.25)',  ring: 'ring-amber-400/40' },
  { dot: 'bg-pink-400',    line: 'rgba(244,114,182,0.25)', ring: 'ring-pink-400/40' },
  { dot: 'bg-violet-400',  line: 'rgba(167,139,250,0.25)', ring: 'ring-violet-400/40' },
  { dot: 'bg-orange-400',  line: 'rgba(251,146,60,0.25)',  ring: 'ring-orange-400/40' },
  { dot: 'bg-teal-300',    line: 'rgba(94,234,212,0.25)',  ring: 'ring-teal-300/40' },
  { dot: 'bg-rose-400',    line: 'rgba(251,113,133,0.25)', ring: 'ring-rose-400/40' },
];

function hashSessionColor(sessionId: string): number {
  let h = 0;
  for (let i = 0; i < sessionId.length; i++) {
    h = ((h << 5) - h + sessionId.charCodeAt(i)) | 0;
  }
  return ((h % SESSION_COLORS.length) + SESSION_COLORS.length) % SESSION_COLORS.length;
}

function isChatEvent(type: string): boolean {
  return type === 'user_message' || type === 'assistant_message' || type === 'thinking_message';
}

function eventSummary(ev: TimelineEvent): string {
  if (ev.summary) return ev.summary;
  const p = ev.payload;
  if (!p) return ev.event_type;
  switch (ev.event_type) {
    case 'slot_state_changed': return `${p.slot_id || ''} → ${p.new_state || ''}`;
    case 'task_lifecycle': return `${p.action || ''}: ${p.task_title || p.task_id || ''}`;
    case 'gemini_request_started': return `${p.caller || ''} → ${p.model || ''} (${p.prompt_chars || 0} chars)`;
    case 'gemini_request_completed': return `${p.caller || ''} ${p.duration_ms ? p.duration_ms + 'ms' : ''} ${p.error ? '❌' : ''}`;
    case 'codex_request_started': return `${p.caller || ''} → ${p.model || ''} (${p.prompt_chars || 0}ch${p.has_image ? ' +img' : ''})`;
    case 'codex_request_completed': return `${p.caller || ''} ${p.duration_ms ? p.duration_ms + 'ms' : ''} ${p.error ? '❌' : '✓'} ${p.output_tokens ? p.output_tokens + 'tok' : ''}`;
    case 'git_commit': return `${p.short_hash || ''} ${p.message || ''}`;
    case 'user_message': return `${p.preview || ''}`;
    case 'assistant_message': return `${p.preview || ''}`;
    case 'decision_made': return `${p.tier || ''}: ${p.question?.slice(0, 60) || ''}`;
    case 'insight_generated': return `${p.title || ''}`;
    case 'board_task_created': return `Created: ${p.title || ''}`;
    case 'board_task_status_changed': return `${p.old_status || ''} → ${p.new_status || ''}`;
    case 'board_task_note_added': return `Note: ${p.content_preview || ''}`;
    case 'board_task_claimed': return `Claimed by ${p.slot_id || ''}`;
    case 'board_task_deleted': return `Deleted: ${p.title || ''}`;
    case 'board_task_updated': return `${p.title || ''} → ${p.status || ''}`;
    case 'briefing_batch_started': return `Briefing: ${p.pending_count || 0} pending`;
    case 'briefing_summary_generated': return `seq=${p.target_seq}: ${p.summary || ''}`;
    case 'system_message': return `[Daemon] ${p.preview || ''}`;
    case 'slot_task_dispatched': return `→ ${p.slot_id || ''} [${p.purpose || ''}] ${p.preview || ''}`;
    case 'translation_started': return `Translating msg#${p.message_id} (${p.content_chars || 0}ch)`;
    case 'translation_completed': return `Translated msg#${p.message_id} (${p.duration_ms || 0}ms): ${p.preview || ''}`;
    case 'translation_failed': return `Translation failed msg#${p.message_id}: ${p.error || ''}`;
    default: return ev.event_type;
  }
}

function hasError(ev: TimelineEvent): boolean {
  if (!ev.payload) return false;
  return !!ev.payload.error || !!ev.payload.error_msg || ev.payload.status === 'error';
}

interface FullMessage {
  content: string;
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  contentBlocks: any[] | null;
  imageCount: number;
  translation?: string | null;
}

/** Fetch full message content + raw_content blocks. Auto-fetches when `enabled` is true. */
function useFullMessage(messageId: number | undefined, enabled: boolean): FullMessage | null {
  const [msg, setMsg] = useState<FullMessage | null>(null);
  useEffect(() => {
    if (!messageId || !enabled) return;
    let cancelled = false;
    fetch(`/api/system/conversation-message?message_id=${messageId}`)
      .then(r => r.json())
      .then(data => {
        if (cancelled || !data?.content) return;
        let blocks: FullMessage['contentBlocks'] = null;
        let imageCount = 0;
        if (data.raw_content) {
          try {
            blocks = JSON.parse(data.raw_content);
            if (Array.isArray(blocks)) {
              // Count images and strip base64 data to avoid bloating React state
              for (const block of blocks) {
                if (block?.type === 'image') {
                  imageCount++;
                  if (block.source?.data) block.source.data = '[stripped]';
                }
              }
            }
          } catch { /* ignore */ }
        }
        setMsg({ content: data.content, contentBlocks: blocks, imageCount, translation: data.translation || null });
      })
      .catch(() => {});
    return () => { cancelled = true; };
  }, [messageId, enabled]);
  return msg;
}

// ── Main Component ─────────────────────────────────────────

export function CognitiveTimeline() {
  const timelineVersion = useEventInvalidation('timeline');

  const [events, setEvents] = useState<TimelineEvent[]>([]);
  const [sessionsMeta, setSessionsMeta] = useState<Record<string, { startedAt: string }>>({});
  const [stats, setStats] = useState<TimelineStats | null>(null);
  const [hourlyStats, setHourlyStats] = useState<TimelineStats | null>(null);
  const [selectedEvent, setSelectedEvent] = useState<TimelineEvent | null>(null);
  const [traceEvents, setTraceEvents] = useState<TimelineEvent[]>([]);
  const [quickFilter, setQuickFilter] = useState('all');
  const [searchInput, setSearchInput] = useState('');
  const [searchQuery, setSearchQuery] = useState(''); // Only updated on Enter — drives data fetching
  const [activeWindow, setActiveWindow] = useState('24h');
  const [loading, setLoading] = useState(false);
  const [selectionSource, setSelectionSource] = useState<'timeline' | 'list' | null>(null);

  // Ref maps for bidirectional scroll sync
  const timelineDotRefs = useRef<Map<number, HTMLButtonElement>>(new Map());
  const traceItemRefs = useRef<Map<number, HTMLButtonElement>>(new Map());

  // Gesture-controlled timeline: pan (two-finger scroll) + zoom (pinch)
  const now = Date.now();
  const { containerRef: timelineRef, timeRange, setTimeRange: setGestureRange } = useTimelineGestures({
    initialRange: { min: now - windowToMs('24h'), max: now },
  });

  // Track the time range we already have data for — avoid redundant fetches
  const loadedRangeRef = useRef<{ min: number; max: number } | null>(null);
  const abortRef = useRef<AbortController | null>(null);

  // Fetch events and merge with existing (deduplicate by seq, no pop-in)
  const fetchForRange = useCallback(async (
    rangeMin: number, rangeMax: number,
    opts?: { signal?: AbortSignal; silent?: boolean; replace?: boolean },
  ) => {
    const duration = rangeMax - rangeMin;
    // Fetch 2× buffer on each side so preloaded events are ready before visible
    const bufMin = rangeMin - duration;
    const bufMax = rangeMax + duration;
    const since = new Date(bufMin).toISOString().replace('T', ' ').slice(0, 19);
    const until = new Date(bufMax).toISOString().replace('T', ' ').slice(0, 19);

    const params = new URLSearchParams({ since, until });
    if (searchQuery) params.set('query', searchQuery);

    const [evRes, stRes, hrRes] = await Promise.allSettled([
      fetch(`/api/timeline/events?${params}`, { signal: opts?.signal }).then(r => r.json()),
      fetch(`/api/timeline/stats?since=${since}&until=${until}`, { signal: opts?.signal }).then(r => r.json()),
      fetch(`/api/timeline/stats?window=1h`, { signal: opts?.signal }).then(r => r.json()),
    ]);

    if (evRes.status === 'fulfilled') {
      const raw = evRes.value.events;
      const incoming: TimelineEvent[] = Array.isArray(raw) ? raw : (raw?.events || []);
      if (opts?.replace) {
        setEvents(incoming);
      } else {
        // Merge: deduplicate by seq, keep newest data for each event
        setEvents(prev => {
          const map = new Map<number, TimelineEvent>();
          for (const ev of prev) map.set(ev.seq, ev);
          for (const ev of incoming) map.set(ev.seq, ev);
          return Array.from(map.values()).sort((a, b) => (b.seq ?? 0) - (a.seq ?? 0));
        });
      }
      if (evRes.value.sessions) {
        setSessionsMeta(prev => ({ ...prev, ...evRes.value.sessions }));
      }
    }
    if (stRes.status === 'fulfilled' && !stRes.value.error) setStats(stRes.value);
    if (hrRes.status === 'fulfilled' && !hrRes.value.error) setHourlyStats(hrRes.value);

    // Extend the loaded range (union of old + new)
    const prevLoaded = loadedRangeRef.current;
    loadedRangeRef.current = prevLoaded
      ? { min: Math.min(prevLoaded.min, bufMin), max: Math.max(prevLoaded.max, bufMax) }
      : { min: bufMin, max: bufMax };
  }, [searchQuery]);

  // Preload when visible range consumes >50% of loaded buffer on either side
  useEffect(() => {
    const { min, max } = timeRange;
    const loaded = loadedRangeRef.current;

    if (!loaded) {
      // Initial load — full fetch with loading indicator
      abortRef.current?.abort();
      const controller = new AbortController();
      abortRef.current = controller;
      setLoading(true);
      fetchForRange(min, max, { signal: controller.signal, replace: true })
        .catch(() => {})
        .finally(() => setLoading(false));
      return () => controller.abort();
    }

    // Check if visible range is approaching the edges of loaded buffer
    const bufferLeft = min - loaded.min;
    const bufferRight = loaded.max - max;
    const visibleDuration = max - min;
    const threshold = visibleDuration * 0.5;

    if (bufferLeft > threshold && bufferRight > threshold) return; // Plenty of buffer

    // Silent preload — no loading spinner, merge results
    abortRef.current?.abort();
    const controller = new AbortController();
    abortRef.current = controller;
    fetchForRange(min, max, { signal: controller.signal, silent: true }).catch(() => {});
    return () => controller.abort();
  }, [timeRange, fetchForRange, timelineVersion]);

  // In-place summary update: briefing worker sends target_seq + new summary via CustomEvent
  useEffect(() => {
    const handler = (e: Event) => {
      const { target_seq, summary } = (e as CustomEvent).detail;
      if (target_seq && summary) {
        setEvents(prev => prev.map(ev =>
          ev.seq === target_seq ? { ...ev, summary } : ev
        ));
      }
    };
    globalThis.addEventListener('timeline-summary-update', handler);
    return () => globalThis.removeEventListener('timeline-summary-update', handler);
  }, []);

  // Load trace events when clicking an event with trace_id
  const selectEvent = useCallback(async (ev: TimelineEvent, source: 'timeline' | 'list') => {
    setSelectionSource(source);
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

  // Bidirectional scroll sync: scroll the OTHER view when selection changes
  useEffect(() => {
    if (!selectedEvent || !selectionSource) return;
    const seq = selectedEvent.seq;

    if (selectionSource === 'timeline') {
      // Clicked timeline dot → scroll trace chain list to the active item
      const item = traceItemRefs.current.get(seq);
      if (item) item.scrollIntoView({ behavior: 'smooth', block: 'center' });
    } else if (selectionSource === 'list') {
      // Clicked list item → blink the timeline dot
      const dot = timelineDotRefs.current.get(seq);
      if (dot) {
        dot.scrollIntoView({ behavior: 'smooth', inline: 'center', block: 'nearest' });
        dot.classList.remove('animate-ping-radar');
        void dot.offsetWidth; // force reflow to re-trigger animation
        dot.classList.add('animate-ping-radar');
      }
    }
  }, [selectedEvent, selectionSource]);

  // Filtered events
  const filtered = useMemo(() => {
    let result = events;
    if (quickFilter === 'errors') result = result.filter(hasError);
    else if (quickFilter === 'chat') result = result.filter(e => e.event_type === 'user_message' || e.event_type === 'assistant_message' || e.event_type === 'thinking_message');
    else if (quickFilter === 'insights') result = result.filter(e => e.event_type === 'insight_generated');
    else if (quickFilter === 'gemini') result = result.filter(e => e.event_type === 'gemini_request_completed' || e.event_type === 'gemini_request_started');
    else if (quickFilter === 'gpt') result = result.filter(e => e.event_type === 'codex_request_started' || e.event_type === 'codex_request_completed');
    else if (quickFilter === 'slot') result = result.filter(e => e.event_type === 'slot_task_dispatched' || !!e.payload?.slot_id);
    return result;
  }, [events, quickFilter]);

  // Build session layout: sub-row assignment + capsule bounds for Chat lane
  const sessionLayout = useMemo(() => {
    // Group chat events by session_id
    const bySession = new Map<string, TimelineEvent[]>();
    for (const ev of filtered) {
      if (isChatEvent(ev.event_type) && ev.payload?.session_id && !ev.payload?.slot_id) {
        const sid = ev.payload.session_id;
        const arr = bySession.get(sid) || [];
        arr.push(ev);
        bySession.set(sid, arr);
      }
    }

    // Build session intervals sorted by start time
    // Use actual session startedAt from metadata so capsules span the full session lifetime
    const sessions: { id: string; parentId?: string; start: number; end: number; startedBefore: boolean; events: TimelineEvent[] }[] = [];
    for (const [sid, evts] of bySession) {
      const times = evts.map(e => utcMs(e.created_at));
      const evtMin = Math.min(...times);
      // Extract parent_session_id from payload — sub-agent sessions inherit parent color
      const parentId = evts.find(e => e.payload?.parent_session_id)?.payload?.parent_session_id;
      // Use session metadata startedAt if available (preserves capsule across time windows)
      const meta = sessionsMeta[sid];
      const actualStart = meta?.startedAt ? Math.min(utcMs(meta.startedAt), evtMin) : evtMin;
      const startedBefore = actualStart < evtMin;
      sessions.push({ id: sid, parentId, start: actualStart, end: Math.max(...times), startedBefore, events: evts });
    }
    sessions.sort((a, b) => a.start - b.start);

    // Greedy row assignment — pack sessions into fewest non-overlapping rows
    const rowEnds: number[] = []; // tracks end-time of last session in each row
    const layout = new Map<string, { colorIdx: number; row: number; startedBefore: boolean; events: TimelineEvent[] }>();
    for (const s of sessions) {
      let assigned = -1;
      for (let r = 0; r < rowEnds.length; r++) {
        if (s.start > rowEnds[r]) { assigned = r; break; }
      }
      if (assigned === -1) { assigned = rowEnds.length; rowEnds.push(0); }
      rowEnds[assigned] = s.end;
      layout.set(s.id, { colorIdx: hashSessionColor(s.parentId || s.id), row: assigned, startedBefore: s.startedBefore, events: s.events });
    }

    return { map: layout, rowCount: Math.max(rowEnds.length, 1) };
  }, [filtered, sessionsMeta]);

  // Build slot layout: group events by slot_id, assign fixed sub-row per slot
  const slotLayout = useMemo(() => {
    const bySlot = new Map<string, TimelineEvent[]>();
    for (const ev of filtered) {
      const sid = ev.payload?.slot_id;
      if (!sid) continue;
      const arr = bySlot.get(sid) || [];
      arr.push(ev);
      bySlot.set(sid, arr);
    }
    // Fixed row assignment by sorted slot_id for visual stability
    const slotIds = Array.from(bySlot.keys()).sort();
    const layout = new Map<string, { row: number; events: TimelineEvent[] }>();
    slotIds.forEach((sid, index) => {
      layout.set(sid, { row: index, events: bySlot.get(sid)! });
    });
    return { map: layout, rowCount: Math.max(slotIds.length, 1) };
  }, [filtered]);

  // Compute lane geometry — Chat + Slot lanes expand with concurrent sessions/slots
  const laneGeometry = useMemo(() => {
    const chatWeight = sessionLayout.rowCount;
    const slotWeight = slotLayout.rowCount;
    // Chat and Slot are dynamic, others are weight=1
    const totalWeight = chatWeight + slotWeight + (SWIMLANES.length - 2);
    const lanes: { top: number; height: number }[] = [];
    let offset = 0;
    for (const lane of SWIMLANES) {
      const w = lane.id === 'chat' ? chatWeight : lane.id === 'slot' ? slotWeight : 1;
      const h = (w / totalWeight) * 100;
      lanes.push({ top: offset, height: h });
      offset += h;
    }
    const chatLane = lanes[SWIMLANES.findIndex(s => s.id === 'chat')];
    const slotLane = lanes[SLOT_LANE_IDX];
    return {
      lanes,
      chatSubRowHeight: chatLane.height / sessionLayout.rowCount,
      slotSubRowHeight: slotLane.height / slotLayout.rowCount,
    };
  }, [sessionLayout, slotLayout]);

  // Get Y position for a chat event accounting for session sub-row
  const getChatY = useCallback((ev: TimelineEvent): number => {
    const sid = ev.payload?.session_id;
    const info = sid ? sessionLayout.map.get(sid) : undefined;
    const row = info?.row ?? 0;
    const chatLane = laneGeometry.lanes[SWIMLANES.findIndex(s => s.id === 'chat')];
    const subH = laneGeometry.chatSubRowHeight;
    return chatLane.top + subH * (row + 0.5);
  }, [sessionLayout, laneGeometry]);

  // Get Y position for a slot event accounting for slot_id sub-row
  const getSlotY = useCallback((ev: TimelineEvent): number => {
    const sid = ev.payload?.slot_id;
    const info = sid ? slotLayout.map.get(sid) : undefined;
    const row = info?.row ?? 0;
    const slotLane = laneGeometry.lanes[SLOT_LANE_IDX];
    const subH = laneGeometry.slotSubRowHeight;
    return slotLane.top + subH * (row + 0.5);
  }, [slotLayout, laneGeometry]);

  // Get Y position for any event
  const getY = useCallback((ev: TimelineEvent): number => {
    // Slot events (with slot_id) → slot sub-row layout
    if (ev.payload?.slot_id) return getSlotY(ev);
    // Master chat events → chat sub-row layout
    if (isChatEvent(ev.event_type)) return getChatY(ev);
    // Everything else → center of its lane
    const laneIdx = getSwimlane(ev);
    const lane = laneGeometry.lanes[laneIdx];
    return lane.top + lane.height * 0.5;
  }, [getChatY, getSlotY, laneGeometry]);

  // Sync window preset buttons with gesture-controlled timeRange
  const handleWindowChange = useCallback((w: string) => {
    setActiveWindow(w);
    const n = Date.now();
    loadedRangeRef.current = null; // Force refetch for new window
    setGestureRange({ min: n - windowToMs(w), max: n });
  }, [setGestureRange]);

  // Position calculation — only used for debounced/non-critical paths (span lines, trace lines)
  const getX = useCallback((dateStr: string) => {
    const t = utcMs(dateStr);
    const { min, max } = timeRange;
    const range = max - min;
    if (range <= 0) return 50;
    return ((t - min) / range) * 100;
  }, [timeRange]);

  // CSS calc()-driven position: elements set --t-event inline, container provides --t-min/--t-range.
  // The browser repositions all elements via CSS without React re-render during gestures.
  const cssLeft = 'calc((var(--t-event) - var(--t-min)) / var(--t-range) * 100%)';

  // Time axis ticks
  const timeTicks = useMemo(() => {
    const { min, max } = timeRange;
    const range = max - min;
    const count = 6;
    return Array.from({ length: count + 1 }, (_, i) => {
      const t = min + (range * i) / count;
      return { pos: (i / count) * 100, label: formatBeijingTime(new Date(t).toISOString()) };
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
      const t = utcMs(ev.created_at);
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
          <div className="w-px h-6 bg-neutral-800" />
          <StatCard
            label="Gemini/h"
            value={hourlyStats?.by_type?.find(t => t[0] === 'gemini_request_completed')?.[1] ?? 0}
            color="text-purple-400"
          />
          <StatCard
            label="GPT/h"
            value={hourlyStats?.by_type?.find(t => t[0] === 'codex_request_completed')?.[1] ?? 0}
            color="text-sky-400"
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
              value={searchInput}
              onChange={(e) => setSearchInput(e.target.value)}
              onKeyDown={(e) => { if (e.key === 'Enter') { setSearchQuery(searchInput); loadedRangeRef.current = null; } }}
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
                onClick={() => handleWindowChange(w.value)}
                className={cn(
                  'px-2 py-1 text-[10px] font-medium rounded transition-colors',
                  activeWindow === w.value ? 'bg-neutral-800 text-white' : 'text-neutral-500 hover:text-neutral-300',
                )}
              >
                {w.label}
              </button>
            ))}
          </div>

          {/* Zoom level indicator — shows effective visible window */}
          {(() => {
            const visibleMs = timeRange.max - timeRange.min;
            const label = visibleMs < 60_000 ? `${Math.round(visibleMs / 1000)}s`
              : visibleMs < 3600_000 ? `${Math.round(visibleMs / 60_000)}m`
              : visibleMs < 86400_000 ? `${(visibleMs / 3600_000).toFixed(1)}h`
              : `${(visibleMs / 86400_000).toFixed(1)}d`;
            return (
              <span className="text-[10px] text-neutral-600 tabular-nums" title="Visible time window (pinch to zoom)">
                {label}
              </span>
            );
          })()}

          <button onClick={() => { loadedRangeRef.current = null; setLoading(true); fetchForRange(timeRange.min, timeRange.max, { replace: true }).catch(() => {}).finally(() => setLoading(false)); }} className="p-1.5 rounded hover:bg-neutral-800 text-neutral-500 hover:text-neutral-300 transition-colors">
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
            {SWIMLANES.map((lane, i) => {
              const geo = laneGeometry.lanes[i];
              return (
                <div key={lane.id} className={cn(
                  'flex items-center justify-center text-[10px] text-neutral-500 font-medium',
                  i < SWIMLANES.length - 1 && 'border-b border-neutral-800/50',
                )} style={{ height: `${geo.height}%` }}>
                  {lane.label}
                </div>
              );
            })}
          </div>

          {/* Timeline canvas — gesture-controlled: scroll to pan, pinch to zoom */}
          <div ref={timelineRef} className="flex-1 relative overflow-hidden touch-none cursor-grab active:cursor-grabbing" style={{ minHeight: 100 + sessionLayout.rowCount * 28 }}>
            {/* Swimlane backgrounds */}
            {SWIMLANES.map((lane, i) => {
              const geo = laneGeometry.lanes[i];
              return (
                <div
                  key={lane.id}
                  className={cn(
                    'absolute left-0 right-0',
                    i < SWIMLANES.length - 1 && 'border-b border-neutral-800/30',
                  )}
                  style={{ top: `${geo.top}%`, height: `${geo.height}%` }}
                />
              );
            })}

            {/* Session capsules for Chat lane — positioned via CSS calc() */}
            {Array.from(sessionLayout.map.entries()).map(([sid, info]) => {
              const sc = SESSION_COLORS[info.colorIdx];
              const times = info.events.map(e => utcMs(e.created_at));
              const evtMin = Math.min(...times);
              const evtMax = Math.max(...times);
              const meta = sessionsMeta[sid];
              const actualStart = meta?.startedAt ? Math.min(utcMs(meta.startedAt), evtMin) : evtMin;
              const chatTop = laneGeometry.lanes[0].top;
              const subH = laneGeometry.chatSubRowHeight;
              const yCenter = chatTop + subH * (info.row + 0.5);
              const capsuleH = subH * 0.7;
              return (
                <div
                  key={`capsule-${sid}`}
                  className="absolute pointer-events-none z-[1]"
                  style={{
                    '--t-start': actualStart,
                    '--t-end': evtMax,
                    left: 'calc((var(--t-start) - var(--t-min)) / var(--t-range) * 100%)',
                    width: 'calc(max((var(--t-end) - var(--t-start)) / var(--t-range) * 100%, 1.2%))',
                    top: `${yCenter - capsuleH / 2}%`,
                    height: `${capsuleH}%`,
                    backgroundColor: sc.line,
                    border: `1px solid ${sc.line.replace('0.25)', '0.45)')}`,
                    borderRadius: '10px',
                  } as React.CSSProperties}
                />
              );
            })}

            {/* Event dots — positioned via CSS calc() driven by --t-min/--t-range on container */}
            {filtered.map((ev) => {
              const y = getY(ev);
              const ec = getEventColor(ev.event_type);
              const isSelected = selectedEvent?.seq === ev.seq;
              const isInsight = ev.event_type === 'insight_generated';
              const isError = hasError(ev);
              const isChat = isChatEvent(ev.event_type);
              const sessionInfo = isChat && ev.payload?.session_id ? sessionLayout.map.get(ev.payload.session_id) : undefined;
              const sessionColor = sessionInfo ? SESSION_COLORS[sessionInfo.colorIdx] : null;

              return (
                <button
                  key={ev.seq}
                  ref={(el) => { if (el) timelineDotRefs.current.set(ev.seq, el); else timelineDotRefs.current.delete(ev.seq); }}
                  onClick={() => selectEvent(ev, 'timeline')}
                  className={cn(
                    'absolute -translate-x-1/2 -translate-y-1/2 rounded-full z-10 transition-[transform,box-shadow,background-color] duration-200 ease-out',
                    isInsight ? 'w-4 h-4 ring-2 ring-emerald-400/40' :
                    isError ? 'w-3 h-3 ring-2 ring-red-500/40' :
                    'w-2.5 h-2.5 hover:w-3.5 hover:h-3.5',
                    sessionColor ? sessionColor.dot : ec.dot,
                    isSelected && 'ring-2 ring-white/60 w-4 h-4',
                  )}
                  style={{ '--t-event': utcMs(ev.created_at), left: cssLeft, top: `${y}%` } as React.CSSProperties}
                  title={`${ec.label}: ${eventSummary(ev)}${isChat && ev.payload?.session_id ? `\nSession: ${ev.payload.session_id.slice(0, 8)}` : ''}\n${formatBeijingTime(ev.created_at)}`}
                />
              );
            })}

            {/* Span pair lines — connect started↔completed sharing same span_id */}
            {(() => {
              const spanPairs = new Map<string, TimelineEvent[]>();
              for (const ev of filtered) {
                if (ev.span_id && (ev.event_type === 'gemini_request_started' || ev.event_type === 'gemini_request_completed' || ev.event_type === 'codex_request_started' || ev.event_type === 'codex_request_completed')) {
                  const arr = spanPairs.get(ev.span_id) || [];
                  arr.push(ev);
                  spanPairs.set(ev.span_id, arr);
                }
              }
              return Array.from(spanPairs.values())
                .filter(pair => pair.length === 2)
                .map(pair => {
                  const [a, b] = pair.sort((x, y) => utcMs(x.created_at) - utcMs(y.created_at));
                  const x1 = getX(a.created_at);
                  const x2 = getX(b.created_at);
                  const y = getY(a);
                  const isActive = selectedEvent && (selectedEvent.span_id === a.span_id);
                  const isCodex = a.event_type.startsWith('codex_');
                  const color = isCodex
                    ? (isActive ? 'rgba(56,189,248,0.5)' : 'rgba(56,189,248,0.15)')
                    : (isActive ? 'rgba(168,85,247,0.5)' : 'rgba(168,85,247,0.15)');
                  return (
                    <svg key={`span-${a.span_id}`} className="absolute inset-0 w-full h-full pointer-events-none z-0" preserveAspectRatio="none">
                      <line
                        x1={`${x1}%`} y1={`${y}%`}
                        x2={`${x2}%`} y2={`${y}%`}
                        stroke={color}
                        strokeWidth={isActive ? '2' : '1'}
                        strokeDasharray="3 2"
                      />
                    </svg>
                  );
                });
            })()}

            {/* Trace connecting lines — show when event selected */}
            {selectedEvent?.trace_id && traceEvents.length > 1 && (() => {
              const sorted = [...traceEvents].sort((a, b) => (utcMs(a.created_at) - utcMs(b.created_at)) || (a.seq - b.seq));
              return sorted.slice(0, -1).map((ev, i) => {
                const next = sorted[i + 1];
                const x1 = getX(ev.created_at);
                const x2 = getX(next.created_at);
                const y1 = getY(ev);
                const y2 = getY(next);
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
                      .sort((a, b) => (utcMs(a.created_at) - utcMs(b.created_at)) || (a.seq - b.seq))
                      .map(tev => {
                        const ec = getEventColor(tev.event_type);
                        const isActive = tev.seq === selectedEvent.seq;
                        return (
                          <button
                            key={tev.seq}
                            ref={(el) => { if (el) traceItemRefs.current.set(tev.seq, el); else traceItemRefs.current.delete(tev.seq); }}
                            onClick={() => selectEvent(tev, 'list')}
                            className={cn(
                              'w-full text-left px-2 py-1.5 rounded text-xs flex items-center gap-2 transition-colors',
                              isActive ? 'bg-neutral-800 text-white' : 'text-neutral-400 hover:bg-neutral-800/50',
                            )}
                          >
                            <div className={cn('w-2 h-2 rounded-full shrink-0', ec.dot)} />
                            <span className={cn('truncate', ec.text)}>{ec.label}</span>
                            <span className="text-neutral-600 text-[10px] ml-auto">{formatBeijingTime(tev.created_at)}</span>
                          </button>
                        );
                      })}
                  </div>
                </div>
              )}
            </div>

            {/* Right: payload */}
            <div className="flex-1 p-3 overflow-y-auto min-w-0">
              <EventPayload event={selectedEvent} filteredEvents={filtered} onNavigate={(ev: TimelineEvent) => selectEvent(ev, 'list')} />
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
        {event.payload?.slot_id && (() => {
          const sc = getSlotColor(event.payload.slot_id);
          return sc ? <span className={cn('text-[10px] px-1.5 py-0.5 rounded', sc.badge)}>{event.payload.slot_id}</span> : null;
        })()}
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
        <MetaRow label="Time" value={formatBeijing(event.created_at)} />
        <MetaRow label="Seq" value={`#${event.seq}`} />
        {event.payload?.session_id && <MetaRow label="Session" value={shortTrace(event.payload.session_id)} mono />}
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

function GeminiContentPanel({ requestId, isResponse }: { requestId: string; isResponse?: boolean }) {
  const [content, setContent] = useState<{ prompt_text?: string; response_text?: string } | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [promptOpen, setPromptOpen] = useState(!isResponse);
  const [responseOpen, setResponseOpen] = useState(true);

  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    setError(null);
    fetch(`/api/system/gemini-content?request_id=${encodeURIComponent(requestId)}`)
      .then(r => r.json())
      .then(data => {
        if (!cancelled) {
          if (data.error) setError(data.error);
          else setContent(data);
        }
      })
      .catch(e => { if (!cancelled) setError(String(e)); })
      .finally(() => { if (!cancelled) setLoading(false); });
    return () => { cancelled = true; };
  }, [requestId]);

  if (loading) return <div className="text-[11px] text-neutral-500 animate-pulse">Loading content...</div>;
  if (error) return <div className="text-[11px] text-red-400">Failed: {error}</div>;
  if (!content) return null;

  return (
    <div className="space-y-3">
      {content.prompt_text && (
        <div>
          <button
            onClick={() => setPromptOpen(v => !v)}
            className="flex items-center gap-1 text-[9px] text-neutral-500 uppercase tracking-wider mb-1 hover:text-neutral-300 transition-colors"
          >
            {promptOpen ? <ChevronDown className="w-3 h-3" /> : <ChevronRight className="w-3 h-3" />}
            Prompt
          </button>
          {promptOpen && (
            <pre className="text-[11px] text-purple-300/80 font-mono bg-neutral-900 rounded p-3 overflow-auto max-h-60 whitespace-pre-wrap break-words leading-relaxed">
              {content.prompt_text}
            </pre>
          )}
        </div>
      )}
      {content.response_text && (
        <div>
          <button
            onClick={() => setResponseOpen(v => !v)}
            className="flex items-center gap-1 text-[9px] text-neutral-500 uppercase tracking-wider mb-1 hover:text-neutral-300 transition-colors"
          >
            {responseOpen ? <ChevronDown className="w-3 h-3" /> : <ChevronRight className="w-3 h-3" />}
            Response
          </button>
          {responseOpen && (
            <pre className="text-[11px] text-emerald-300/80 font-mono bg-neutral-900 rounded p-3 overflow-auto max-h-60 whitespace-pre-wrap break-words leading-relaxed">
              {content.response_text}
            </pre>
          )}
        </div>
      )}
    </div>
  );
}

function EventPayload({ event, filteredEvents, onNavigate }: {
  event: TimelineEvent;
  filteredEvents: TimelineEvent[];
  onNavigate: (ev: TimelineEvent) => void;
}) {
  const [tab, setTab] = useState<'summary' | 'payload'>('summary');

  const currentIndex = useMemo(
    () => filteredEvents.findIndex(e => e.seq === event.seq),
    [filteredEvents, event.seq],
  );
  const total = filteredEvents.length;
  const hasPrev = currentIndex > 0;
  const hasNext = currentIndex >= 0 && currentIndex < total - 1;

  const goPrev = useCallback(() => { if (hasPrev) onNavigate(filteredEvents[currentIndex - 1]); }, [hasPrev, filteredEvents, currentIndex, onNavigate]);
  const goNext = useCallback(() => { if (hasNext) onNavigate(filteredEvents[currentIndex + 1]); }, [hasNext, filteredEvents, currentIndex, onNavigate]);

  // J/K and arrow key navigation
  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if (['INPUT', 'TEXTAREA', 'SELECT'].includes((e.target as HTMLElement).tagName)) return;
      if (e.key === 'j' || e.key === 'ArrowDown') { e.preventDefault(); goNext(); }
      if (e.key === 'k' || e.key === 'ArrowUp') { e.preventDefault(); goPrev(); }
    };
    window.addEventListener('keydown', handler);
    return () => window.removeEventListener('keydown', handler);
  }, [goNext, goPrev]);

  return (
    <div className="flex flex-col h-full">
      {/* Tabs + Navigation */}
      <div className="flex items-center gap-1 mb-3 border-b border-neutral-800 pb-2">
        <button
          onClick={() => setTab('summary')}
          className={cn(
            'px-3 py-1.5 text-xs font-medium rounded-md transition-colors',
            tab === 'summary' ? 'bg-neutral-800 text-white' : 'text-neutral-500 hover:text-neutral-300 hover:bg-neutral-800/50',
          )}
        >
          Summary
        </button>
        <button
          onClick={() => setTab('payload')}
          className={cn(
            'px-3 py-1.5 text-xs font-medium rounded-md transition-colors',
            tab === 'payload' ? 'bg-neutral-800 text-white' : 'text-neutral-500 hover:text-neutral-300 hover:bg-neutral-800/50',
          )}
        >
          Payload
        </button>

        {/* Navigation: prev / index / next */}
        <div className="ml-auto flex items-center gap-1.5 text-[10px]">
          <button
            onClick={goPrev}
            disabled={!hasPrev}
            className="p-1 rounded hover:bg-neutral-800 disabled:opacity-30 text-neutral-400 hover:text-white transition-colors"
            title="Previous (K / ↑)"
          >
            <ArrowUp className="w-3.5 h-3.5" />
          </button>
          <span className="text-neutral-500 font-mono tabular-nums min-w-[4ch] text-center">
            {currentIndex >= 0 ? `${currentIndex + 1}/${total}` : '—'}
          </span>
          <button
            onClick={goNext}
            disabled={!hasNext}
            className="p-1 rounded hover:bg-neutral-800 disabled:opacity-30 text-neutral-400 hover:text-white transition-colors"
            title="Next (J / ↓)"
          >
            <ArrowDown className="w-3.5 h-3.5" />
          </button>
        </div>
      </div>

      <div className="flex-1 min-h-0 overflow-y-auto">
        {tab === 'summary' ? (
          <EventSummaryView event={event} />
        ) : (
          <JsonTreeViewer data={event.payload} />
        )}
      </div>
    </div>
  );
}

// ── Summary Views (per event type) ───────────────────────────

function EventSummaryView({ event }: { event: TimelineEvent }) {
  const p = event.payload;
  if (!p) return <p className="text-xs text-neutral-500 italic">No payload</p>;

  switch (event.event_type) {
    case 'user_message':
    case 'assistant_message':
      return <ChatSummary event={event} />;
    case 'thinking_message':
      return <ThinkingSummary event={event} />;
    case 'slot_state_changed':
      return <SlotStateSummary event={event} />;
    case 'task_lifecycle':
    case 'board_task_created':
    case 'board_task_status_changed':
    case 'board_task_note_added':
    case 'board_task_claimed':
    case 'board_task_deleted':
    case 'board_task_updated':
      return <TaskSummary event={event} />;
    case 'gemini_request_started':
    case 'gemini_request_completed':
    case 'codex_request_started':
    case 'codex_request_completed':
      return <LlmRequestSummary event={event} />;
    case 'git_commit':
      return <GitCommitSummary event={event} />;
    case 'memory_phase_changed':
      return <MemoryPhaseSummary event={event} />;
    case 'decision_made':
      return <DecisionSummary event={event} />;
    case 'insight_generated':
      return <InsightSummary event={event} />;
    case 'question_created':
    case 'question_resolved':
      return <QuestionSummary event={event} />;
    default:
      return <DefaultSummary event={event} />;
  }
}

function ChatSummary({ event }: { event: TimelineEvent }) {
  const { role, preview, content_chars, message_id } = event.payload || {};
  const isUser = role === 'user';
  const toolMatch = preview?.match(/^\[([\w_]+)\]$/);

  const fullMsg = useFullMessage(message_id, true);

  // Render structured content blocks for assistant messages with tool_use
  if (fullMsg?.contentBlocks && !isUser) {
    return (
      <div className="space-y-3">
        <div className="flex items-center gap-2">
          <span className="text-[10px] uppercase tracking-wider text-neutral-500 font-medium">Assistant Response</span>
          <span className="text-[10px] text-neutral-600 bg-neutral-900 px-1.5 py-0.5 rounded font-mono">{content_chars} chars</span>
        </div>
        <ContentBlocksRenderer blocks={fullMsg.contentBlocks} />
      </div>
    );
  }

  // User message with images: render text + inline images
  if (isUser && fullMsg && fullMsg.imageCount > 0) {
    // Extract text parts from contentBlocks (images are stripped to placeholders)
    const textParts = fullMsg.contentBlocks
      ?.filter((b: { type: string }) => b.type === 'text')
      .map((b: { text: string }) => b.text)
      .join('\n') || fullMsg.content;
    // Strip image placeholder text like [图片: image/png]
    const cleanText = textParts.replace(/\[图片: [\w/]+\]\n?/g, '').trim();

    return (
      <div className="space-y-3">
        <div className="flex items-center gap-2">
          <span className="text-[10px] uppercase tracking-wider text-neutral-500 font-medium">User Message</span>
          <span className="text-[10px] text-neutral-600 bg-neutral-900 px-1.5 py-0.5 rounded font-mono">{content_chars} chars</span>
        </div>
        {cleanText && (
          <div className="p-3 rounded-lg text-sm leading-relaxed bg-blue-500/10 border border-blue-500/20 text-blue-100 whitespace-pre-wrap break-words">
            {cleanText}
          </div>
        )}
        <div className="flex flex-wrap gap-2">
          {Array.from({ length: fullMsg.imageCount }, (_, i) => (
            <MessageImage key={i} messageId={message_id} index={i} />
          ))}
        </div>
      </div>
    );
  }

  const displayText = fullMsg?.content ?? preview;

  return (
    <div className="space-y-3">
      <div className="flex items-center gap-2">
        <span className="text-[10px] uppercase tracking-wider text-neutral-500 font-medium">
          {isUser ? 'User Message' : 'Assistant Response'}
        </span>
        <span className="text-[10px] text-neutral-600 bg-neutral-900 px-1.5 py-0.5 rounded font-mono">
          {content_chars} chars
        </span>
      </div>

      {toolMatch && !isUser && !fullMsg ? (
        <div className="inline-flex items-center gap-1.5 px-2.5 py-1.5 rounded-md bg-teal-500/10 border border-teal-500/20 text-teal-400 text-xs font-mono">
          <Wrench className="w-3 h-3" />
          <span>{toolMatch[1]}</span>
        </div>
      ) : displayText ? (
        <div className={cn(
          'p-3 rounded-lg text-sm leading-relaxed max-h-[600px] overflow-auto',
          isUser
            ? 'bg-blue-500/10 border border-blue-500/20 text-blue-100 whitespace-pre-wrap break-words'
            : 'bg-teal-500/10 border border-teal-500/20',
        )}>
          {isUser ? displayText : <MarkdownContent content={displayText} />}
        </div>
      ) : null}
    </div>
  );
}

/** Lazy-loaded image from a conversation message */
function MessageImage({ messageId, index }: { messageId: number; index: number }) {
  const [expanded, setExpanded] = useState(false);
  const src = `/api/system/message-image?message_id=${messageId}&index=${index}`;
  return (
    <div className="my-1">
      {/* eslint-disable-next-line @next/next/no-img-element */}
      <img
        src={src}
        alt={`Attachment ${index + 1}`}
        className={cn(
          'rounded-lg border border-neutral-700 cursor-pointer transition-all hover:border-neutral-500',
          expanded ? 'max-w-full' : 'max-w-sm max-h-64 object-cover',
        )}
        onClick={() => setExpanded(!expanded)}
        loading="lazy"
      />
    </div>
  );
}

function ThinkingSummary({ event }: { event: TimelineEvent }) {
  const preview = event.payload?.preview || '';
  const messageId = event.payload?.message_id;
  const contentChars = event.payload?.content_chars || 0;

  const fullMsg = useFullMessage(messageId, true);
  const displayText = fullMsg?.content ?? preview;
  const translation = fullMsg?.translation;
  const [showOriginal, setShowOriginal] = useState(false);

  return (
    <div className="border border-violet-500/20 rounded-lg bg-violet-500/5 overflow-hidden">
      <div className="flex items-center gap-2 p-2.5">
        <Brain className="w-4 h-4 text-violet-400 shrink-0" />
        <span className="text-xs text-violet-300 font-medium">Thinking</span>
        <span className="text-[10px] text-neutral-600 font-mono">{contentChars} chars</span>
        <div className="flex-1" />
        {translation && (
          <button
            onClick={() => setShowOriginal(!showOriginal)}
            className={cn(
              'text-[10px] px-1.5 py-0.5 rounded font-medium transition-colors',
              showOriginal ? 'bg-violet-500/20 text-violet-300' : 'bg-indigo-500/20 text-indigo-300',
            )}
          >
            {showOriginal ? 'EN' : '中'}
          </button>
        )}
      </div>
      <div className="px-3 pb-3 border-t border-violet-500/10">
        {translation && !showOriginal ? (
          <pre className="text-[12px] text-indigo-100/90 whitespace-pre-wrap break-words leading-relaxed max-h-96 overflow-auto mt-2">
            {translation}
          </pre>
        ) : (
          <pre className="text-[11px] text-violet-200/80 font-mono whitespace-pre-wrap break-words leading-relaxed max-h-96 overflow-auto mt-2">
            {displayText}
          </pre>
        )}
      </div>
    </div>
  );
}

function SlotStateSummary({ event }: { event: TimelineEvent }) {
  const { slot_id, prev_state, new_state } = event.payload || {};
  return (
    <div className="flex flex-col items-center justify-center py-6 bg-neutral-900/50 rounded-lg border border-neutral-800">
      <div className="flex items-center gap-1.5 text-neutral-400 mb-4 bg-neutral-800 px-2 py-1 rounded text-xs font-mono">
        <Settings2 className="w-3 h-3" /> {slot_id}
      </div>
      <div className="flex items-center gap-4">
        <SlotBadge state={prev_state || '?'} />
        <div className="flex flex-col items-center text-neutral-500">
          <ArrowRight className="w-5 h-5" />
        </div>
        <SlotBadge state={new_state || '?'} />
      </div>
    </div>
  );
}

function SlotBadge({ state }: { state: string }) {
  const isActive = state === 'Thinking' || state === 'Working' || state === 'Running';
  return (
    <div className={cn(
      'px-4 py-2 rounded-full text-xs font-medium border',
      isActive ? 'bg-amber-500/20 border-amber-500/30 text-amber-300' : 'bg-slate-500/20 border-slate-500/30 text-slate-300',
    )}>
      {state}
    </div>
  );
}

function TaskSummary({ event }: { event: TimelineEvent }) {
  const p = event.payload || {};
  const isUpdate = event.event_type === 'board_task_updated';
  return (
    <div className="border border-blue-500/20 bg-blue-950/10 rounded-lg p-4 space-y-2">
      <div className="flex items-center gap-2">
        <Activity className="w-4 h-4 text-blue-400" />
        <span className="text-xs font-medium text-blue-300">{isUpdate ? 'Board Task Updated' : 'Task Lifecycle'}</span>
        {p.action && (
          <span className={cn(
            'text-[10px] px-1.5 py-0.5 rounded font-medium',
            p.action === 'completed' ? 'bg-green-500/20 text-green-400' :
            p.action === 'created' ? 'bg-blue-500/20 text-blue-400' : 'bg-neutral-800 text-neutral-400',
          )}>
            {p.action}
          </span>
        )}
        {p.status && (
          <span className={cn(
            'text-[10px] px-1.5 py-0.5 rounded font-medium',
            p.status === 'done' ? 'bg-green-500/20 text-green-400' : 'bg-neutral-800 text-neutral-400',
          )}>
            {p.status}
          </span>
        )}
      </div>
      {p.title && <p className="text-sm text-neutral-200">{p.title}</p>}
      {p.task_id && <p className="text-[10px] text-neutral-500 font-mono">{p.task_id}</p>}
    </div>
  );
}

function LlmRequestSummary({ event }: { event: TimelineEvent }) {
  const p = event.payload || {};
  const isStarted = event.event_type.endsWith('_started');
  const isCodex = event.event_type.startsWith('codex_');
  const accent = isCodex ? 'sky' : 'purple';
  const [imageExpanded, setImageExpanded] = useState(false);

  return (
    <div className={cn(
      'border rounded-lg overflow-hidden',
      `border-${accent}-500/20 bg-${accent}-950/10`,
    )} style={{
      borderColor: isCodex ? 'rgba(56,189,248,0.2)' : 'rgba(168,85,247,0.2)',
      backgroundColor: isCodex ? 'rgba(8,47,73,0.1)' : 'rgba(59,7,100,0.1)',
    }}>
      <div className="flex items-center gap-2 p-3 pb-0">
        <Cpu className={cn('w-4 h-4', isCodex ? 'text-sky-400' : 'text-purple-400')} />
        <span className={cn('text-xs font-medium', isCodex ? 'text-sky-300' : 'text-purple-300')}>
          {isCodex ? 'GPT' : 'Gemini'} — {isStarted ? 'Request Sent' : 'Response Received'}
        </span>
        {p.error && <span className="text-[10px] px-1.5 py-0.5 rounded bg-red-500/20 text-red-400">Error</span>}
      </div>

      <div className="flex gap-3 p-3">
        {/* Left: content (prompt/response + image) */}
        <div className="flex-1 min-w-0 space-y-3">
          {p.image_hash && (
            <div>
              <button
                onClick={() => setImageExpanded(v => !v)}
                className="flex items-center gap-1 text-[9px] text-neutral-500 uppercase tracking-wider mb-1 hover:text-neutral-300 transition-colors"
              >
                {imageExpanded ? <ChevronDown className="w-3 h-3" /> : <ChevronRight className="w-3 h-3" />}
                Source Image
              </button>
              {imageExpanded ? (
                <img
                  src={`/api/images?hash=${p.image_hash}`}
                  alt="Vision source"
                  className="max-w-full max-h-[500px] rounded-lg border border-neutral-700 object-contain"
                />
              ) : (
                <img
                  src={`/api/images?hash=${p.image_hash}`}
                  alt="Vision source"
                  className="w-24 h-16 rounded border border-neutral-700 object-cover cursor-pointer hover:opacity-80 transition-opacity"
                  onClick={() => setImageExpanded(true)}
                />
              )}
            </div>
          )}
          {p.request_id && <GeminiContentPanel requestId={p.request_id} isResponse={!isStarted} />}
        </div>

        {/* Right: stat cards */}
        <div className="w-40 shrink-0 space-y-1.5">
          <MiniStat label="Model" value={p.model || '-'} />
          <MiniStat label="Caller" value={p.caller || '-'} />
          {!isStarted && <MiniStat label="Duration" value={p.duration_ms ? `${(p.duration_ms / 1000).toFixed(1)}s` : '-'} />}
          <MiniStat label="Prompt" value={`${p.prompt_chars || 0} chars`} />
          {!isStarted && <MiniStat label="Response" value={`${p.response_chars || 0} chars`} />}
          {!isStarted && p.status && <MiniStat label="Status" value={p.status} />}
          {p.has_image && !p.image_hash && <MiniStat label="Image" value="Yes" />}
          {p.output_tokens != null && <MiniStat label="Out Tokens" value={`${p.output_tokens}`} />}
        </div>
      </div>
    </div>
  );
}

function GitCommitSummary({ event }: { event: TimelineEvent }) {
  const { short_hash, hash, message, author, repo, committed_at } = event.payload || {};
  return (
    <div className="border border-green-500/20 bg-green-950/10 rounded-lg p-4">
      <div className="flex items-start justify-between mb-3">
        <div className="flex items-center gap-2 text-green-400 font-mono text-sm bg-green-500/10 px-2 py-1 rounded">
          <GitCommit className="w-4 h-4" />
          {short_hash}
        </div>
        {repo && <span className="text-[10px] text-neutral-500 uppercase bg-neutral-900 px-2 py-1 rounded">{repo}</span>}
      </div>
      <p className="text-sm text-neutral-200 font-medium mb-4 leading-relaxed">{message}</p>
      <div className="flex items-center gap-4 text-xs text-neutral-400">
        {author && <div className="flex items-center gap-1.5"><User className="w-3.5 h-3.5" />{author}</div>}
        {committed_at && <div className="flex items-center gap-1.5"><Clock className="w-3.5 h-3.5" />{formatBeijing(committed_at)}</div>}
      </div>
      {hash && <p className="text-[10px] text-neutral-600 font-mono mt-2 select-all">{hash}</p>}
    </div>
  );
}

function MemoryPhaseSummary({ event }: { event: TimelineEvent }) {
  const p = event.payload || {};
  return (
    <div className="flex flex-col items-center justify-center py-6 bg-neutral-900/50 rounded-lg border border-neutral-800">
      <Brain className="w-5 h-5 text-indigo-400 mb-2" />
      <span className="text-[10px] text-neutral-500 uppercase tracking-wider mb-3">Memory Phase</span>
      <div className="flex items-center gap-4">
        <SlotBadge state={p.prev_phase || p.from || '?'} />
        <ArrowRight className="w-5 h-5 text-neutral-500" />
        <SlotBadge state={p.new_phase || p.to || '?'} />
      </div>
    </div>
  );
}

function DecisionSummary({ event }: { event: TimelineEvent }) {
  const p = event.payload || {};
  return (
    <div className="border border-amber-500/20 bg-amber-950/10 rounded-lg p-4 space-y-2">
      <div className="flex items-center gap-2">
        <Zap className="w-4 h-4 text-amber-400" />
        <span className="text-xs font-medium text-amber-300">Decision Made</span>
        {p.tier && <span className="text-[10px] bg-amber-500/20 text-amber-400 px-1.5 py-0.5 rounded">{p.tier}</span>}
      </div>
      {p.question && <p className="text-sm text-neutral-200 leading-relaxed">{p.question}</p>}
      {p.answer && <p className="text-xs text-neutral-400 leading-relaxed">{p.answer}</p>}
    </div>
  );
}

function InsightSummary({ event }: { event: TimelineEvent }) {
  const p = event.payload || {};
  return (
    <div className="border border-emerald-500/30 bg-emerald-950/20 rounded-lg p-4 space-y-2">
      <div className="flex items-center gap-1.5">
        <Sparkles className="w-4 h-4 text-emerald-400" />
        <span className="text-xs font-medium text-emerald-300">Insight</span>
      </div>
      <p className="text-sm text-emerald-100 leading-relaxed">{p.title || eventSummary(event)}</p>
      {p.body && <p className="text-xs text-emerald-300/70 leading-relaxed">{p.body}</p>}
    </div>
  );
}

function QuestionSummary({ event }: { event: TimelineEvent }) {
  const p = event.payload || {};
  const isResolved = event.event_type === 'question_resolved';
  return (
    <div className="border border-amber-500/20 bg-amber-950/10 rounded-lg p-4 space-y-2">
      <div className="flex items-center gap-2">
        <MessageSquare className="w-4 h-4 text-amber-400" />
        <span className="text-xs font-medium text-amber-300">{isResolved ? 'Question Resolved' : 'Question Created'}</span>
      </div>
      {p.question && <p className="text-sm text-neutral-200 leading-relaxed">{p.question}</p>}
      {isResolved && p.answer && <p className="text-xs text-green-400 leading-relaxed mt-1">{p.answer}</p>}
      {p.question_id && <p className="text-[10px] text-neutral-500 font-mono">{p.question_id}</p>}
    </div>
  );
}

// ── Markdown Renderer ───────────────────────────────────────

const MarkdownContent = memo(function MarkdownContent({ content }: { content: string }) {
  return (
    <div className="prose prose-sm prose-invert max-w-none
      prose-headings:text-teal-200 prose-headings:font-semibold prose-headings:mt-3 prose-headings:mb-1
      prose-p:text-teal-100/90 prose-p:my-1.5 prose-p:leading-relaxed
      prose-strong:text-teal-200 prose-em:text-teal-200/80
      prose-li:text-teal-100/90 prose-li:my-0.5
      prose-ul:my-1.5 prose-ol:my-1.5
      prose-a:text-cyan-400 prose-a:no-underline hover:prose-a:underline
      prose-code:text-amber-300 prose-code:bg-neutral-800 prose-code:px-1 prose-code:py-0.5 prose-code:rounded prose-code:text-xs prose-code:before:content-none prose-code:after:content-none
      prose-pre:bg-neutral-900 prose-pre:border prose-pre:border-neutral-800 prose-pre:rounded-md prose-pre:my-2
      prose-blockquote:border-teal-500/30 prose-blockquote:text-teal-200/70
      prose-hr:border-neutral-700
      prose-table:text-xs prose-table:w-full prose-table:border-collapse prose-table:my-2
      prose-thead:border-b prose-thead:border-neutral-700
      prose-th:text-teal-300 prose-th:text-left prose-th:px-3 prose-th:py-1.5 prose-th:bg-neutral-800/50 prose-th:border prose-th:border-neutral-700/60 prose-th:font-medium
      prose-td:text-teal-100/80 prose-td:px-3 prose-td:py-1.5 prose-td:border prose-td:border-neutral-700/40
    ">
      <ReactMarkdown remarkPlugins={[remarkGfm]}>{content}</ReactMarkdown>
    </div>
  );
});

// ── Tool Call Viewers (pluggable registry) ──────────────────

// eslint-disable-next-line @typescript-eslint/no-explicit-any
const ToolViewers: Record<string, React.FC<{ input: any; block?: any }>> = {
  Edit: EditDiffViewer,
  Write: WriteSummaryViewer,
  Skill: ToolResultViewer,
  Read: ToolResultViewer,
};

/** Render content blocks from raw_content (text + tool_use interleaved) */
// eslint-disable-next-line @typescript-eslint/no-explicit-any
function ContentBlocksRenderer({ blocks }: { blocks: any[] }) {
  const rendered = useMemo(() => {
    const items: { type: 'text'; text: string }[] | { type: 'tool'; block: Record<string, unknown> }[] = [];
    let textBuf = '';

    for (const block of blocks) {
      const btype = block?.type;
      if (btype === 'text' && block.text) {
        textBuf += (textBuf ? '\n' : '') + block.text;
      } else if (btype === 'tool_use') {
        if (textBuf) { items.push({ type: 'text', text: textBuf } as never); textBuf = ''; }
        items.push({ type: 'tool', block } as never);
      }
      // skip thinking, tool_result, etc.
    }
    if (textBuf) items.push({ type: 'text', text: textBuf } as never);
    return items;
  }, [blocks]);

  return (
    <div className="space-y-2">
      {rendered.map((item, i) => {
        if (item.type === 'text') {
          const t = item as { type: 'text'; text: string };
          return (
            <div key={i} className="p-3 rounded-lg text-sm leading-relaxed max-h-[600px] overflow-auto bg-teal-500/10 border border-teal-500/20">
              <MarkdownContent content={t.text} />
            </div>
          );
        }
        // eslint-disable-next-line @typescript-eslint/no-explicit-any
        const tool = item as { type: 'tool'; block: any };
        return <ToolCallCard key={i} block={tool.block} />;
      })}
    </div>
  );
}

/** Generic tool call card with collapsible body and pluggable viewer */
// eslint-disable-next-line @typescript-eslint/no-explicit-any
function ToolCallCard({ block }: { block: any }) {
  const [expanded, setExpanded] = useState(true);
  const name: string = block.name || 'unknown';
  const input = block.input || {};
  const Viewer = ToolViewers[name];

  const filePath = input.file_path || input.path || input.command;
  const subtitle = typeof filePath === 'string'
    ? filePath.replace(/^.*\/Projects\//, '~/Projects/')
    : null;

  const iconMap: Record<string, React.ReactNode> = {
    Edit: <FileCode className="w-3 h-3" />,
    Write: <File className="w-3 h-3" />,
    Read: <Eye className="w-3 h-3" />,
    Bash: <Terminal className="w-3 h-3" />,
    Grep: <Search className="w-3 h-3" />,
    Skill: <BookOpen className="w-3 h-3" />,
  };

  return (
    <div className="border border-neutral-800 rounded-md overflow-hidden">
      <div
        className="bg-neutral-900/80 px-3 py-1.5 flex items-center gap-2 cursor-pointer hover:bg-neutral-800/60 transition-colors"
        onClick={() => setExpanded(!expanded)}
      >
        {expanded ? <ChevronDown className="w-3 h-3 text-neutral-500" /> : <ChevronRight className="w-3 h-3 text-neutral-500" />}
        <span className="text-teal-400 font-mono text-xs font-medium">{iconMap[name] || <Wrench className="w-3 h-3" />}</span>
        <span className="text-teal-400 font-mono text-xs">{name}</span>
        {subtitle && <span className="text-neutral-500 text-[10px] font-mono truncate">{subtitle}</span>}
      </div>
      {expanded && (
        <div className="bg-neutral-950">
          {Viewer ? <Viewer input={input} block={block} /> : <FallbackToolViewer name={name} input={input} />}
        </div>
      )}
    </div>
  );
}

/** Edit tool: diff viewer with red/green highlighting */
// eslint-disable-next-line @typescript-eslint/no-explicit-any
function EditDiffViewer({ input }: { input: any }) {
  const { file_path, old_string, new_string } = input;

  const changes = useMemo(() => {
    if (!old_string && !new_string) return [];
    return diffLines(old_string || '', new_string || '');
  }, [old_string, new_string]);

  // Compute line numbers
  const lineData = useMemo(() => {
    let oldLine = 1;
    let newLine = 1;
    const result: { prefix: string; oldNum: string; newNum: string; text: string; kind: 'add' | 'del' | 'ctx' }[] = [];

    for (const part of changes) {
      const lines = part.value.replace(/\n$/, '').split('\n');
      for (const line of lines) {
        if (part.added) {
          result.push({ prefix: '+', oldNum: '', newNum: String(newLine++), text: line, kind: 'add' });
        } else if (part.removed) {
          result.push({ prefix: '-', oldNum: String(oldLine++), newNum: '', text: line, kind: 'del' });
        } else {
          result.push({ prefix: ' ', oldNum: String(oldLine++), newNum: String(newLine++), text: line, kind: 'ctx' });
        }
      }
    }
    return result;
  }, [changes]);

  const shortPath = file_path?.replace(/^.*\/Projects\//, '~/Projects/') || 'unknown';

  return (
    <div className="flex flex-col text-[11px] font-mono">
      <div className="px-3 py-1.5 bg-neutral-800/40 border-b border-neutral-800 text-neutral-400 flex items-center gap-2">
        <FileCode className="w-3 h-3 text-neutral-500" />
        <span className="truncate">{shortPath}</span>
      </div>
      <div className="overflow-auto max-h-[500px]">
        {lineData.map((ln, i) => (
          <div
            key={i}
            className={cn(
              'flex leading-relaxed',
              ln.kind === 'add' && 'bg-green-500/10',
              ln.kind === 'del' && 'bg-red-500/10',
            )}
          >
            <span className="select-none text-neutral-600 w-8 text-right pr-1 shrink-0">{ln.oldNum}</span>
            <span className="select-none text-neutral-600 w-8 text-right pr-1 shrink-0 border-r border-neutral-800">{ln.newNum}</span>
            <span className={cn(
              'select-none w-4 text-center shrink-0',
              ln.kind === 'add' && 'text-green-400',
              ln.kind === 'del' && 'text-red-400',
              ln.kind === 'ctx' && 'text-neutral-600',
            )}>{ln.prefix}</span>
            <span className={cn(
              'whitespace-pre-wrap break-all px-1',
              ln.kind === 'add' && 'text-green-300',
              ln.kind === 'del' && 'text-red-300',
              ln.kind === 'ctx' && 'text-neutral-300',
            )}>{ln.text}</span>
          </div>
        ))}
        {lineData.length === 0 && (
          <div className="px-3 py-2 text-neutral-500 italic">No changes</div>
        )}
      </div>
    </div>
  );
}

/** Write tool: show file path and content preview */
// eslint-disable-next-line @typescript-eslint/no-explicit-any
function WriteSummaryViewer({ input }: { input: any }) {
  const { file_path, content } = input;
  const shortPath = file_path?.replace(/^.*\/Projects\//, '~/Projects/') || 'unknown';
  const preview = typeof content === 'string' ? content.slice(0, 500) : '';
  return (
    <div className="flex flex-col text-[11px] font-mono">
      <div className="px-3 py-1.5 bg-neutral-800/40 border-b border-neutral-800 text-neutral-400 flex items-center gap-2">
        <File className="w-3 h-3 text-neutral-500" />
        <span className="truncate">{shortPath}</span>
        {content && <span className="text-neutral-600 ml-auto">{content.length} chars</span>}
      </div>
      <pre className="p-3 text-green-300/80 whitespace-pre-wrap break-all max-h-64 overflow-auto">{preview}{content && content.length > 500 ? '\n...' : ''}</pre>
    </div>
  );
}

/** Lazy-load tool result from audit_detail API, shared by Skill/Read viewers */
// eslint-disable-next-line @typescript-eslint/no-explicit-any
function ToolResultViewer({ input, block }: { input: any; block?: any }) {
  const toolId: string | undefined = block?.id;
  const [detail, setDetail] = useState<{ input?: unknown; output?: unknown } | null>(null);
  const [loading, setLoading] = useState(false);

  // Auto-load tool result on mount (user preference: show full content directly)
  useEffect(() => {
    if (!toolId || detail) return;
    setLoading(true);
    fetch(`/api/system/tool-call?id=${encodeURIComponent(toolId)}`)
      .then(r => r.json())
      .then(data => { if (!data.error) setDetail(data); })
      .catch(() => {})
      .finally(() => setLoading(false));
  }, [toolId, detail]);

  // Extract readable text from tool result
  const resultText = useMemo(() => {
    if (!detail?.output) return '';
    const out = detail.output;
    if (typeof out === 'string') return out;
    // Handle array of content blocks [{type:'text', text:'...'}]
    if (Array.isArray(out)) {
      return out
        .filter((b: Record<string, unknown>) => b.type === 'text')
        .map((b: Record<string, unknown>) => b.text)
        .join('\n');
    }
    // Handle object with text/content field
    if (typeof out === 'object' && out !== null) {
      const o = out as Record<string, unknown>;
      if (typeof o.text === 'string') return o.text;
      if (typeof o.content === 'string') return o.content;
      return JSON.stringify(out, null, 2);
    }
    return String(out);
  }, [detail]);

  const toolName = block?.name || 'Tool';
  const isSkill = toolName === 'Skill';
  const isRead = toolName === 'Read';

  // For Skill: derive file path from skill name; for Read: use input.file_path
  const displayPath = isSkill
    ? (input?.skill ? `~/.claude/skills/${input.skill}/SKILL.md` : null)
    : (input?.file_path?.replace(/^.*\/Projects\//, '~/Projects/') || input?.path || null);

  return (
    <div className="p-3 space-y-2">
      {/* Input params */}
      <div className="space-y-1">
        {Object.entries(input || {}).filter(([, v]) => v != null).map(([k, v]) => {
          const val = typeof v === 'string' ? v : JSON.stringify(v);
          // Truncate long values in input display (e.g., Read's file_path is shown above)
          const display = val.length > 200 ? val.slice(0, 200) + '...' : val;
          return (
            <div key={k} className="flex gap-2 text-[11px] font-mono">
              <span className="text-neutral-500 shrink-0">{k}:</span>
              <span className="text-neutral-300 break-all">{display}</span>
            </div>
          );
        })}
      </div>

      {/* Derived path for Skill */}
      {isSkill && displayPath && (
        <div className="flex items-center gap-1.5 text-[11px] font-mono text-amber-400/80">
          <BookOpen className="w-3 h-3" />
          <span>{displayPath}</span>
        </div>
      )}

      {/* Tool result — auto-loaded */}
      {loading && (
        <div className="flex items-center gap-1.5 text-[11px] text-neutral-500">
          <Loader2 className="w-3 h-3 animate-spin" />
          Loading...
        </div>
      )}

      {resultText && (
        <div className="border border-neutral-800 rounded overflow-hidden">
          <div className="bg-neutral-900/60 px-2 py-1 flex items-center gap-1.5 text-[10px] text-neutral-400 border-b border-neutral-800">
            {isRead ? <Eye className="w-3 h-3" /> : <BookOpen className="w-3 h-3" />}
            <span className="font-mono truncate">{displayPath || 'Result'}</span>
            <span className="text-neutral-600 ml-auto">{resultText.length} chars</span>
          </div>
          <pre className="p-2 text-[11px] font-mono text-neutral-300/80 whitespace-pre-wrap break-words max-h-80 overflow-auto leading-relaxed">
            {resultText}
          </pre>
        </div>
      )}

      {!loading && !resultText && detail && (
        <span className="text-[10px] text-neutral-500 italic">No result data</span>
      )}
    </div>
  );
}

/** Fallback: show tool input as compact key-value pairs */
// eslint-disable-next-line @typescript-eslint/no-explicit-any
function FallbackToolViewer({ input }: { name: string; input: any }) {
  const entries = Object.entries(input || {}).filter(([, v]) => v != null);
  return (
    <div className="p-3 space-y-1">
      {entries.length === 0 ? (
        <span className="text-neutral-500 text-xs italic">No input parameters</span>
      ) : entries.map(([k, v]) => {
        const val = typeof v === 'string' ? (v.length > 200 ? v.slice(0, 200) + '...' : v) : JSON.stringify(v);
        return (
          <div key={k} className="flex gap-2 text-[11px] font-mono">
            <span className="text-neutral-500 shrink-0">{k}:</span>
            <span className="text-neutral-300 break-all whitespace-pre-wrap">{val}</span>
          </div>
        );
      })}
    </div>
  );
}

function DefaultSummary({ event }: { event: TimelineEvent }) {
  return (
    <div className="space-y-2">
      <p className="text-xs text-neutral-300">{eventSummary(event)}</p>
      {event.summary && <p className="text-xs text-neutral-500 italic">{event.summary}</p>}
    </div>
  );
}

// ── Collapsible JSON Tree Viewer ────────────────────────────

// eslint-disable-next-line @typescript-eslint/no-explicit-any
function JsonTreeViewer({ data }: { data: any }) {
  return (
    <div className="bg-[#0d1117] text-[#c9d1d9] font-mono text-[11px] p-3 rounded-lg overflow-auto border border-neutral-800">
      <JsonNode value={data} isRoot />
    </div>
  );
}

// eslint-disable-next-line @typescript-eslint/no-explicit-any
function JsonNode({ value, name, isRoot = false }: { value: any; name?: string; isRoot?: boolean }) {
  const [expanded, setExpanded] = useState(true);
  const isObject = value !== null && typeof value === 'object';
  const isArray = Array.isArray(value);

  if (!isObject) {
    let color = 'text-[#a5d6ff]';
    if (typeof value === 'number') color = 'text-[#79c0ff]';
    if (typeof value === 'boolean') color = 'text-[#ff7b72]';
    if (value === null) color = 'text-[#8b949e]';

    return (
      <div className="flex leading-relaxed" style={{ marginLeft: isRoot ? 0 : 16 }}>
        {name != null && <span className="text-[#7ee787] mr-1">&quot;{name}&quot;:</span>}
        <span className={color}>
          {typeof value === 'string' ? `"${value}"` : String(value)}
        </span>
      </div>
    );
  }

  const keys = Object.keys(value);
  const isEmpty = keys.length === 0;
  const bracket = isArray ? ['[', ']'] : ['{', '}'];

  return (
    <div style={{ marginLeft: isRoot ? 0 : 16 }} className="leading-relaxed">
      <div
        className={cn('flex items-center w-fit pr-2 rounded', !isEmpty && 'cursor-pointer hover:bg-white/5')}
        onClick={() => !isEmpty && setExpanded(!expanded)}
      >
        {!isEmpty ? (
          expanded ? <ChevronDown className="w-3 h-3 text-neutral-500 mr-1 shrink-0" /> : <ChevronRight className="w-3 h-3 text-neutral-500 mr-1 shrink-0" />
        ) : <span className="w-4 shrink-0" />}
        {name != null && <span className="text-[#7ee787] mr-1">&quot;{name}&quot;:</span>}
        <span className="text-neutral-400">{bracket[0]}</span>
        {!expanded && !isEmpty && <span className="text-neutral-500 px-1">…{keys.length}</span>}
        {(!expanded || isEmpty) && <span className="text-neutral-400">{bracket[1]}</span>}
      </div>
      {expanded && !isEmpty && (
        <>
          {keys.map((key) => (
            <JsonNode key={key} name={isArray ? undefined : key} value={value[key as keyof typeof value]} />
          ))}
          <div style={{ marginLeft: 16 }} className="text-neutral-400">{bracket[1]}</div>
        </>
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
