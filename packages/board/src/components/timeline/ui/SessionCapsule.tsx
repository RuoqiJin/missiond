'use client';

import React, { useCallback } from 'react';
import { cn } from '@/lib/utils';
import { utcMs } from '@/lib/time';
import { SESSION_COLORS, CAPSULE_STYLES } from '../constants';
import { getCapsuleStatus } from '../helpers';
import { useTimelineStore } from '../stores/timelineStore';
import type { SessionInfo } from '../core/layoutEngine';

interface SessionCapsuleProps {
  sid: string;
  info: SessionInfo;
  meta: { startedAt: string } | undefined;
  chatLaneTop: number;
  chatSubRowHeight: number;
  onSelect: (sid: string) => void;
}

export const SessionCapsule = React.memo(function SessionCapsule({
  sid, info, meta, chatLaneTop, chatSubRowHeight, onSelect,
}: SessionCapsuleProps) {
  // Atomic subscription: only re-renders when THIS capsule's status changes
  const cStatus = useTimelineStore(
    useCallback((s) => getCapsuleStatus(sid, s.selection), [sid]),
  );

  const sc = SESSION_COLORS[info.colorIdx];
  const times = info.events.map(e => utcMs(e.created_at));
  const evtMin = Math.min(...times);
  const evtMax = Math.max(...times);
  const actualStart = meta?.startedAt ? Math.min(utcMs(meta.startedAt), evtMin) : evtMin;
  const yCenter = chatLaneTop + chatSubRowHeight * (info.row + 0.5);
  const capsuleH = chatSubRowHeight * 0.7;

  return (
    <div
      className={cn(
        'absolute cursor-pointer overflow-hidden',
        'transition-[opacity,box-shadow,border-color,transform] duration-200 ease-out',
        'animate-capsule-enter',
        cStatus === 'normal' && 'hover:brightness-110 hover:shadow-md hover:shadow-black/30',
        CAPSULE_STYLES[cStatus],
      )}
      style={{
        '--t-start': actualStart, '--t-end': evtMax,
        left: 'calc((var(--t-start) - var(--t-min)) / var(--t-range) * 100%)',
        width: 'calc(max((var(--t-end) - var(--t-start)) / var(--t-range) * 100%, 1.2%))',
        top: `${yCenter - capsuleH / 2}%`, height: `${capsuleH}%`,
        backgroundColor: sc.line,
        border: cStatus === 'selected' ? `2px solid ${sc.line.replace('0.25)', '0.85)')}` : `1px solid ${sc.line.replace('0.25)', '0.45)')}`,
        borderRadius: '10px',
        boxShadow: cStatus === 'selected'
          ? `0 0 14px ${sc.line.replace('0.25)', '0.35)')}, inset 0 1px 0 rgba(255,255,255,0.08)`
          : 'inset 0 1px 0 rgba(255,255,255,0.06)',
      } as React.CSSProperties}
      onClick={(e) => { e.stopPropagation(); onSelect(sid); }}
      title={`Session: ${sid.slice(0, 8)} · ${info.events.length} events`}
    >
      <div className="absolute inset-0 bg-gradient-to-b from-white/[0.05] to-transparent pointer-events-none rounded-[10px]" />
    </div>
  );
});
