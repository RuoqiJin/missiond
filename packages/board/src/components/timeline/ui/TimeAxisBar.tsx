'use client';

import React from 'react';
import { cn } from '@/lib/utils';

// ── Time Axis ──

interface TimeAxisProps {
  ticks: Array<{ pos: number; label: string }>;
}

export const TimeAxis = React.memo(function TimeAxis({ ticks }: TimeAxisProps) {
  return (
    <div className="h-5 border-t border-neutral-800 flex items-center relative ml-16">
      {ticks.map((tick, i) => (
        <span key={i} className="absolute text-[9px] text-neutral-600 -translate-x-1/2" style={{ left: `${tick.pos}%` }}>
          {tick.label}
        </span>
      ))}
    </div>
  );
});

// ── Minimap Histogram ──

interface MinimapProps {
  histogram: Array<{ height: number; errors: number; idx: number }>;
}

export const Minimap = React.memo(function Minimap({ histogram }: MinimapProps) {
  return (
    <div className="h-6 border-t border-neutral-800 flex items-end px-0 ml-16">
      {histogram.map((bar, i) => (
        <div key={i} className="flex-1" style={{ height: `${Math.max(bar.height, 2)}%` }}>
          <div className={cn('w-full h-full', bar.errors > 0 ? 'bg-red-500/40' : 'bg-neutral-700/40')} />
        </div>
      ))}
    </div>
  );
});
