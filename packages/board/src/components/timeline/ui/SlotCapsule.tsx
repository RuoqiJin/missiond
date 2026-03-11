'use client';

import React, { useCallback } from 'react';
import { cn } from '@/lib/utils';
import { utcMs } from '@/lib/time';
import { CAPSULE_STYLES, getSlotLine } from '../constants';
import { getCapsuleStatus } from '../helpers';
import { useTimelineStore } from '../stores/timelineStore';
import type { SlotInfo } from '../core/layoutEngine';

interface SlotCapsuleProps {
  slotId: string;
  info: SlotInfo;
  slotLaneTop: number;
  slotSubRowHeight: number;
  onSelect: (slotId: string) => void;
}

export const SlotCapsule = React.memo(function SlotCapsule({
  slotId, info, slotLaneTop, slotSubRowHeight, onSelect,
}: SlotCapsuleProps) {
  const cStatus = useTimelineStore(
    useCallback((s) => getCapsuleStatus(slotId, s.selection), [slotId]),
  );

  const times = info.events.map(e => utcMs(e.created_at));
  const evtMin = Math.min(...times);
  const evtMax = Math.max(...times);
  const lineColor = getSlotLine(slotId);
  const yCenter = slotLaneTop + slotSubRowHeight * (info.row + 0.5);
  const capsuleH = slotSubRowHeight * 0.7;

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
        '--t-start': evtMin, '--t-end': evtMax,
        left: 'calc((var(--t-start) - var(--t-min)) / var(--t-range) * 100%)',
        width: 'calc(max((var(--t-end) - var(--t-start)) / var(--t-range) * 100%, 1.2%))',
        top: `${yCenter - capsuleH / 2}%`, height: `${capsuleH}%`,
        backgroundColor: lineColor,
        border: cStatus === 'selected' ? `2px solid ${lineColor.replace('0.25)', '0.85)')}` : `1px solid ${lineColor.replace('0.25)', '0.45)')}`,
        borderRadius: '10px',
        boxShadow: cStatus === 'selected'
          ? `0 0 14px ${lineColor.replace('0.25)', '0.35)')}, inset 0 1px 0 rgba(255,255,255,0.08)`
          : 'inset 0 1px 0 rgba(255,255,255,0.06)',
      } as React.CSSProperties}
      onClick={(e) => { e.stopPropagation(); onSelect(slotId); }}
      title={`Slot: ${slotId} · ${info.events.length} events`}
    >
      <div className="absolute inset-0 flex items-center px-1.5 pointer-events-none">
        <span className="text-[8px] font-medium text-white/40 truncate">{slotId.replace('slot-', '')}</span>
      </div>
      <div className="absolute inset-0 bg-gradient-to-b from-white/[0.05] to-transparent pointer-events-none rounded-[10px]" />
    </div>
  );
});
