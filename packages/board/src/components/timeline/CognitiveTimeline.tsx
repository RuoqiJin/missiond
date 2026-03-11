'use client';

import { useState, useEffect, useCallback, useMemo, useRef } from 'react';
import {
  Search, RefreshCw, ChevronLeft, ChevronRight,
  Calendar,
} from 'lucide-react';
import { cn } from '@/lib/utils';
import { utcMs, formatBeijingTime, beijingDayRange, toBeijingDate, todayBeijing } from '@/lib/time';
import { useEventInvalidation } from '../../hooks/useEventStream';
import { useTimelineGestures } from '../../hooks/useTimelineGestures';

// ── Local modules ──
import type { TimelineEvent, SelectionState, TimelineStats, ViewMode } from './types';
import { EMPTY_SELECTION } from './types';
import {
  SESSION_COLORS, SWIMLANES, SLOT_LANE_IDX, WINDOW_OPTIONS,
  DOT_STYLES, CAPSULE_STYLES, getSlotLine,
} from './constants';
import {
  getDotStatus, getCapsuleStatus, getEventColor,
  getSwimlane, isLaneVisible, getEventLaneId, hashSessionColor, isChatEvent,
  eventSummary, hasError, windowToMs,
  loadViewState, saveViewState, formatDailyLabel, shiftDay,
} from './helpers';
import { StatCard } from './EventSummaryView';
import { UnifiedDetailPanel } from './UnifiedDetailPanel';

// ── Main Component ─────────────────────────────────────────

export function CognitiveTimeline() {
  const timelineVersion = useEventInvalidation('timeline');

  const [events, setEvents] = useState<TimelineEvent[]>([]);
  const [sessionsMeta, setSessionsMeta] = useState<Record<string, { startedAt: string }>>({});
  const [stats, setStats] = useState<TimelineStats | null>(null);
  const [hourlyStats, setHourlyStats] = useState<TimelineStats | null>(null);
  const [selection, setSelection] = useState<SelectionState>(EMPTY_SELECTION);
  const requestIdRef = useRef(0); // Guard against async race conditions
  const savedView = useMemo(() => loadViewState(), []);
  const [searchInput, setSearchInput] = useState('');
  const [searchQuery, setSearchQuery] = useState(''); // Only updated on Enter — drives data fetching
  const [activeWindow, setActiveWindow] = useState(savedView.activeWindow);
  const [loading, setLoading] = useState(false);

  // Derived: focused event object from selection (reference-stable across data refreshes)
  const selectedEventRef = useRef<TimelineEvent | null>(null);
  const selectedEvent = useMemo(() => {
    if (selection.focusedSeq == null) { selectedEventRef.current = null; return null; }
    const found = events.find(e => e.seq === selection.focusedSeq) ?? null;
    if (!found) { selectedEventRef.current = null; return null; }
    // Keep same reference if seq + summary haven't changed (avoids cascade on data refresh)
    const prev = selectedEventRef.current;
    if (prev && prev.seq === found.seq && prev.summary === found.summary && prev.trace_id === found.trace_id) return prev;
    selectedEventRef.current = found;
    return found;
  }, [selection.focusedSeq, events]);

  // Ref mirror of selection for stable useCallback closures
  const selectionRef = useRef(selection);
  selectionRef.current = selection;

  // Lane visibility (Solo / Mute)
  const [soloed, setSoloed] = useState<Set<string>>(() => new Set(savedView.soloed));
  const [muted, setMuted] = useState<Set<string>>(() => new Set(savedView.muted));

  // Daily view state
  const [viewMode, setViewMode] = useState<ViewMode>(savedView.dailyDate ? 'daily' : 'relative');
  const [dailyDate, setDailyDate] = useState<string | null>(savedView.dailyDate);

  // In daily mode, allow panning to end of selected day (even if it's "future" for today)
  const gestureMaxTime = useMemo(() => {
    if (viewMode === 'daily' && dailyDate) {
      const { end } = beijingDayRange(dailyDate);
      return Math.max(end, Date.now() + 5 * 60_000);
    }
    return Date.now() + 5 * 60_000;
  }, [viewMode, dailyDate]);

  // Persist view state on change
  useEffect(() => {
    saveViewState({ activeWindow, dailyDate, soloed: Array.from(soloed), muted: Array.from(muted) });
  }, [activeWindow, dailyDate, soloed, muted]);

  // Ref maps for bidirectional scroll sync
  const timelineDotRefs = useRef<Map<number, HTMLButtonElement>>(new Map());
  const contextItemRefs = useRef<Map<number, HTMLButtonElement>>(new Map());

  // Gesture-controlled timeline: pan (two-finger scroll) + zoom (pinch)
  const now = Date.now();
  // eslint-disable-next-line react-hooks/exhaustive-deps
  const initialRange = useMemo(() => {
    if (savedView.dailyDate) {
      const { start, end } = beijingDayRange(savedView.dailyDate);
      return { min: start, max: end };
    }
    return { min: now - windowToMs(savedView.activeWindow), max: now };
  }, []);
  const { containerRef: timelineRef, timeRange, animateToRange, isRefreshing } = useTimelineGestures({
    initialRange,
    maxAllowedTime: gestureMaxTime,
    onRefresh: async () => {
      loadedRangeRef.current = null;
      const { min, max } = timeRange;
      const windowMs = max - min;
      const n = Date.now();
      setLoading(true);
      await fetchForRange(n - windowMs, n, { replace: true }).catch(() => {});
      setLoading(false);
    },
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
          let added = 0;
          for (const ev of incoming) { if (!map.has(ev.seq)) added++; map.set(ev.seq, ev); }
          if (added === 0) return prev; // No new events — skip re-render cascade
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
        // No need to sync selection — contextSeqs are IDs, events update propagates automatically
      }
    };
    globalThis.addEventListener('timeline-summary-update', handler);
    return () => globalThis.removeEventListener('timeline-summary-update', handler);
  }, []);

  // ── Unified selection handlers (defined as refs to avoid sessionLayout ordering) ──
  const sessionLayoutRef = useRef<typeof sessionLayout>(null!);
  const slotLayoutRef = useRef<typeof slotLayout>(null!);

  const selectCapsule = useCallback((sid: string) => {
    setSelection(prev => {
      if (prev.scope === 'session' && prev.scopeId === sid) return EMPTY_SELECTION; // toggle off
      const sessionEvents = sessionLayoutRef.current?.map.get(sid)?.events || [];
      const sorted = [...sessionEvents].sort((a, b) => utcMs(a.created_at) - utcMs(b.created_at));
      return {
        scope: 'session', scopeId: sid,
        focusedSeq: sorted[0]?.seq ?? null,
        contextSeqs: sorted.map(e => e.seq),
        source: 'timeline',
      };
    });
  }, []);

  const selectSlotCapsule = useCallback((slotId: string) => {
    setSelection(prev => {
      if (prev.scope === 'slot' && prev.scopeId === slotId) return EMPTY_SELECTION; // toggle off
      const slotEvents = slotLayoutRef.current?.map.get(slotId)?.events || [];
      const sorted = [...slotEvents].sort((a, b) => utcMs(a.created_at) - utcMs(b.created_at));
      return {
        scope: 'slot', scopeId: slotId,
        focusedSeq: sorted[0]?.seq ?? null,
        contextSeqs: sorted.map(e => e.seq),
        source: 'timeline',
      };
    });
  }, []);

  const selectEvent = useCallback(async (ev: TimelineEvent, source: 'timeline' | 'list') => {
    const rid = ++requestIdRef.current;
    const sessionId = ev.payload?.session_id as string | undefined;
    const traceId = ev.trace_id;

    if (traceId) {
      // Already in this trace — just move focus, no refetch
      const cur = selectionRef.current;
      if (cur.scope === 'trace' && cur.scopeId === traceId && cur.contextSeqs.length > 0) {
        setSelection(prev => ({ ...prev, focusedSeq: ev.seq, source }));
        return;
      }
      // Enter trace scope — fetch context events
      setSelection({ scope: 'trace', scopeId: traceId, focusedSeq: ev.seq, contextSeqs: [], source });
      try {
        const res = await fetch(`/api/timeline/events?traceId=${traceId}&limit=50`);
        if (requestIdRef.current !== rid) return; // Stale
        const data = await res.json();
        const raw = data.events;
        const incoming: TimelineEvent[] = Array.isArray(raw) ? raw : (raw?.events || []);
        // Store only seq IDs in selection
        const incomingSeqs = incoming.map(e => e.seq);
        setSelection(prev => prev.scopeId === traceId ? { ...prev, contextSeqs: incomingSeqs } : prev);
        // Merge incoming events into global events array
        if (incoming.length > 0) {
          setEvents(prev => {
            const map = new Map<number, TimelineEvent>();
            for (const e of prev) map.set(e.seq, e);
            let added = 0;
            for (const e of incoming) { if (!map.has(e.seq)) added++; map.set(e.seq, e); }
            if (added === 0) return prev;
            return Array.from(map.values()).sort((a, b) => (b.seq ?? 0) - (a.seq ?? 0));
          });
        }
      } catch { /* keep selection, just no context events */ }
    } else if (sessionId) {
      // Already in this session — just move focus
      const cur = selectionRef.current;
      if (cur.scope === 'session' && cur.scopeId === sessionId && cur.contextSeqs.length > 0) {
        setSelection(prev => ({ ...prev, focusedSeq: ev.seq, source }));
        return;
      }
      // Enter session scope, focus this event
      const sessionEvents = sessionLayoutRef.current?.map.get(sessionId)?.events || [];
      const sorted = [...sessionEvents].sort((a, b) => utcMs(a.created_at) - utcMs(b.created_at));
      setSelection({ scope: 'session', scopeId: sessionId, focusedSeq: ev.seq, contextSeqs: sorted.map(e => e.seq), source });
    } else {
      // No context — focus event in global scope
      setSelection({ scope: 'global', scopeId: null, focusedSeq: ev.seq, contextSeqs: [], source });
    }
  }, []);

  // Bidirectional scroll sync: scroll the OTHER view when selection changes
  useEffect(() => {
    if (selection.focusedSeq == null || !selection.source) return;
    const seq = selection.focusedSeq;

    if (selection.source === 'timeline') {
      // Clicked timeline dot → scroll context list to the active item
      const item = contextItemRefs.current.get(seq);
      if (item) item.scrollIntoView({ behavior: 'smooth', block: 'center' });
    } else if (selection.source === 'list') {
      // Clicked list item → blink the timeline dot
      const dot = timelineDotRefs.current.get(seq);
      if (dot) {
        dot.scrollIntoView({ behavior: 'smooth', inline: 'center', block: 'nearest' });
        dot.classList.remove('animate-ping-radar');
        void dot.offsetWidth; // force reflow to re-trigger animation
        dot.classList.add('animate-ping-radar');
      }
    }
  }, [selection.focusedSeq, selection.source]);

  // Filtered events — lane visibility (Solo/Mute)
  const filtered = useMemo(() => {
    return events.filter(e => isLaneVisible(getEventLaneId(e), soloed, muted));
  }, [events, soloed, muted]);

  // Derived sets for O(1) lookups (P1 refactor: single source of truth)
  const contextSeqSet = useMemo(() => new Set(selection.contextSeqs), [selection.contextSeqs]);
  const eventMap = useMemo(() => new Map(events.map(e => [e.seq, e])), [events]);
  const contextEvents = useMemo(() =>
    selection.contextSeqs.map(seq => eventMap.get(seq)).filter(Boolean) as TimelineEvent[],
    [selection.contextSeqs, eventMap],
  );

  // Build session layout: sub-row assignment + capsule bounds for Chat lane
  // Uses `events` (not `filtered`) so layout stays stable when toggling lane visibility
  const sessionLayout = useMemo(() => {
    // Group chat events by session_id
    const bySession = new Map<string, TimelineEvent[]>();
    for (const ev of events) {
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
  }, [events, sessionsMeta]);

  // Build slot layout: group events by slot_id, assign fixed sub-row per slot
  // Uses `events` (not `filtered`) so layout stays stable when toggling lane visibility
  const slotLayout = useMemo(() => {
    const bySlot = new Map<string, TimelineEvent[]>();
    for (const ev of events) {
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
  }, [events]);

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

  // Keep refs in sync for selection handlers
  sessionLayoutRef.current = sessionLayout;
  slotLayoutRef.current = slotLayout;

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
    setViewMode('relative');
    setDailyDate(null);
    const n = Date.now();
    loadedRangeRef.current = null; // Force refetch for new window
    animateToRange({ min: n - windowToMs(w), max: n }, 300);
  }, [animateToRange]);

  // Daily view navigation
  const earliestDate = useMemo(() => toBeijingDate(Date.now() - 7 * 86400_000), []);

  const handleDailyNav = useCallback((dateStr: string) => {
    setDailyDate(dateStr);
    setViewMode('daily');
    setActiveWindow('');
    const { start, end } = beijingDayRange(dateStr);
    loadedRangeRef.current = null;
    animateToRange({ min: start, max: end }, 300);
  }, [animateToRange]);

  const handleDailyPrev = useCallback(() => {
    const current = dailyDate || todayBeijing();
    const prev = shiftDay(current, -1);
    if (prev >= earliestDate) handleDailyNav(prev);
  }, [dailyDate, earliestDate, handleDailyNav]);

  const handleDailyNext = useCallback(() => {
    const current = dailyDate || todayBeijing();
    const next = shiftDay(current, 1);
    if (next <= todayBeijing()) handleDailyNav(next);
  }, [dailyDate, handleDailyNav]);

  // Solo/Mute handlers
  const handleSolo = useCallback((id: string, shift: boolean) => {
    setSoloed(prev => {
      const next = new Set(prev);
      if (shift) {
        // Shift: toggle this lane in the solo set
        if (next.has(id)) next.delete(id); else next.add(id);
      } else {
        // Normal: exclusive solo (click again to clear)
        if (next.has(id) && next.size === 1) next.clear();
        else { next.clear(); next.add(id); }
      }
      // Unmute if soloing
      setMuted(m => { const nm = new Set(m); nm.delete(id); return nm; });
      return next;
    });
  }, []);

  const handleMute = useCallback((id: string) => {
    setMuted(prev => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else {
        next.add(id);
        // Remove from solo if muting
        setSoloed(s => { const ns = new Set(s); ns.delete(id); return ns; });
      }
      return next;
    });
  }, []);

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
            label="CLI/h"
            value={(hourlyStats?.by_type?.find(t => t[0] === 'cli_request_completed')?.[1] ?? 0) + (hourlyStats?.by_type?.find(t => t[0] === 'gemini_request_completed')?.[1] ?? 0) + (hourlyStats?.by_type?.find(t => t[0] === 'codex_request_completed')?.[1] ?? 0)}
            color="text-purple-400"
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

          {/* Reset lane visibility */}
          {(soloed.size > 0 || muted.size > 0) && (
            <button
              onClick={() => { setSoloed(new Set()); setMuted(new Set()); }}
              className="px-2 py-1 text-[10px] font-medium rounded bg-neutral-900 text-neutral-400 hover:text-white transition-colors"
              title="Reset all Solo/Mute"
            >
              All
            </button>
          )}

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

          {/* Daily navigator */}
          <div className="w-px h-5 bg-neutral-800" />
          <div className="flex items-center gap-0.5">
            <button
              onClick={handleDailyPrev}
              disabled={dailyDate ? dailyDate <= earliestDate : false}
              className="p-1 rounded hover:bg-neutral-800 text-neutral-500 hover:text-neutral-300 disabled:opacity-30 disabled:cursor-not-allowed transition-colors"
              title="前一天"
            >
              <ChevronLeft className="w-3.5 h-3.5" />
            </button>

            <div className="relative">
              <span className={cn(
                'px-2 py-1 text-[10px] font-medium rounded cursor-pointer transition-colors flex items-center gap-1',
                viewMode === 'daily' ? 'bg-neutral-800 text-white' : 'text-neutral-500 hover:text-neutral-300',
              )}>
                <Calendar className="w-3 h-3" />
                {dailyDate ? formatDailyLabel(dailyDate) : '日历'}
              </span>
              <input
                type="date"
                className="absolute inset-0 opacity-0 cursor-pointer"
                value={dailyDate || todayBeijing()}
                onChange={(e) => { if (e.target.value) handleDailyNav(e.target.value); }}
                max={todayBeijing()}
                min={earliestDate}
              />
            </div>

            <button
              onClick={handleDailyNext}
              disabled={!dailyDate || dailyDate >= todayBeijing()}
              className="p-1 rounded hover:bg-neutral-800 text-neutral-500 hover:text-neutral-300 disabled:opacity-30 disabled:cursor-not-allowed transition-colors"
              title="后一天"
            >
              <ChevronRight className="w-3.5 h-3.5" />
            </button>
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
          {/* Swimlane labels with Solo/Mute controls */}
          <div className="w-20 shrink-0 border-r border-neutral-800/80 bg-neutral-950 flex flex-col">
            {SWIMLANES.map((lane, i) => {
              const geo = laneGeometry.lanes[i];
              const isSoloed = soloed.has(lane.id);
              const isMuted = muted.has(lane.id);
              const isVisible = isLaneVisible(lane.id, soloed, muted);
              return (
                <div key={lane.id} className={cn(
                  'group relative flex items-center gap-1 px-1.5 text-[10px] font-medium overflow-hidden',
                  'hover:bg-white/[0.03] transition-colors',
                  i < SWIMLANES.length - 1 && 'border-b border-neutral-800/50',
                  !isVisible && 'opacity-40',
                )} style={{ height: `${geo.height}%` }}>
                  {/* Accent dot — lane identity */}
                  <div
                    className={cn('w-1.5 h-1.5 rounded-full shrink-0', lane.accent.dot)}
                    style={{ boxShadow: isVisible ? `0 0 6px ${lane.accent.css}` : 'none', opacity: isVisible ? 0.85 : 0.3 }}
                  />
                  {/* Label */}
                  <span className={cn(
                    'truncate flex-1',
                    isSoloed ? 'text-amber-400' : isMuted ? 'text-neutral-600 line-through' : 'text-neutral-400',
                  )} title={lane.label}>{lane.label}</span>
                  {/* Solo/Mute — right-aligned, appear on hover */}
                  <div className={cn(
                    'absolute right-0.5 top-1/2 -translate-y-1/2 items-center gap-0.5',
                    (isSoloed || isMuted) ? 'flex' : 'hidden group-hover:flex',
                  )}>
                    <button
                      onClick={(e) => handleSolo(lane.id, e.shiftKey)}
                      className={cn(
                        'w-3.5 h-3.5 flex items-center justify-center rounded text-[7px] font-bold transition-all',
                        isSoloed
                          ? 'bg-amber-500 text-black'
                          : 'text-neutral-500 hover:bg-neutral-700 hover:text-neutral-300',
                      )}
                      title={`Solo ${lane.label} (Shift+click to add)`}
                    >S</button>
                    <button
                      onClick={() => handleMute(lane.id)}
                      className={cn(
                        'w-3.5 h-3.5 flex items-center justify-center rounded text-[7px] font-bold transition-all',
                        isMuted
                          ? 'bg-red-500/80 text-white'
                          : 'text-neutral-500 hover:bg-neutral-700 hover:text-neutral-300',
                      )}
                      title={`Mute ${lane.label}`}
                    >M</button>
                  </div>
                </div>
              );
            })}
          </div>

          {/* Timeline canvas — gesture-controlled: scroll to pan, pinch to zoom */}
          <div ref={timelineRef} className="flex-1 relative overflow-hidden touch-none cursor-grab active:cursor-grabbing" style={{ minHeight: 100 + sessionLayout.rowCount * 28 }} onClick={() => setSelection(EMPTY_SELECTION)}>
            {/* Swimlane backgrounds — alternating tint + accent color */}
            {SWIMLANES.map((lane, i) => {
              const geo = laneGeometry.lanes[i];
              const laneHidden = !isLaneVisible(lane.id, soloed, muted);
              return (
                <div
                  key={lane.id}
                  className={cn(
                    'absolute left-0 right-0 transition-opacity',
                    i < SWIMLANES.length - 1 && 'border-b border-neutral-800/40',
                    laneHidden ? 'bg-neutral-900/40' : [
                      i % 2 === 0 && 'bg-white/[0.012]',
                      lane.accent.bg,
                    ],
                  )}
                  style={{
                    top: `${geo.top}%`,
                    height: `${geo.height}%`,
                    opacity: laneHidden ? 0.3 : 1,
                    borderLeft: laneHidden ? 'none' : `2px solid ${lane.accent.css}20`,
                  }}
                />
              );
            })}

            {/* Current time marker — red line at "now" */}
            {viewMode === 'daily' && (
              <div
                className="absolute top-0 bottom-0 w-px bg-red-500/60 z-20 pointer-events-none"
                style={{ '--t-event': Date.now(), left: cssLeft } as React.CSSProperties}
              />
            )}

            {/* Session capsules for Chat lane — positioned via CSS calc() */}
            {isLaneVisible('chat', soloed, muted) && Array.from(sessionLayout.map.entries()).map(([sid, info]) => {
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
              const cStatus = getCapsuleStatus(sid, selection);
              return (
                <div
                  key={`capsule-${sid}`}
                  className={cn(
                    'absolute cursor-pointer overflow-hidden',
                    'transition-[opacity,box-shadow,border-color,transform] duration-200 ease-out',
                    'animate-capsule-enter',
                    cStatus === 'normal' && 'hover:brightness-110 hover:shadow-md hover:shadow-black/30',
                    CAPSULE_STYLES[cStatus],
                  )}
                  style={{
                    '--t-start': actualStart,
                    '--t-end': evtMax,
                    left: 'calc((var(--t-start) - var(--t-min)) / var(--t-range) * 100%)',
                    width: 'calc(max((var(--t-end) - var(--t-start)) / var(--t-range) * 100%, 1.2%))',
                    top: `${yCenter - capsuleH / 2}%`,
                    height: `${capsuleH}%`,
                    backgroundColor: sc.line,
                    border: cStatus === 'selected'
                      ? `2px solid ${sc.line.replace('0.25)', '0.85)')}`
                      : `1px solid ${sc.line.replace('0.25)', '0.45)')}`,
                    borderRadius: '10px',
                    boxShadow: cStatus === 'selected'
                      ? `0 0 14px ${sc.line.replace('0.25)', '0.35)')}, inset 0 1px 0 rgba(255,255,255,0.08)`
                      : 'inset 0 1px 0 rgba(255,255,255,0.06)',
                  } as React.CSSProperties}
                  onClick={(e) => { e.stopPropagation(); selectCapsule(sid); }}
                  title={`Session: ${sid.slice(0, 8)} · ${info.events.length} events`}
                >
                  {/* Top highlight gradient — subtle 3D depth */}
                  <div className="absolute inset-0 bg-gradient-to-b from-white/[0.05] to-transparent pointer-events-none rounded-[10px]" />
                </div>
              );
            })}

            {/* Slot capsules — grouped by slot_id, positioned in Slot lane sub-rows */}
            {isLaneVisible('slot', soloed, muted) && Array.from(slotLayout.map.entries()).map(([slotId, info]) => {
              const times = info.events.map(e => utcMs(e.created_at));
              const evtMin = Math.min(...times);
              const evtMax = Math.max(...times);
              const slotLane = laneGeometry.lanes[SLOT_LANE_IDX];
              const subH = laneGeometry.slotSubRowHeight;
              const yCenter = slotLane.top + subH * (info.row + 0.5);
              const capsuleH = subH * 0.7;
              const lineColor = getSlotLine(slotId);
              const cStatus = getCapsuleStatus(slotId, selection);
              return (
                <div
                  key={`slot-capsule-${slotId}`}
                  className={cn(
                    'absolute cursor-pointer overflow-hidden',
                    'transition-[opacity,box-shadow,border-color,transform] duration-200 ease-out',
                    'animate-capsule-enter',
                    cStatus === 'normal' && 'hover:brightness-110 hover:shadow-md hover:shadow-black/30',
                    CAPSULE_STYLES[cStatus],
                  )}
                  style={{
                    '--t-start': evtMin,
                    '--t-end': evtMax,
                    left: 'calc((var(--t-start) - var(--t-min)) / var(--t-range) * 100%)',
                    width: 'calc(max((var(--t-end) - var(--t-start)) / var(--t-range) * 100%, 1.2%))',
                    top: `${yCenter - capsuleH / 2}%`,
                    height: `${capsuleH}%`,
                    backgroundColor: lineColor,
                    border: cStatus === 'selected'
                      ? `2px solid ${lineColor.replace('0.25)', '0.85)')}`
                      : `1px solid ${lineColor.replace('0.25)', '0.45)')}`,
                    borderRadius: '10px',
                    boxShadow: cStatus === 'selected'
                      ? `0 0 14px ${lineColor.replace('0.25)', '0.35)')}, inset 0 1px 0 rgba(255,255,255,0.08)`
                      : 'inset 0 1px 0 rgba(255,255,255,0.06)',
                  } as React.CSSProperties}
                  onClick={(e) => { e.stopPropagation(); selectSlotCapsule(slotId); }}
                  title={`Slot: ${slotId} · ${info.events.length} events`}
                >
                  {/* Slot label inside capsule */}
                  <div className="absolute inset-0 flex items-center px-1.5 pointer-events-none">
                    <span className="text-[8px] font-medium text-white/40 truncate">{slotId.replace('slot-', '')}</span>
                  </div>
                  {/* Top highlight gradient */}
                  <div className="absolute inset-0 bg-gradient-to-b from-white/[0.05] to-transparent pointer-events-none rounded-[10px]" />
                </div>
              );
            })}

            {/* Event dots — wrapper pattern: 24px hit area + 7px visual dot */}
            {filtered.map((ev) => {
              const y = getY(ev);
              const ec = getEventColor(ev.event_type);
              const dStatus = getDotStatus(ev, selection, contextSeqSet);
              const isInsight = ev.event_type === 'insight_generated';
              const isError = hasError(ev);
              const isChat = isChatEvent(ev.event_type);
              const sessionInfo = isChat && ev.payload?.session_id ? sessionLayout.map.get(ev.payload.session_id) : undefined;
              const sessionColor = sessionInfo ? SESSION_COLORS[sessionInfo.colorIdx] : null;

              return (
                /* Outer wrapper: 24px invisible hit area, handles positioning + click */
                <button
                  key={ev.seq}
                  ref={(el) => { if (el) timelineDotRefs.current.set(ev.seq, el); else timelineDotRefs.current.delete(ev.seq); }}
                  onClick={(e) => { e.stopPropagation(); selectEvent(ev, 'timeline'); }}
                  className={cn(
                    'absolute -translate-x-1/2 -translate-y-1/2 w-6 h-6 flex items-center justify-center',
                    'group/dot cursor-pointer z-[25]',
                    dStatus === 'focused' && 'z-[35]',
                    dStatus === 'highlighted' && 'z-30',
                  )}
                  style={{ '--t-event': utcMs(ev.created_at), left: cssLeft, top: `${y}%` } as React.CSSProperties}
                  title={`${ec.label}: ${eventSummary(ev)}${isChat && ev.payload?.session_id ? `\nSession: ${ev.payload.session_id.slice(0, 8)}` : ''}\n${formatBeijingTime(ev.created_at)}`}
                >
                  {/* Hover confirm halo — expands on mouse enter */}
                  <div className={cn(
                    'absolute inset-0 rounded-full pointer-events-none',
                    'scale-50 opacity-0 transition-all duration-200 ease-out',
                    dStatus === 'normal' && 'group-hover/dot:scale-100 group-hover/dot:opacity-100',
                    ec.bg,
                  )} />
                  {/* Visual dot — stays 7px, animates independently */}
                  <div className={cn(
                    'rounded-full relative',
                    'transition-[transform,box-shadow,ring-color,opacity] duration-200 ease-out',
                    'animate-spring-pop',
                    isInsight ? 'w-3.5 h-3.5' :
                    isError ? 'w-3 h-3' :
                    'w-[7px] h-[7px]',
                    dStatus === 'normal' && !sessionColor && 'ring-2 ring-current/20',
                    dStatus === 'normal' && 'group-hover/dot:scale-[1.8] group-hover/dot:shadow-[0_0_10px_var(--tw-shadow-color)]',
                    ec.dot, ec.glow,
                    sessionColor && dStatus === 'normal' && `ring-[2px] ${sessionColor.ring}`,
                    DOT_STYLES[dStatus],
                  )} />
                </button>
              );
            })}

            {/* Span pair lines — connect started↔completed sharing same span_id */}
            {(() => {
              const spanPairs = new Map<string, TimelineEvent[]>();
              for (const ev of filtered) {
                if (ev.span_id && (ev.event_type === 'cli_request_started' || ev.event_type === 'cli_request_completed' || ev.event_type === 'gemini_request_started' || ev.event_type === 'gemini_request_completed' || ev.event_type === 'codex_request_started' || ev.event_type === 'codex_request_completed')) {
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

            {/* Trace connecting lines — show when in trace scope */}
            {selection.scope === 'trace' && contextEvents.length > 1 && (() => {
              const sorted = [...contextEvents].sort((a, b) => (utcMs(a.created_at) - utcMs(b.created_at)) || (a.seq - b.seq));
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

            {/* Pull-to-refresh indicator — slides in from right via GPU-composited transform */}
            <div
              className="absolute top-0 bottom-0 z-50 pointer-events-none flex items-center justify-center"
              style={{
                right: '-56px',
                width: '56px',
                transform: 'translateX(var(--overscroll-x, 0px))',
                willChange: 'transform',
              }}
            >
              <div className="absolute inset-0 bg-gradient-to-l from-neutral-900/80 to-transparent" />
              <div className={cn(
                'relative rounded-full p-2 border',
                isRefreshing
                  ? 'bg-blue-500/20 border-blue-500/30'
                  : 'bg-neutral-800/60 border-neutral-700/40',
              )}>
                <RefreshCw className={cn(
                  'w-4 h-4',
                  isRefreshing ? 'text-blue-400 animate-spin' : 'text-neutral-400',
                )} style={!isRefreshing ? {
                  transform: 'rotate(calc(var(--overscroll-progress, 0) * 360deg))',
                } : undefined} />
              </div>
            </div>
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

      {/* ── Unified Detail Panel (lower section) ── */}
      <div className="flex-1 min-h-0 border border-neutral-800 rounded-lg bg-neutral-950/50 overflow-auto">
        {selection.focusedSeq != null && selectedEvent ? (
          <div className="h-full animate-in fade-in slide-in-from-bottom-2 duration-200 ease-out fill-mode-both">
            <UnifiedDetailPanel
              selection={selection}
              focusedEvent={selectedEvent}
              sessionsMeta={sessionsMeta}
              filteredEvents={filtered}
              contextEvents={contextEvents}
              onSelectEvent={(ev) => selectEvent(ev, 'list')}
              onClose={() => setSelection(EMPTY_SELECTION)}
              contextItemRefs={contextItemRefs}
            />
          </div>
        ) : (
          <div className="flex items-center justify-center h-full text-neutral-600 text-xs">
            Click an event or session capsule to view details
          </div>
        )}
      </div>
    </div>
  );
}

// ── Sub-components extracted to separate files ──
// UnifiedDetailPanel → ./UnifiedDetailPanel.tsx
// EventSummaryView, StatCard, MiniStat, EventMeta → ./EventSummaryView.tsx
// JsonTreeViewer → ./JsonTreeViewer.tsx
// MarkdownContent → ./MarkdownContent.tsx
// ToolViewers → ./ToolViewers.tsx
