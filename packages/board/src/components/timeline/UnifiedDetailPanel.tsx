import { useState, useEffect, useCallback, useMemo, useRef } from 'react';
import { ArrowUp, ArrowDown, Sparkles } from 'lucide-react';
import { cn } from '@/lib/utils';
import { utcMs, formatBeijingTime } from '@/lib/time';
import type { TimelineEvent, SelectionState, Narration } from './types';
import { getEventColor, getSlotColor, abstractTaskStep } from './helpers';
import { EventSummaryView, EventMeta } from './EventSummaryView';
import { JsonTreeViewer } from './JsonTreeViewer';

export function UnifiedDetailPanel({ selection, focusedEvent, sessionsMeta, filteredEvents, contextEvents, onSelectEvent, onClose, contextItemRefs }: {
  selection: SelectionState;
  focusedEvent: TimelineEvent;
  sessionsMeta: Record<string, { startedAt: string }>;
  filteredEvents: TimelineEvent[];
  contextEvents: TimelineEvent[];
  onSelectEvent: (ev: TimelineEvent) => void;
  onClose: () => void;
  contextItemRefs: React.MutableRefObject<Map<number, HTMLButtonElement>>;
}) {
  const [tab, setTab] = useState<'summary' | 'payload' | 'meta'>('summary');
  const [narrations, setNarrations] = useState<Map<number, Narration>>(new Map());
  const listRef = useRef<HTMLDivElement>(null);
  const lastScrolledSeqRef = useRef<number | null>(null);

  // Reset tab + scroll when scope changes (replaces key-based remount)
  const scopeKey = `${selection.scope}-${selection.scopeId}`;
  const prevScopeRef = useRef(scopeKey);
  useEffect(() => {
    if (prevScopeRef.current !== scopeKey) {
      prevScopeRef.current = scopeKey;
      setTab('summary');
      lastScrolledSeqRef.current = null;
      listRef.current?.scrollTo({ top: 0 });
    }
  }, [scopeKey]);

  // Fetch narrations when in session or trace scope (trace_id is often session_id)
  const sessionId = (selection.scope === 'session' || selection.scope === 'trace') ? selection.scopeId : null;
  useEffect(() => {
    if (!sessionId) { setNarrations(new Map()); return; }
    let cancelled = false;
    fetch(`/api/system/narrations?session_id=${encodeURIComponent(sessionId)}`)
      .then(r => r.json())
      .then(data => {
        if (cancelled || !data.narrations) return;
        const map = new Map<number, Narration>();
        for (const n of data.narrations) map.set(n.message_id, n);
        setNarrations(map);
      })
      .catch(() => {});
    return () => { cancelled = true; };
  }, [sessionId]);

  // Context events sorted by time (derived from parent's eventMap via contextEvents prop)
  const contextSorted = useMemo(() =>
    [...contextEvents].sort((a, b) => utcMs(a.created_at) - utcMs(b.created_at)),
    [contextEvents],
  );

  const activeIdx = useMemo(() => {
    return contextSorted.findIndex(e => e.seq === focusedEvent.seq);
  }, [contextSorted, focusedEvent.seq]);

  // Narration for focused event
  const narration = useMemo(() => {
    if (narrations.size === 0) return null;
    const n = narrations.get(focusedEvent.seq);
    if (n) return n;
    const msgId = focusedEvent.payload?.message_id;
    return msgId ? narrations.get(msgId) ?? null : null;
  }, [narrations, focusedEvent]);

  const stepInfo = useMemo(() => {
    if (narration) return { title: narration.step_title, subtitle: narration.step_result, intent: narration.step_intent, isAi: true };
    return { ...abstractTaskStep(focusedEvent), isAi: false };
  }, [narration, focusedEvent]);

  // Session duration
  const scopeStats = useMemo(() => {
    if (selection.scope !== 'session' || contextSorted.length === 0 || !sessionId) return null;
    const times = contextSorted.map(e => utcMs(e.created_at));
    const meta = sessionsMeta[sessionId];
    const tStart = meta?.startedAt ? Math.min(utcMs(meta.startedAt), times[0]) : times[0];
    const tEnd = times[times.length - 1];
    const duration = tEnd - tStart;
    const durationStr = duration < 60_000 ? `${(duration / 1000).toFixed(0)}s`
      : duration < 3600_000 ? `${(duration / 60_000).toFixed(1)}m`
      : `${(duration / 3600_000).toFixed(1)}h`;
    return { id: sessionId, steps: contextSorted.length, durationStr };
  }, [selection.scope, contextSorted, sessionId, sessionsMeta]);

  // Keyboard navigation — unified J/K
  const navigateInContext = useCallback((dir: 'next' | 'prev') => {
    if (contextSorted.length === 0) return;
    const curIdx = activeIdx >= 0 ? activeIdx : 0;
    const newIdx = dir === 'next' ? Math.min(curIdx + 1, contextSorted.length - 1) : Math.max(curIdx - 1, 0);
    if (newIdx !== curIdx) onSelectEvent(contextSorted[newIdx]);
  }, [contextSorted, activeIdx, onSelectEvent]);

  // Global navigation for events without context
  // filteredEvents is sorted descending by seq (newest first), so
  // 'next' (J/↓) = forward in time = lower index, 'prev' (K/↑) = backward = higher index
  const navigateGlobal = useCallback((dir: 'next' | 'prev') => {
    const idx = filteredEvents.findIndex(e => e.seq === focusedEvent.seq);
    if (idx < 0) return;
    const newIdx = dir === 'next' ? Math.max(idx - 1, 0) : Math.min(idx + 1, filteredEvents.length - 1);
    if (newIdx !== idx) onSelectEvent(filteredEvents[newIdx]);
  }, [filteredEvents, focusedEvent.seq, onSelectEvent]);

  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if (['INPUT', 'TEXTAREA', 'SELECT'].includes((e.target as HTMLElement).tagName)) return;
      const hasContext = contextSorted.length > 0;
      if (e.key === 'j' || e.key === 'ArrowDown') { e.preventDefault(); hasContext ? navigateInContext('next') : navigateGlobal('next'); }
      if (e.key === 'k' || e.key === 'ArrowUp') { e.preventDefault(); hasContext ? navigateInContext('prev') : navigateGlobal('prev'); }
      if (e.key === 'Escape') { e.preventDefault(); onClose(); }
    };
    window.addEventListener('keydown', handler);
    return () => window.removeEventListener('keydown', handler);
  }, [navigateInContext, navigateGlobal, contextSorted.length, onClose]);

  // Auto-scroll active item in context list.
  // Uses "consumed tracker" pattern: only scroll once per focusedSeq to avoid
  // re-scrolling on data refreshes (window switch, polling) that don't change the focus.
  useEffect(() => {
    if (activeIdx === -1) return;
    if (lastScrolledSeqRef.current === focusedEvent.seq) return; // Already scrolled for this event
    const raf = requestAnimationFrame(() => {
      const el = listRef.current?.querySelector('[data-active="true"]');
      if (el) {
        el.scrollIntoView({ behavior: 'smooth', block: 'nearest' });
        lastScrolledSeqRef.current = focusedEvent.seq;
      }
    });
    return () => cancelAnimationFrame(raf);
  }, [activeIdx, focusedEvent.seq]);

  const ec = getEventColor(focusedEvent.event_type);
  const hasContext = contextSorted.length > 0;

  return (
    <div className="flex h-full min-h-0">
      {/* ── Left: Context Browser (Master) ── */}
      {hasContext && (
        <div ref={listRef} className="w-64 shrink-0 border-r border-neutral-800 bg-neutral-900/40 flex flex-col overflow-y-auto">
          {/* Header: scope indicator + ID */}
          <div className="sticky top-0 bg-neutral-900/95 backdrop-blur-sm border-b border-neutral-800 z-10 px-3 py-2">
            <div className="flex items-center gap-2">
              <span className="text-[10px] font-medium text-neutral-500 uppercase tracking-wider">
                {selection.scope === 'session' ? 'Steps' : 'Trace'}
              </span>
              <span className="text-[10px] text-neutral-600 font-mono ml-auto">{activeIdx >= 0 ? activeIdx + 1 : '—'}/{contextSorted.length}</span>
            </div>
            {selection.scopeId && (
              <div className="text-[9px] text-neutral-600 font-mono mt-1 truncate" title={selection.scopeId}>
                {selection.scope === 'session' ? 'Session' : 'Trace'}: {selection.scopeId.slice(0, 12)}
              </div>
            )}
          </div>

          {/* Context list */}
          <div className="p-1.5 space-y-0.5 flex-1">
            {contextSorted.map((ev) => {
              const evEc = getEventColor(ev.event_type);
              const isActive = ev.seq === focusedEvent.seq;
              // Get narrated title for session scope
              const evNarration = narrations.get(ev.seq) || (ev.payload?.message_id ? narrations.get(ev.payload.message_id) : undefined);
              const title = evNarration ? evNarration.step_title : abstractTaskStep(ev).title;
              const subtitle = evNarration ? evNarration.step_result : abstractTaskStep(ev).subtitle;
              return (
                <button
                  key={ev.seq}
                  data-active={isActive}
                  ref={(el) => { if (el) contextItemRefs.current.set(ev.seq, el); else contextItemRefs.current.delete(ev.seq); }}
                  onClick={() => onSelectEvent(ev)}
                  className={cn(
                    'w-full text-left px-2.5 py-2 rounded-md flex gap-2.5 items-start transition-all duration-150',
                    isActive
                      ? 'bg-blue-500/10 border border-blue-500/25'
                      : 'border border-transparent opacity-60 hover:opacity-100 hover:bg-neutral-800/50',
                  )}
                >
                  <div className={cn('w-2 h-2 rounded-full shrink-0 mt-1', evEc.dot)} />
                  <div className="flex-1 min-w-0">
                    <div className={cn('text-xs font-medium truncate', isActive ? 'text-blue-100' : 'text-neutral-300')}>
                      {title}
                    </div>
                    {subtitle && <div className="text-[10px] text-neutral-600 truncate mt-0.5">{subtitle}</div>}
                  </div>
                  <div className="flex flex-col items-end shrink-0 mt-0.5">
                    <span className="text-[9px] text-neutral-600">{formatBeijingTime(ev.created_at)}</span>
                    <span className="text-[8px] text-neutral-700 font-mono">#{ev.seq}</span>
                  </div>
                </button>
              );
            })}
          </div>

          {/* Footer: scope stats */}
          {scopeStats && (
            <div className="border-t border-neutral-800/60 px-3 py-2 flex items-center gap-3 text-[10px] text-neutral-600">
              <span className="font-mono">{scopeStats.id.slice(0, 8)}</span>
              <span>{scopeStats.steps} steps</span>
              <span>{scopeStats.durationStr}</span>
            </div>
          )}
        </div>
      )}

      {/* ── Right: Event Detail (Detail) ── */}
      <div className="flex-1 flex flex-col min-w-0">
        {/* Header: event info + IDs + narration */}
        <div className="p-4 border-b border-neutral-800/60 bg-neutral-900/30">
          <div className="flex items-center gap-2 mb-1">
            <div className={cn('w-3 h-3 rounded-full', ec.dot)} />
            <span className={cn('text-sm font-medium', ec.text)}>{stepInfo.isAi ? stepInfo.title : ec.label}</span>
            {stepInfo.isAi && <span className="shrink-0" data-tooltip="AI 解说"><Sparkles className="w-3 h-3 text-purple-400" /></span>}
            {focusedEvent.payload?.slot_id && (() => {
              const sc = getSlotColor(focusedEvent.payload.slot_id);
              return sc ? <span className={cn('text-[10px] px-1.5 py-0.5 rounded', sc.badge)}>{focusedEvent.payload.slot_id}</span> : null;
            })()}
            <span className="text-[10px] text-neutral-600 font-mono ml-auto">{formatBeijingTime(focusedEvent.created_at)}</span>
          </div>
          {/* IDs row */}
          <div className="flex items-center gap-3 text-[10px] text-neutral-600 font-mono mt-1">
            <span title={`Seq: ${focusedEvent.seq}`}>#{focusedEvent.seq}</span>
            {focusedEvent.payload?.session_id && (
              <span title={focusedEvent.payload.session_id}>Session: {(focusedEvent.payload.session_id as string).slice(0, 8)}</span>
            )}
            {focusedEvent.trace_id && (
              <span title={focusedEvent.trace_id}>Trace: {focusedEvent.trace_id.slice(0, 8)}</span>
            )}
            {focusedEvent.payload?.message_id != null && (
              <span>MsgID: {focusedEvent.payload.message_id}</span>
            )}
            {focusedEvent.span_id && (
              <span title={focusedEvent.span_id}>Span: {focusedEvent.span_id.slice(0, 8)}</span>
            )}
            {focusedEvent.parent_span_id && (
              <span title={focusedEvent.parent_span_id} className="text-teal-600">Parent: {focusedEvent.parent_span_id.slice(0, 8)}</span>
            )}
          </div>
          {stepInfo.subtitle && <p className="text-xs text-neutral-400 truncate mt-1 font-mono">{stepInfo.subtitle}</p>}
          {stepInfo.intent && <div className="mt-2 text-xs text-neutral-500 border-l-2 border-neutral-700 pl-2.5">{stepInfo.intent}</div>}
        </div>

        {/* Tabs */}
        <div className="flex items-center gap-1 px-4 pt-2 border-b border-neutral-800 pb-2">
          {(['summary', 'payload', 'meta'] as const).map(t => (
            <button
              key={t}
              onClick={() => setTab(t)}
              className={cn(
                'px-3 py-1.5 text-xs font-medium rounded-md transition-colors capitalize',
                tab === t ? 'bg-neutral-800 text-white' : 'text-neutral-500 hover:text-neutral-300 hover:bg-neutral-800/50',
              )}
            >
              {t}
            </button>
          ))}

          {/* Navigation — only for global scope (no context list) */}
          {!hasContext && (() => {
            const idx = filteredEvents.findIndex(e => e.seq === focusedEvent.seq);
            return (
              <div className="ml-auto flex items-center gap-1.5 text-[10px]">
                <button onClick={() => navigateGlobal('prev')} disabled={idx < 0 || idx >= filteredEvents.length - 1}
                  className="p-1 rounded hover:bg-neutral-800 disabled:opacity-30 text-neutral-400 hover:text-white transition-colors" title="Previous (K / ↑)">
                  <ArrowUp className="w-3.5 h-3.5" />
                </button>
                <span className="text-neutral-500 font-mono tabular-nums min-w-[4ch] text-center">
                  {idx >= 0 ? `${filteredEvents.length - idx}/${filteredEvents.length}` : '—'}
                </span>
                <button onClick={() => navigateGlobal('next')} disabled={idx <= 0}
                  className="p-1 rounded hover:bg-neutral-800 disabled:opacity-30 text-neutral-400 hover:text-white transition-colors" title="Next (J / ↓)">
                  <ArrowDown className="w-3.5 h-3.5" />
                </button>
              </div>
            );
          })()}
        </div>

        {/* Content */}
        <div className="flex-1 overflow-y-auto p-4 min-h-0">
          {tab === 'summary' ? (
            <EventSummaryView event={focusedEvent} />
          ) : tab === 'payload' ? (
            <JsonTreeViewer data={focusedEvent.payload} />
          ) : (
            <EventMeta event={focusedEvent} />
          )}
        </div>
      </div>
    </div>
  );
}
