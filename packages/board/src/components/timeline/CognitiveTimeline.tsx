'use client';

import { useState, useEffect, useCallback, useMemo, useRef } from 'react';
import { RefreshCw } from 'lucide-react';
import { cn } from '@/lib/utils';
import { utcMs, formatBeijingTime, beijingDayRange } from '@/lib/time';
import { useEventInvalidation } from '../../hooks/useEventStream';
import { useTimelineGestures } from '../../hooks/useTimelineGestures';

// ── Local modules ──
import type { TimelineEvent } from './types';
import { EMPTY_SELECTION } from './types';
import { SWIMLANES, SLOT_LANE_IDX, CSS_LEFT } from './constants';
import {
  getSwimlane, isLaneVisible, isChatEvent,
  hasError, windowToMs, loadViewState,
} from './helpers';
import { UnifiedDetailPanel } from './UnifiedDetailPanel';

// ── Refactored modules ──
import { useTimelineStore } from './stores/timelineStore';
import { TimelineHeader } from './ui/TimelineHeader';
import { LaneSidebar } from './ui/LaneSidebar';
import { EventDot } from './ui/EventDot';
import { SessionCapsule } from './ui/SessionCapsule';
import { SlotCapsule } from './ui/SlotCapsule';
import { SpanLines } from './ui/SpanLines';
import { TraceLines } from './ui/TraceLines';
import { CrossLaneLinks } from './ui/CrossLaneLinks';
import { TimeAxis, Minimap } from './ui/TimeAxisBar';

// ── Main Component ─────────────────────────────────────────

export function CognitiveTimeline() {
  const timelineVersion = useEventInvalidation('timeline');

  // ── Individual store selectors (no full-store subscription) ──
  const events = useTimelineStore(s => s.events);
  const sessionsMeta = useTimelineStore(s => s.sessionsMeta);
  const selection = useTimelineStore(s => s.selection);
  const filtered = useTimelineStore(s => s.filtered);
  const sessionLayout = useTimelineStore(s => s.sessionLayout);
  const slotLayout = useTimelineStore(s => s.slotLayout);
  const laneGeometry = useTimelineStore(s => s.laneGeometry);
  const hoveredLane = useTimelineStore(s => s.hoveredLane);
  const soloed = useTimelineStore(s => s.soloed);
  const muted = useTimelineStore(s => s.muted);
  const viewMode = useTimelineStore(s => s.viewMode);
  const dailyDate = useTimelineStore(s => s.dailyDate);

  const setEvents = useTimelineStore(s => s.setEvents);
  const mergeEvents = useTimelineStore(s => s.mergeEvents);
  const setSessionsMeta = useTimelineStore(s => s.setSessionsMeta);
  const setStats = useTimelineStore(s => s.setStats);
  const setHourlyStats = useTimelineStore(s => s.setHourlyStats);
  const setSelection = useTimelineStore(s => s.setSelection);
  const setHoveredLane = useTimelineStore(s => s.setHoveredLane);
  const setLoading = useTimelineStore(s => s.setLoading);
  const setActiveWindow = useTimelineStore(s => s.setActiveWindow);
  const setViewMode = useTimelineStore(s => s.setViewMode);
  const setDailyDate = useTimelineStore(s => s.setDailyDate);
  const updateEventSummary = useTimelineStore(s => s.updateEventSummary);

  const savedView = useMemo(() => loadViewState(), []);
  const [searchInput, setSearchInput] = useState('');

  // Derived: focused event (reference-stable)
  const selectedEventRef = useRef<TimelineEvent | null>(null);
  const selectedEvent = useMemo(() => {
    if (selection.focusedSeq == null) { selectedEventRef.current = null; return null; }
    const found = events.find(e => e.seq === selection.focusedSeq) ?? null;
    if (!found) { selectedEventRef.current = null; return null; }
    const prev = selectedEventRef.current;
    if (prev && prev.seq === found.seq && prev.summary === found.summary && prev.trace_id === found.trace_id) return prev;
    selectedEventRef.current = found;
    return found;
  }, [selection.focusedSeq, events]);

  const selectionRef = useRef(selection);
  selectionRef.current = selection;

  // Ref maps for bidirectional scroll sync
  const timelineDotRefs = useRef<Map<number, HTMLButtonElement>>(new Map());
  const contextItemRefs = useRef<Map<number, HTMLButtonElement>>(new Map());

  // ── Gesture-controlled timeline ──
  const gestureMaxTime = useMemo(() => {
    if (viewMode === 'daily' && dailyDate) {
      const { end } = beijingDayRange(dailyDate);
      return Math.max(end, Date.now() + 5 * 60_000);
    }
    return Date.now() + 5 * 60_000;
  }, [viewMode, dailyDate]);

  const now = Date.now();
  // eslint-disable-next-line react-hooks/exhaustive-deps
  const initialRange = useMemo(() => {
    if (savedView.dailyDate) {
      const { start, end } = beijingDayRange(savedView.dailyDate);
      return { min: start, max: end };
    }
    return { min: now - windowToMs(savedView.activeWindow), max: now };
  }, []);

  const loadedRangeRef = useRef<{ min: number; max: number } | null>(null);
  const abortRef = useRef<AbortController | null>(null);
  const requestIdRef = useRef(0);

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

  // ── Data fetching ──
  const fetchForRange = useCallback(async (
    rangeMin: number, rangeMax: number,
    opts?: { signal?: AbortSignal; silent?: boolean; replace?: boolean },
  ) => {
    const duration = rangeMax - rangeMin;
    const bufMin = rangeMin - duration;
    const bufMax = rangeMax + duration;
    const since = new Date(bufMin).toISOString().replace('T', ' ').slice(0, 19);
    const until = new Date(bufMax).toISOString().replace('T', ' ').slice(0, 19);

    const searchQuery = useTimelineStore.getState().searchQuery;
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
        mergeEvents(incoming);
      }
      if (evRes.value.sessions) {
        setSessionsMeta(evRes.value.sessions);
      }
    }
    if (stRes.status === 'fulfilled' && !stRes.value.error) setStats(stRes.value);
    if (hrRes.status === 'fulfilled' && !hrRes.value.error) setHourlyStats(hrRes.value);

    const prevLoaded = loadedRangeRef.current;
    loadedRangeRef.current = prevLoaded
      ? { min: Math.min(prevLoaded.min, bufMin), max: Math.max(prevLoaded.max, bufMax) }
      : { min: bufMin, max: bufMax };
  }, [setEvents, mergeEvents, setSessionsMeta, setStats, setHourlyStats]);

  // Preload on range change
  useEffect(() => {
    const { min, max } = timeRange;
    const loaded = loadedRangeRef.current;

    if (!loaded) {
      abortRef.current?.abort();
      const controller = new AbortController();
      abortRef.current = controller;
      setLoading(true);
      fetchForRange(min, max, { signal: controller.signal, replace: true })
        .catch(() => {})
        .finally(() => setLoading(false));
      return () => controller.abort();
    }

    const bufferLeft = min - loaded.min;
    const bufferRight = loaded.max - max;
    const visibleDuration = max - min;
    const threshold = visibleDuration * 0.5;
    if (bufferLeft > threshold && bufferRight > threshold) return;

    abortRef.current?.abort();
    const controller = new AbortController();
    abortRef.current = controller;
    fetchForRange(min, max, { signal: controller.signal, silent: true }).catch(() => {});
    return () => controller.abort();
  }, [timeRange, fetchForRange, timelineVersion, setLoading]);

  // Summary update via CustomEvent
  useEffect(() => {
    const handler = (e: Event) => {
      const { target_seq, summary } = (e as CustomEvent).detail;
      if (target_seq && summary) updateEventSummary(target_seq, summary);
    };
    globalThis.addEventListener('timeline-summary-update', handler);
    return () => globalThis.removeEventListener('timeline-summary-update', handler);
  }, [updateEventSummary]);

  // ── Selection handlers ──
  const selectCapsule = useCallback((sid: string) => {
    setSelection(prev => {
      if (prev.scope === 'session' && prev.scopeId === sid) return EMPTY_SELECTION;
      const sessionEvents = sessionLayout.map.get(sid)?.events || [];
      const sorted = [...sessionEvents].sort((a, b) => utcMs(a.created_at) - utcMs(b.created_at));
      return { scope: 'session', scopeId: sid, focusedSeq: sorted[0]?.seq ?? null, contextSeqs: sorted.map(e => e.seq), source: 'timeline' };
    });
  }, [setSelection, sessionLayout]);

  const selectSlotCapsule = useCallback((slotId: string) => {
    setSelection(prev => {
      if (prev.scope === 'slot' && prev.scopeId === slotId) return EMPTY_SELECTION;
      const slotEvents = slotLayout.map.get(slotId)?.events || [];
      const sorted = [...slotEvents].sort((a, b) => utcMs(a.created_at) - utcMs(b.created_at));
      return { scope: 'slot', scopeId: slotId, focusedSeq: sorted[0]?.seq ?? null, contextSeqs: sorted.map(e => e.seq), source: 'timeline' };
    });
  }, [setSelection, slotLayout]);

  const selectEvent = useCallback(async (ev: TimelineEvent, source: 'timeline' | 'list') => {
    const rid = ++requestIdRef.current;
    const sessionId = ev.payload?.session_id as string | undefined;
    const traceId = ev.trace_id;

    if (traceId) {
      const cur = selectionRef.current;
      if (cur.scope === 'trace' && cur.scopeId === traceId && cur.contextSeqs.length > 0) {
        setSelection(prev => ({ ...prev, focusedSeq: ev.seq, source }));
        return;
      }
      setSelection({ scope: 'trace', scopeId: traceId, focusedSeq: ev.seq, contextSeqs: [], source });
      try {
        const res = await fetch(`/api/timeline/events?traceId=${traceId}&limit=50`);
        if (requestIdRef.current !== rid) return;
        const data = await res.json();
        const raw = data.events;
        const incoming: TimelineEvent[] = Array.isArray(raw) ? raw : (raw?.events || []);
        const incomingSeqs = incoming.map(e => e.seq);
        setSelection(prev => prev.scopeId === traceId ? { ...prev, contextSeqs: incomingSeqs } : prev);
        if (incoming.length > 0) mergeEvents(incoming);
      } catch { /* keep selection */ }
    } else if (sessionId) {
      const cur = selectionRef.current;
      if (cur.scope === 'session' && cur.scopeId === sessionId && cur.contextSeqs.length > 0) {
        setSelection(prev => ({ ...prev, focusedSeq: ev.seq, source }));
        return;
      }
      const sessionEvents = sessionLayout.map.get(sessionId)?.events || [];
      const sorted = [...sessionEvents].sort((a, b) => utcMs(a.created_at) - utcMs(b.created_at));
      setSelection({ scope: 'session', scopeId: sessionId, focusedSeq: ev.seq, contextSeqs: sorted.map(e => e.seq), source });
    } else {
      setSelection({ scope: 'global', scopeId: null, focusedSeq: ev.seq, contextSeqs: [], source });
    }
  }, [setSelection, mergeEvents, sessionLayout]);

  // Bidirectional scroll sync
  useEffect(() => {
    if (selection.focusedSeq == null || !selection.source) return;
    const seq = selection.focusedSeq;
    if (selection.source === 'timeline') {
      const item = contextItemRefs.current.get(seq);
      if (item) item.scrollIntoView({ behavior: 'smooth', block: 'center' });
    } else if (selection.source === 'list') {
      const dot = timelineDotRefs.current.get(seq);
      if (dot) {
        dot.scrollIntoView({ behavior: 'smooth', inline: 'center', block: 'nearest' });
        dot.classList.remove('animate-ping-radar');
        void dot.offsetWidth;
        dot.classList.add('animate-ping-radar');
      }
    }
  }, [selection.focusedSeq, selection.source]);

  // ── Derived data ──
  const eventMap = useMemo(() => new Map(events.map(e => [e.seq, e])), [events]);
  const contextEvents = useMemo(() =>
    selection.contextSeqs.map(seq => eventMap.get(seq)).filter(Boolean) as TimelineEvent[],
    [selection.contextSeqs, eventMap],
  );

  // ── Y position helpers ──
  const getChatY = useCallback((ev: TimelineEvent): number => {
    const sid = ev.payload?.session_id;
    const info = sid ? sessionLayout.map.get(sid) : undefined;
    const chatLane = laneGeometry.lanes[SWIMLANES.findIndex(s => s.id === 'chat')];
    return chatLane.top + laneGeometry.chatSubRowHeight * ((info?.row ?? 0) + 0.5);
  }, [sessionLayout, laneGeometry]);

  const getSlotY = useCallback((ev: TimelineEvent): number => {
    const sid = ev.payload?.slot_id;
    const info = sid ? slotLayout.map.get(sid) : undefined;
    const slotLane = laneGeometry.lanes[SLOT_LANE_IDX];
    return slotLane.top + laneGeometry.slotSubRowHeight * ((info?.row ?? 0) + 0.5);
  }, [slotLayout, laneGeometry]);

  const getY = useCallback((ev: TimelineEvent): number => {
    if (ev.payload?.slot_id) return getSlotY(ev);
    if (isChatEvent(ev.event_type)) return getChatY(ev);
    const laneIdx = getSwimlane(ev);
    const lane = laneGeometry.lanes[laneIdx];
    return lane.top + lane.height * 0.5;
  }, [getChatY, getSlotY, laneGeometry]);

  // ── Navigation handlers ──
  const handleWindowChange = useCallback((w: string) => {
    setActiveWindow(w);
    setViewMode('relative');
    setDailyDate(null);
    const n = Date.now();
    loadedRangeRef.current = null;
    animateToRange({ min: n - windowToMs(w), max: n }, 300);
  }, [animateToRange, setActiveWindow, setViewMode, setDailyDate]);

  const handleDailyNav = useCallback((dateStr: string) => {
    setDailyDate(dateStr);
    setViewMode('daily');
    setActiveWindow('');
    const { start, end } = beijingDayRange(dateStr);
    loadedRangeRef.current = null;
    animateToRange({ min: start, max: end }, 300);
  }, [animateToRange, setDailyDate, setViewMode, setActiveWindow]);

  const handleRefresh = useCallback(() => {
    loadedRangeRef.current = null;
    setLoading(true);
    fetchForRange(timeRange.min, timeRange.max, { replace: true }).catch(() => {}).finally(() => setLoading(false));
  }, [fetchForRange, timeRange, setLoading]);

  // ── Position helpers for span/trace lines ──
  const getX = useCallback((dateStr: string) => {
    const t = utcMs(dateStr);
    const { min, max } = timeRange;
    const range = max - min;
    return range <= 0 ? 50 : ((t - min) / range) * 100;
  }, [timeRange]);

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

  // Density histogram
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
      if (idx >= 0) { counts[idx]++; if (hasError(ev)) errorCounts[idx]++; }
    }
    const maxCount = Math.max(...counts, 1);
    return counts.map((c, i) => ({ height: (c / maxCount) * 100, errors: errorCounts[i], idx: i }));
  }, [events, timeRange]);

  // Hovered lane detection via mouse Y
  const handleTimelineMouseMove = useCallback((e: React.MouseEvent<HTMLDivElement>) => {
    const rect = e.currentTarget.getBoundingClientRect();
    const yPct = ((e.clientY - rect.top) / rect.height) * 100;
    for (let i = 0; i < SWIMLANES.length; i++) {
      const geo = laneGeometry.lanes[i];
      if (geo && yPct >= geo.top && yPct < geo.top + geo.height) {
        setHoveredLane(SWIMLANES[i].id);
        return;
      }
    }
    setHoveredLane(null);
  }, [laneGeometry, setHoveredLane]);

  // ── Precomputed dot data (stable between renders when layout doesn't change) ──
  const dotData = useMemo(() => filtered.map(ev => {
    const isChat = isChatEvent(ev.event_type);
    const sessionInfo = isChat && ev.payload?.session_id ? sessionLayout.map.get(ev.payload.session_id) : undefined;
    return {
      ev,
      y: getY(ev),
      sessionColorIdx: sessionInfo ? sessionInfo.colorIdx : null,
    };
  }), [filtered, getY, sessionLayout]);

  // Stable ref callback for EventDot registration (scroll sync)
  const registerDotRef = useCallback((seq: number, el: HTMLButtonElement | null) => {
    if (el) timelineDotRefs.current.set(seq, el);
    else timelineDotRefs.current.delete(seq);
  }, []);

  // Stable event select for timeline dots
  const selectTimelineEvent = useCallback((ev: TimelineEvent) => selectEvent(ev, 'timeline'), [selectEvent]);

  // ── Render ──
  return (
    <div className="flex-1 flex flex-col min-h-0 px-4 sm:px-8 pb-4 gap-3">
      {/* ── Top Bar ── */}
      <TimelineHeader
        searchInput={searchInput}
        setSearchInput={setSearchInput}
        onRefresh={handleRefresh}
        onWindowChange={handleWindowChange}
        onDailyNav={handleDailyNav}
      />

      {/* ── Horizontal Timeline ── */}
      <div className="border border-neutral-800 rounded-lg bg-neutral-950/50 flex flex-col" style={{ minHeight: 200 }}>
        <div className="flex flex-1 min-h-0">
          <LaneSidebar />

          {/* Timeline canvas */}
          <div
            ref={timelineRef}
            className="flex-1 relative overflow-hidden touch-none cursor-grab active:cursor-grabbing"
            style={{ minHeight: 100 + sessionLayout.rowCount * 28 }}
            onClick={() => setSelection(EMPTY_SELECTION)}
            onMouseMove={handleTimelineMouseMove}
            onMouseLeave={() => setHoveredLane(null)}
          >
            {/* Swimlane backgrounds */}
            {SWIMLANES.map((lane, i) => {
              const geo = laneGeometry.lanes[i];
              const laneHidden = !isLaneVisible(lane.id, soloed, muted);
              const isHovered = hoveredLane === lane.id;
              return (
                <div
                  key={lane.id}
                  className={cn(
                    'absolute left-0 right-0 transition-[opacity,background-color] duration-150',
                    i < SWIMLANES.length - 1 && 'border-b border-neutral-800/40',
                    laneHidden ? 'bg-neutral-900/40' : [i % 2 === 0 && 'bg-white/[0.012]', lane.accent.bg],
                  )}
                  style={{
                    top: `${geo.top}%`, height: `${geo.height}%`,
                    opacity: laneHidden ? 0.3 : 1,
                    borderLeft: laneHidden ? 'none' : `2px solid ${lane.accent.css}${isHovered ? '50' : '20'}`,
                    backgroundColor: isHovered && !laneHidden ? `${lane.accent.css}08` : undefined,
                  }}
                />
              );
            })}

            {/* Current time marker */}
            {viewMode === 'daily' && (
              <div
                className="absolute top-0 bottom-0 w-px bg-red-500/60 z-20 pointer-events-none"
                style={{ '--t-event': Date.now(), left: CSS_LEFT } as React.CSSProperties}
              />
            )}

            {/* Chat session capsules */}
            {isLaneVisible('chat', soloed, muted) && Array.from(sessionLayout.map.entries()).map(([sid, info]) => (
              <SessionCapsule
                key={`capsule-${sid}`}
                sid={sid}
                info={info}
                meta={sessionsMeta[sid]}
                chatLaneTop={laneGeometry.lanes[0].top}
                chatSubRowHeight={laneGeometry.chatSubRowHeight}
                onSelect={selectCapsule}
              />
            ))}

            {/* Slot capsules */}
            {isLaneVisible('slot', soloed, muted) && Array.from(slotLayout.map.entries()).map(([slotId, info]) => (
              <SlotCapsule
                key={`slot-capsule-${slotId}`}
                slotId={slotId}
                info={info}
                slotLaneTop={laneGeometry.lanes[SLOT_LANE_IDX].top}
                slotSubRowHeight={laneGeometry.slotSubRowHeight}
                onSelect={selectSlotCapsule}
              />
            ))}

            {/* Event dots */}
            {dotData.map(({ ev, y, sessionColorIdx }) => (
              <EventDot
                key={ev.seq}
                ev={ev}
                y={y}
                sessionColorIdx={sessionColorIdx}
                onSelect={selectTimelineEvent}
                onRegisterRef={registerDotRef}
              />
            ))}

            {/* Span pair lines */}
            <SpanLines
              filtered={filtered}
              getX={getX}
              getY={getY}
              selectedSpanId={selectedEvent?.span_id ?? null}
            />

            {/* Trace connecting lines */}
            <TraceLines
              contextEvents={contextEvents}
              scope={selection.scope}
              getX={getX}
              getY={getY}
            />

            {/* Cross-lane causal links (tool call → LLM request) */}
            <CrossLaneLinks
              filtered={filtered}
              selection={selection}
              getX={getX}
              getY={getY}
            />

            {/* Pull-to-refresh indicator — hidden unless actively overscrolling */}
            <div
              className="absolute top-0 bottom-0 z-50 pointer-events-none flex items-center justify-center"
              style={{ right: '-56px', width: '56px', transform: 'translateX(var(--overscroll-x, 0px))', opacity: 'calc(clamp(0, var(--overscroll-progress, 0) * 10, 1))', willChange: 'transform, opacity' }}
            >
              <div className="absolute inset-0 bg-gradient-to-l from-neutral-900/80 to-transparent" />
              <div className={cn('relative rounded-full p-2 border', isRefreshing ? 'bg-blue-500/20 border-blue-500/30' : 'bg-neutral-800/60 border-neutral-700/40')}>
                <RefreshCw className={cn('w-4 h-4', isRefreshing ? 'text-blue-400 animate-spin' : 'text-neutral-400')} style={!isRefreshing ? { transform: 'rotate(calc(var(--overscroll-progress, 0) * 360deg))' } : undefined} />
              </div>
            </div>
          </div>
        </div>

        <TimeAxis ticks={timeTicks} />
        <Minimap histogram={histogram} />
      </div>

      {/* ── Detail Panel ── */}
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
