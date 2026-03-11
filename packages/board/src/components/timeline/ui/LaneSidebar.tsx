'use client';

import React from 'react';
import { cn } from '@/lib/utils';
import { SWIMLANES } from '../constants';
import { isLaneVisible } from '../helpers';
import { useTimelineStore } from '../stores/timelineStore';

export const LaneSidebar = React.memo(function LaneSidebar() {
  const laneGeometry = useTimelineStore(s => s.laneGeometry);
  const soloed = useTimelineStore(s => s.soloed);
  const muted = useTimelineStore(s => s.muted);
  const hoveredLane = useTimelineStore(s => s.hoveredLane);
  const setHoveredLane = useTimelineStore(s => s.setHoveredLane);
  const setSoloed = useTimelineStore(s => s.setSoloed);
  const setMuted = useTimelineStore(s => s.setMuted);

  const handleSolo = (id: string, shift: boolean) => {
    setSoloed(prev => {
      const next = new Set(prev);
      if (shift) {
        if (next.has(id)) next.delete(id); else next.add(id);
      } else {
        if (next.has(id) && next.size === 1) next.clear();
        else { next.clear(); next.add(id); }
      }
      return next;
    });
    // Unmute if soloing
    setMuted(m => { const nm = new Set(m); nm.delete(id); return nm; });
  };

  const handleMute = (id: string) => {
    setMuted(prev => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else {
        next.add(id);
        setSoloed(s => { const ns = new Set(s); ns.delete(id); return ns; });
      }
      return next;
    });
  };

  return (
    <div className="w-20 shrink-0 border-r border-neutral-800/80 bg-neutral-950 flex flex-col">
      {SWIMLANES.map((lane, i) => {
        const geo = laneGeometry.lanes[i];
        const isSoloed = soloed.has(lane.id);
        const isMuted = muted.has(lane.id);
        const isVisible = isLaneVisible(lane.id, soloed, muted);
        const isHovered = hoveredLane === lane.id;
        return (
          <div key={lane.id} className={cn(
            'group relative flex items-center gap-1 px-1.5 text-[10px] font-medium overflow-hidden',
            'transition-colors duration-150',
            i < SWIMLANES.length - 1 && 'border-b border-neutral-800/50',
            !isVisible && 'opacity-40',
          )} style={{
            height: `${geo.height}%`,
            backgroundColor: isHovered && isVisible ? `${lane.accent.css}10` : undefined,
          }}
            onMouseEnter={() => setHoveredLane(lane.id)}
            onMouseLeave={() => setHoveredLane(null)}
          >
            {/* Accent dot */}
            <div
              className={cn('w-1.5 h-1.5 rounded-full shrink-0', lane.accent.dot)}
              style={{
                boxShadow: isVisible ? `0 0 ${isHovered ? '8' : '6'}px ${lane.accent.css}` : 'none',
                opacity: isVisible ? (isHovered ? 1 : 0.85) : 0.3,
              }}
            />
            {/* Label */}
            <span className={cn(
              'truncate flex-1',
              isSoloed ? 'text-amber-400'
                : isMuted ? 'text-neutral-600 line-through'
                : isHovered ? 'text-neutral-200' : 'text-neutral-400',
            )} title={lane.label}>{lane.label}</span>
            {/* Solo/Mute */}
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
  );
});
