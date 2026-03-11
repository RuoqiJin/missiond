/**
 * Timeline Zustand Store — centralized state management.
 * Enables atomic subscriptions so individual EventDots/Capsules only re-render when their own status changes.
 */
import { create } from 'zustand';
import type { TimelineEvent, SelectionState, TimelineStats, ViewMode } from '../types';
import { EMPTY_SELECTION } from '../types';
import {
  isLaneVisible, getEventLaneId, loadViewState, saveViewState,
} from '../helpers';
import {
  computeSessionLayout, computeSlotLayout, computeLaneGeometry,
  type LayoutResult, type SessionInfo, type SlotInfo, type LaneGeometry,
} from '../core/layoutEngine';

// ── Store Interface ──

interface TimelineState {
  // Data (low-frequency updates)
  events: TimelineEvent[];
  sessionsMeta: Record<string, { startedAt: string }>;
  stats: TimelineStats | null;
  hourlyStats: TimelineStats | null;

  // View/control (medium-frequency)
  soloed: Set<string>;
  muted: Set<string>;
  viewMode: ViewMode;
  dailyDate: string | null;
  activeWindow: string;
  searchQuery: string;
  loading: boolean;

  // Interaction (high-frequency)
  selection: SelectionState;
  hoveredLane: string | null;

  // Derived/cached layout
  sessionLayout: LayoutResult<SessionInfo>;
  slotLayout: LayoutResult<SlotInfo>;
  laneGeometry: LaneGeometry;
  filtered: TimelineEvent[];
  contextSeqSet: Set<number>;

  // Actions
  setEvents: (events: TimelineEvent[], replace?: boolean) => void;
  mergeEvents: (incoming: TimelineEvent[]) => void;
  setSessionsMeta: (meta: Record<string, { startedAt: string }>) => void;
  setStats: (stats: TimelineStats | null) => void;
  setHourlyStats: (stats: TimelineStats | null) => void;
  setSelection: (sel: SelectionState | ((prev: SelectionState) => SelectionState)) => void;
  setHoveredLane: (lane: string | null) => void;
  setSoloed: (updater: (prev: Set<string>) => Set<string>) => void;
  setMuted: (updater: (prev: Set<string>) => Set<string>) => void;
  setViewMode: (mode: ViewMode) => void;
  setDailyDate: (date: string | null) => void;
  setActiveWindow: (w: string) => void;
  setSearchQuery: (q: string) => void;
  setLoading: (l: boolean) => void;
  updateEventSummary: (seq: number, summary: string) => void;
}

// ── Helpers ──

const EMPTY_SESSION_LAYOUT: LayoutResult<SessionInfo> = { map: new Map(), rowCount: 1 };
const EMPTY_SLOT_LAYOUT: LayoutResult<SlotInfo> = { map: new Map(), rowCount: 1 };
const INITIAL_GEOMETRY = computeLaneGeometry(1, 1);

function recomputeLayouts(events: TimelineEvent[], sessionsMeta: Record<string, { startedAt: string }>) {
  const sessionLayout = events.length > 0 ? computeSessionLayout(events, sessionsMeta) : EMPTY_SESSION_LAYOUT;
  const slotLayout = events.length > 0 ? computeSlotLayout(events) : EMPTY_SLOT_LAYOUT;
  const laneGeometry = computeLaneGeometry(sessionLayout.rowCount, slotLayout.rowCount);
  return { sessionLayout, slotLayout, laneGeometry };
}

function recomputeFiltered(events: TimelineEvent[], soloed: Set<string>, muted: Set<string>) {
  return events.filter(e => isLaneVisible(getEventLaneId(e), soloed, muted));
}

// ── Store Creation ──

const savedView = loadViewState();

export const useTimelineStore = create<TimelineState>((set, get) => ({
  // Data
  events: [],
  sessionsMeta: {},
  stats: null,
  hourlyStats: null,

  // View/control
  soloed: new Set(savedView.soloed),
  muted: new Set(savedView.muted),
  viewMode: savedView.dailyDate ? 'daily' : 'relative',
  dailyDate: savedView.dailyDate,
  activeWindow: savedView.activeWindow,
  searchQuery: '',
  loading: false,

  // Interaction
  selection: EMPTY_SELECTION,
  hoveredLane: null,

  // Derived
  sessionLayout: EMPTY_SESSION_LAYOUT,
  slotLayout: EMPTY_SLOT_LAYOUT,
  laneGeometry: INITIAL_GEOMETRY,
  filtered: [],
  contextSeqSet: new Set(),

  // Actions
  setEvents: (events) => {
    const { sessionsMeta, soloed, muted } = get();
    const layouts = recomputeLayouts(events, sessionsMeta);
    const filtered = recomputeFiltered(events, soloed, muted);
    set({ events, ...layouts, filtered });
  },

  mergeEvents: (incoming) => {
    const { events: prev, sessionsMeta, soloed, muted } = get();
    const map = new Map<number, TimelineEvent>();
    for (const ev of prev) map.set(ev.seq, ev);
    let added = 0;
    for (const ev of incoming) { if (!map.has(ev.seq)) added++; map.set(ev.seq, ev); }
    if (added === 0) return; // No new events
    const events = Array.from(map.values()).sort((a, b) => (b.seq ?? 0) - (a.seq ?? 0));
    const layouts = recomputeLayouts(events, sessionsMeta);
    const filtered = recomputeFiltered(events, soloed, muted);
    set({ events, ...layouts, filtered });
  },

  setSessionsMeta: (meta) => {
    const { events, sessionsMeta: prev, soloed, muted } = get();
    const sessionsMeta = { ...prev, ...meta };
    const layouts = recomputeLayouts(events, sessionsMeta);
    const filtered = recomputeFiltered(events, soloed, muted);
    set({ sessionsMeta, ...layouts, filtered });
  },

  setStats: (stats) => set({ stats }),
  setHourlyStats: (hourlyStats) => set({ hourlyStats }),

  setSelection: (selOrFn) => {
    const prev = get().selection;
    const next = typeof selOrFn === 'function' ? selOrFn(prev) : selOrFn;
    if (next === prev) return;
    set({ selection: next, contextSeqSet: new Set(next.contextSeqs) });
  },

  setHoveredLane: (hoveredLane) => set({ hoveredLane }),

  setSoloed: (updater) => {
    const { soloed: prev, events, muted } = get();
    const next = updater(prev);
    const filtered = recomputeFiltered(events, next, muted);
    set({ soloed: next, filtered });
    saveViewState({ activeWindow: get().activeWindow, dailyDate: get().dailyDate, soloed: Array.from(next), muted: Array.from(muted) });
  },

  setMuted: (updater) => {
    const { muted: prev, events, soloed } = get();
    const next = updater(prev);
    const filtered = recomputeFiltered(events, soloed, next);
    set({ muted: next, filtered });
    saveViewState({ activeWindow: get().activeWindow, dailyDate: get().dailyDate, soloed: Array.from(soloed), muted: Array.from(next) });
  },

  setViewMode: (viewMode) => set({ viewMode }),
  setDailyDate: (dailyDate) => {
    set({ dailyDate });
    const s = get();
    saveViewState({ activeWindow: s.activeWindow, dailyDate, soloed: Array.from(s.soloed), muted: Array.from(s.muted) });
  },
  setActiveWindow: (activeWindow) => {
    set({ activeWindow });
    const s = get();
    saveViewState({ activeWindow, dailyDate: s.dailyDate, soloed: Array.from(s.soloed), muted: Array.from(s.muted) });
  },
  setSearchQuery: (searchQuery) => set({ searchQuery }),
  setLoading: (loading) => set({ loading }),

  updateEventSummary: (seq, summary) => {
    const { events: prev, sessionsMeta, soloed, muted } = get();
    const events = prev.map(ev => ev.seq === seq ? { ...ev, summary } : ev);
    const layouts = recomputeLayouts(events, sessionsMeta);
    const filtered = recomputeFiltered(events, soloed, muted);
    set({ events, ...layouts, filtered });
  },
}));
