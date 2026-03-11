'use client';

import React from 'react';
import { utcMs } from '@/lib/time';
import type { TimelineEvent } from '../types';

interface SpanLinesProps {
  filtered: TimelineEvent[];
  getX: (dateStr: string) => number;
  getY: (ev: TimelineEvent) => number;
  selectedSpanId: string | null;
}

const SPAN_TYPES = new Set([
  'cli_request_started', 'cli_request_completed',
  'gemini_request_started', 'gemini_request_completed',
  'codex_request_started', 'codex_request_completed',
]);

export const SpanLines = React.memo(function SpanLines({ filtered, getX, getY, selectedSpanId }: SpanLinesProps) {
  const spanPairs = new Map<string, TimelineEvent[]>();
  for (const ev of filtered) {
    if (ev.span_id && SPAN_TYPES.has(ev.event_type)) {
      const arr = spanPairs.get(ev.span_id) || [];
      arr.push(ev);
      spanPairs.set(ev.span_id, arr);
    }
  }

  return (
    <>
      {Array.from(spanPairs.values())
        .filter(pair => pair.length === 2)
        .map(pair => {
          const [a, b] = pair.sort((x, y) => utcMs(x.created_at) - utcMs(y.created_at));
          const x1 = getX(a.created_at);
          const x2 = getX(b.created_at);
          const lineY = getY(a);
          const isActive = selectedSpanId === a.span_id;
          const isCodex = a.event_type.startsWith('codex_');
          const color = isCodex
            ? (isActive ? 'rgba(56,189,248,0.5)' : 'rgba(56,189,248,0.15)')
            : (isActive ? 'rgba(168,85,247,0.5)' : 'rgba(168,85,247,0.15)');
          return (
            <svg key={`span-${a.span_id}`} className="absolute inset-0 w-full h-full pointer-events-none z-0" preserveAspectRatio="none">
              <line x1={`${x1}%`} y1={`${lineY}%`} x2={`${x2}%`} y2={`${lineY}%`} stroke={color} strokeWidth={isActive ? '2' : '1'} strokeDasharray="3 2" />
            </svg>
          );
        })}
    </>
  );
});
