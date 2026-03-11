'use client';

import React from 'react';
import { utcMs } from '@/lib/time';
import type { TimelineEvent, SelectionScope } from '../types';

interface TraceLinesProps {
  contextEvents: TimelineEvent[];
  scope: SelectionScope;
  getX: (dateStr: string) => number;
  getY: (ev: TimelineEvent) => number;
}

export const TraceLines = React.memo(function TraceLines({ contextEvents, scope, getX, getY }: TraceLinesProps) {
  if (scope !== 'trace' || contextEvents.length <= 1) return null;

  const sorted = [...contextEvents].sort((a, b) => (utcMs(a.created_at) - utcMs(b.created_at)) || (a.seq - b.seq));

  return (
    <>
      {sorted.slice(0, -1).map((ev, i) => {
        const next = sorted[i + 1];
        return (
          <svg key={`line-${i}`} className="absolute inset-0 w-full h-full pointer-events-none z-0" preserveAspectRatio="none">
            <line
              x1={`${getX(ev.created_at)}%`} y1={`${getY(ev)}%`}
              x2={`${getX(next.created_at)}%`} y2={`${getY(next)}%`}
              stroke="rgba(255,255,255,0.15)" strokeWidth="1" strokeDasharray="4 2"
            />
          </svg>
        );
      })}
    </>
  );
});
