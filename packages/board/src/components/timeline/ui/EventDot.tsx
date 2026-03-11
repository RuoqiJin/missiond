'use client';

import React, { useCallback } from 'react';
import { cn } from '@/lib/utils';
import { utcMs, formatBeijingTime } from '@/lib/time';
import type { TimelineEvent } from '../types';
import { SESSION_COLORS, DOT_STYLES, CSS_LEFT } from '../constants';
import { getDotStatus, getEventColor, isChatEvent, eventSummary, hasError } from '../helpers';
import { useTimelineStore } from '../stores/timelineStore';

interface EventDotProps {
  ev: TimelineEvent;
  y: number;
  sessionColorIdx: number | null;
  onSelect: (ev: TimelineEvent) => void;
  onRegisterRef?: (seq: number, el: HTMLButtonElement | null) => void;
}

export const EventDot = React.memo(function EventDot({ ev, y, sessionColorIdx, onSelect, onRegisterRef }: EventDotProps) {
  // Atomic subscription: only re-renders when THIS dot's visual status changes
  const dStatus = useTimelineStore(
    useCallback((s) => getDotStatus(ev, s.selection, s.contextSeqSet), [ev]),
  );

  const ec = getEventColor(ev.event_type);
  const isInsight = ev.event_type === 'insight_generated';
  const isError = hasError(ev);
  const sessionColor = sessionColorIdx != null ? SESSION_COLORS[sessionColorIdx] : null;

  return (
    <button
      ref={onRegisterRef ? (el) => onRegisterRef(ev.seq, el) : undefined}
      onClick={(e) => { e.stopPropagation(); onSelect(ev); }}
      className={cn(
        'absolute -translate-x-1/2 -translate-y-1/2 w-6 h-6 flex items-center justify-center',
        'group/dot cursor-pointer z-[25]',
        dStatus === 'focused' && 'z-[35]',
        dStatus === 'highlighted' && 'z-30',
      )}
      style={{ '--t-event': utcMs(ev.created_at), left: CSS_LEFT, top: `${y}%` } as React.CSSProperties}
      title={`${ec.label}: ${eventSummary(ev)}${isChatEvent(ev.event_type) && ev.payload?.session_id ? `\nSession: ${ev.payload.session_id.slice(0, 8)}` : ''}\n${formatBeijingTime(ev.created_at)}`}
    >
      {/* Hover ring background */}
      <div className={cn(
        'absolute inset-0 rounded-full pointer-events-none',
        'scale-50 opacity-0 transition-all duration-200 ease-out',
        dStatus === 'normal' && 'group-hover/dot:scale-100 group-hover/dot:opacity-100',
        ec.bg,
      )} />
      {/* Visual dot */}
      <div className={cn(
        'rounded-full relative',
        'transition-[transform,box-shadow,ring-color,opacity] duration-200 ease-out',
        'animate-spring-pop',
        isInsight ? 'w-3.5 h-3.5' : isError ? 'w-3 h-3' : 'w-[7px] h-[7px]',
        dStatus === 'normal' && !sessionColor && 'ring-2 ring-current/20',
        dStatus === 'normal' && 'group-hover/dot:scale-[1.8] group-hover/dot:shadow-[0_0_10px_var(--tw-shadow-color)]',
        ec.dot, ec.glow,
        sessionColor && dStatus === 'normal' && `ring-[2px] ${sessionColor.ring}`,
        DOT_STYLES[dStatus],
      )} />
    </button>
  );
}, (prev, next) => prev.ev.seq === next.ev.seq && prev.y === next.y && prev.sessionColorIdx === next.sessionColorIdx && prev.onSelect === next.onSelect && prev.onRegisterRef === next.onRegisterRef);
