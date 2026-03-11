/**
 * Layout Engine — pure computation functions for timeline lane geometry.
 * No React dependencies. Extracted from CognitiveTimeline for testability and reuse.
 */
import { utcMs } from '@/lib/time';
import type { TimelineEvent } from '../types';
import { SWIMLANES, SLOT_LANE_IDX } from '../constants';
import { isChatEvent, hashSessionColor } from '../helpers';

// ── Types ──

export interface SessionInfo {
  colorIdx: number;
  row: number;
  startedBefore: boolean;
  events: TimelineEvent[];
}

export interface SlotInfo {
  row: number;
  events: TimelineEvent[];
}

export interface LayoutResult<T> {
  map: Map<string, T>;
  rowCount: number;
}

export interface LaneGeo {
  top: number;
  height: number;
}

export interface LaneGeometry {
  lanes: LaneGeo[];
  chatSubRowHeight: number;
  slotSubRowHeight: number;
}

// ── Session Layout (Chat lane sub-row packing) ──

export function computeSessionLayout(
  events: TimelineEvent[],
  sessionsMeta: Record<string, { startedAt: string }>,
): LayoutResult<SessionInfo> {
  // Group chat events by session_id (exclude slot events)
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
  const sessions: { id: string; parentId?: string; start: number; end: number; startedBefore: boolean; events: TimelineEvent[] }[] = [];
  for (const [sid, evts] of bySession) {
    const times = evts.map(e => utcMs(e.created_at));
    const evtMin = Math.min(...times);
    const parentId = evts.find(e => e.payload?.parent_session_id)?.payload?.parent_session_id;
    const meta = sessionsMeta[sid];
    const actualStart = meta?.startedAt ? Math.min(utcMs(meta.startedAt), evtMin) : evtMin;
    const startedBefore = actualStart < evtMin;
    sessions.push({ id: sid, parentId, start: actualStart, end: Math.max(...times), startedBefore, events: evts });
  }
  sessions.sort((a, b) => a.start - b.start);

  // Greedy row assignment — pack sessions into fewest non-overlapping rows
  const rowEnds: number[] = [];
  const layout = new Map<string, SessionInfo>();
  for (const s of sessions) {
    let assigned = -1;
    for (let r = 0; r < rowEnds.length; r++) {
      if (s.start > rowEnds[r]) { assigned = r; break; }
    }
    if (assigned === -1) { assigned = rowEnds.length; rowEnds.push(0); }
    rowEnds[assigned] = s.end;
    layout.set(s.id, {
      colorIdx: hashSessionColor(s.parentId || s.id),
      row: assigned,
      startedBefore: s.startedBefore,
      events: s.events,
    });
  }

  return { map: layout, rowCount: Math.max(rowEnds.length, 1) };
}

// ── Slot Layout (Slot lane sub-row assignment) ──

export function computeSlotLayout(events: TimelineEvent[]): LayoutResult<SlotInfo> {
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
  const layout = new Map<string, SlotInfo>();
  slotIds.forEach((sid, index) => {
    layout.set(sid, { row: index, events: bySlot.get(sid)! });
  });
  return { map: layout, rowCount: Math.max(slotIds.length, 1) };
}

// ── Lane Geometry (proportional height allocation) ──

export function computeLaneGeometry(
  sessionRowCount: number,
  slotRowCount: number,
): LaneGeometry {
  const chatWeight = sessionRowCount;
  const slotWeight = slotRowCount;
  const totalWeight = chatWeight + slotWeight + (SWIMLANES.length - 2);
  const lanes: LaneGeo[] = [];
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
    chatSubRowHeight: chatLane.height / sessionRowCount,
    slotSubRowHeight: slotLane.height / slotRowCount,
  };
}
