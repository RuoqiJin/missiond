'use client';

import React, { useMemo } from 'react';
import { utcMs } from '@/lib/time';
import type { TimelineEvent, SelectionState } from '../types';
import { getEventLaneId } from '../helpers';

interface CrossLaneLinksProps {
  filtered: TimelineEvent[];
  selection: SelectionState;
  getX: (dateStr: string) => number;
  getY: (ev: TimelineEvent) => number;
}

interface CausalLink {
  source: TimelineEvent; // parent (e.g. assistant_message)
  target: TimelineEvent; // child (e.g. cli_request_started)
}

// ── DAG-based linking via parent_span_id (precise) ──

function findDagLinks(events: TimelineEvent[]): CausalLink[] {
  const bySpan = new Map<string, TimelineEvent>();
  for (const ev of events) {
    if (ev.span_id) bySpan.set(ev.span_id, ev);
  }

  const links: CausalLink[] = [];
  const seen = new Set<string>(); // dedup by "source.seq-target.seq"

  for (const ev of events) {
    if (!ev.parent_span_id) continue;
    const parent = bySpan.get(ev.parent_span_id);
    if (!parent) continue;
    // Only cross-lane links (same-lane pairs are handled by SpanLines)
    if (getEventLaneId(parent) === getEventLaneId(ev)) continue;
    const key = `${parent.seq}-${ev.seq}`;
    if (seen.has(key)) continue;
    seen.add(key);
    links.push({ source: parent, target: ev });
  }

  return links;
}

// ── Heuristic fallback for events without parent_span_id ──

const TOOL_CALLER_MAP: Record<string, string> = {
  'mission_router_chat': 'router_chat',
  'mission_kb_analyze': 'kb_analyze',
};
const MAX_LINK_GAP_MS = 5 * 60 * 1000;

function extractCallerFromPreview(preview: string): string | null {
  const match = preview.match(/^\[(.+)\]$/);
  if (!match) return null;
  for (const tool of match[1].split(', ')) {
    const parts = tool.trim().split('__');
    const toolName = parts.length >= 3 ? parts.slice(2).join('__') : parts[parts.length - 1];
    const caller = TOOL_CALLER_MAP[toolName];
    if (caller) return caller;
  }
  return null;
}

function findHeuristicLinks(events: TimelineEvent[], dagMatched: Set<number>): CausalLink[] {
  const links: CausalLink[] = [];
  const llmStarted = events.filter(e =>
    (e.event_type === 'cli_request_started' || e.event_type === 'gemini_request_started')
    && !dagMatched.has(e.seq) // Skip events already matched by DAG
  );
  const toolCalls = events.filter(e =>
    e.event_type === 'assistant_message' && e.payload?.preview && !dagMatched.has(e.seq)
  );
  const matched = new Set<number>();

  for (const chatEv of toolCalls) {
    const caller = extractCallerFromPreview(chatEv.payload.preview);
    if (!caller) continue;
    const chatTime = utcMs(chatEv.created_at);
    const traceId = chatEv.trace_id;
    let bestMatch: TimelineEvent | null = null;
    let bestDelta = Infinity;

    for (const llmEv of llmStarted) {
      if (matched.has(llmEv.seq)) continue;
      if (llmEv.payload?.caller !== caller) continue;
      if (traceId && llmEv.trace_id !== traceId) continue;
      const delta = utcMs(llmEv.created_at) - chatTime;
      if (delta >= -2000 && delta < MAX_LINK_GAP_MS && Math.abs(delta) < bestDelta) {
        bestMatch = llmEv;
        bestDelta = Math.abs(delta);
      }
    }
    if (bestMatch) {
      matched.add(bestMatch.seq);
      links.push({ source: chatEv, target: bestMatch });
    }
  }
  return links;
}

// ── Combined: DAG first, heuristic fallback for historical events ──

function findCausalLinks(events: TimelineEvent[]): CausalLink[] {
  const dagLinks = findDagLinks(events);
  // Collect all seqs already matched by DAG to avoid double-linking
  const dagMatched = new Set<number>();
  for (const { source, target } of dagLinks) {
    dagMatched.add(source.seq);
    dagMatched.add(target.seq);
  }
  const heuristicLinks = findHeuristicLinks(events, dagMatched);
  return [...dagLinks, ...heuristicLinks];
}

export const CrossLaneLinks = React.memo(function CrossLaneLinks({
  filtered,
  selection,
  getX,
  getY,
}: CrossLaneLinksProps) {
  const allLinks = useMemo(() => findCausalLinks(filtered), [filtered]);

  const visibleLinks = useMemo(() => {
    if (allLinks.length === 0) return [];
    const contextSet = new Set(selection.contextSeqs);
    return allLinks.filter(({ source, target }) => {
      if (selection.focusedSeq === source.seq || selection.focusedSeq === target.seq) return true;
      if (selection.scope === 'trace' && (contextSet.has(source.seq) || contextSet.has(target.seq))) return true;
      if (selection.scope === 'session' && source.payload?.session_id === selection.scopeId) return true;
      return false;
    });
  }, [allLinks, selection]);

  if (visibleLinks.length === 0) return null;

  return (
    <>
      {visibleLinks.map(({ source, target }) => {
        const x1 = getX(source.created_at);
        const y1 = getY(source);
        const x2 = getX(target.created_at);
        const y2 = getY(target);
        const isFocused = selection.focusedSeq === source.seq || selection.focusedSeq === target.seq;

        // Two-segment path through a midpoint that arches between lanes
        const midX = (x1 + x2) / 2;
        const midY = Math.min(y1, y2) + Math.abs(y2 - y1) * 0.15;

        const color = isFocused
          ? 'rgba(45,212,191,0.55)'
          : 'rgba(45,212,191,0.2)';

        return (
          <svg
            key={`xlink-${source.seq}-${target.seq}`}
            className="absolute inset-0 w-full h-full pointer-events-none z-[2]"
            preserveAspectRatio="none"
          >
            <line
              x1={`${x1}%`} y1={`${y1}%`}
              x2={`${midX}%`} y2={`${midY}%`}
              stroke={color}
              strokeWidth={isFocused ? '1.5' : '1'}
              strokeDasharray={isFocused ? '6 3' : '3 2'}
            />
            <line
              x1={`${midX}%`} y1={`${midY}%`}
              x2={`${x2}%`} y2={`${y2}%`}
              stroke={isFocused ? 'rgba(168,85,247,0.55)' : 'rgba(168,85,247,0.2)'}
              strokeWidth={isFocused ? '1.5' : '1'}
              strokeDasharray={isFocused ? '6 3' : '3 2'}
            />
          </svg>
        );
      })}
    </>
  );
});
