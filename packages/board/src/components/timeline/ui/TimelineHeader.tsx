'use client';

import { useCallback, useMemo } from 'react';
import {
  Search, RefreshCw, ChevronLeft, ChevronRight, Calendar,
} from 'lucide-react';
import { cn } from '@/lib/utils';
import { todayBeijing, toBeijingDate } from '@/lib/time';
import { WINDOW_OPTIONS } from '../constants';
import { formatDailyLabel, shiftDay } from '../helpers';
import { StatCard } from '../EventSummaryView';
import { useTimelineStore } from '../stores/timelineStore';

interface TimelineHeaderProps {
  searchInput: string;
  setSearchInput: (v: string) => void;
  onRefresh: () => void;
  onWindowChange: (w: string) => void;
  onDailyNav: (date: string) => void;
}

export function TimelineHeader({ searchInput, setSearchInput, onRefresh, onWindowChange, onDailyNav }: TimelineHeaderProps) {
  const stats = useTimelineStore(s => s.stats);
  const hourlyStats = useTimelineStore(s => s.hourlyStats);
  const loading = useTimelineStore(s => s.loading);
  const activeWindow = useTimelineStore(s => s.activeWindow);
  const viewMode = useTimelineStore(s => s.viewMode);
  const dailyDate = useTimelineStore(s => s.dailyDate);
  const soloed = useTimelineStore(s => s.soloed);
  const muted = useTimelineStore(s => s.muted);
  const setSoloed = useTimelineStore(s => s.setSoloed);
  const setMuted = useTimelineStore(s => s.setMuted);
  const setSearchQuery = useTimelineStore(s => s.setSearchQuery);

  const earliestDate = useMemo(() => toBeijingDate(Date.now() - 7 * 86400_000), []);

  const handleDailyPrev = useCallback(() => {
    const current = dailyDate || todayBeijing();
    const prev = shiftDay(current, -1);
    if (prev >= earliestDate) onDailyNav(prev);
  }, [dailyDate, earliestDate, onDailyNav]);

  const handleDailyNext = useCallback(() => {
    const current = dailyDate || todayBeijing();
    const next = shiftDay(current, 1);
    if (next <= todayBeijing()) onDailyNav(next);
  }, [dailyDate, onDailyNav]);

  return (
    <div className="flex items-center justify-between gap-4 flex-wrap">
      {/* Stats cards */}
      <div className="flex items-center gap-3">
        <StatCard label="Events" value={stats?.total_events ?? '-'} />
        <StatCard label="Traces" value={stats?.unique_traces ?? '-'} />
        <StatCard
          label="Gemini P90"
          value={stats?.gemini_latency ? `${stats.gemini_latency.p90_ms}ms` : '-'}
          color={stats?.gemini_latency && stats.gemini_latency.p90_ms > 10000 ? 'text-red-400' : undefined}
        />
        <StatCard
          label="Insights"
          value={stats?.by_type?.find(t => t[0] === 'insight_generated')?.[1] ?? 0}
          color="text-emerald-400"
        />
        <div className="w-px h-6 bg-neutral-800" />
        <StatCard
          label="CLI/h"
          value={(hourlyStats?.by_type?.find(t => t[0] === 'cli_request_completed')?.[1] ?? 0) + (hourlyStats?.by_type?.find(t => t[0] === 'codex_request_completed')?.[1] ?? 0) + (hourlyStats?.by_type?.find(t => t[0] === 'gemini_request_completed')?.[1] ?? 0)}
          color="text-purple-400"
        />
      </div>

      {/* Controls */}
      <div className="flex items-center gap-2">
        {/* Search */}
        <div className="relative">
          <Search className="absolute left-2 top-1/2 -translate-y-1/2 w-3 h-3 text-neutral-500" />
          <input
            type="text"
            placeholder="Search..."
            value={searchInput}
            onChange={(e) => setSearchInput(e.target.value)}
            onKeyDown={(e) => { if (e.key === 'Enter') setSearchQuery(searchInput); }}
            className="pl-7 pr-3 py-1.5 text-xs bg-neutral-900 border border-neutral-800 rounded-md text-neutral-300 placeholder-neutral-600 w-44 focus:outline-none focus:border-neutral-600"
          />
        </div>

        {/* Reset lane visibility */}
        {(soloed.size > 0 || muted.size > 0) && (
          <button
            onClick={() => { setSoloed(() => new Set()); setMuted(() => new Set()); }}
            className="px-2 py-1 text-[10px] font-medium rounded bg-neutral-900 text-neutral-400 hover:text-white transition-colors"
            title="Reset all Solo/Mute"
          >
            All
          </button>
        )}

        {/* Window presets */}
        <div className="flex items-center gap-0.5 bg-neutral-900 rounded-md p-0.5">
          {WINDOW_OPTIONS.map(w => (
            <button
              key={w.value}
              onClick={() => onWindowChange(w.value)}
              className={cn(
                'px-2 py-1 text-[10px] font-medium rounded transition-colors',
                activeWindow === w.value ? 'bg-neutral-800 text-white' : 'text-neutral-500 hover:text-neutral-300',
              )}
            >
              {w.label}
            </button>
          ))}
        </div>

        {/* Daily navigator */}
        <div className="w-px h-5 bg-neutral-800" />
        <div className="flex items-center gap-0.5">
          <button
            onClick={handleDailyPrev}
            disabled={dailyDate ? dailyDate <= earliestDate : false}
            className="p-1 rounded hover:bg-neutral-800 text-neutral-500 hover:text-neutral-300 disabled:opacity-30 disabled:cursor-not-allowed transition-colors"
            title="前一天"
          >
            <ChevronLeft className="w-3.5 h-3.5" />
          </button>

          <div className="relative">
            <span className={cn(
              'px-2 py-1 text-[10px] font-medium rounded cursor-pointer transition-colors flex items-center gap-1',
              viewMode === 'daily' ? 'bg-neutral-800 text-white' : 'text-neutral-500 hover:text-neutral-300',
            )}>
              <Calendar className="w-3 h-3" />
              {dailyDate ? formatDailyLabel(dailyDate) : '日历'}
            </span>
            <input
              type="date"
              className="absolute inset-0 opacity-0 cursor-pointer"
              value={dailyDate || todayBeijing()}
              onChange={(e) => { if (e.target.value) onDailyNav(e.target.value); }}
              max={todayBeijing()}
              min={earliestDate}
            />
          </div>

          <button
            onClick={handleDailyNext}
            disabled={!dailyDate || dailyDate >= todayBeijing()}
            className="p-1 rounded hover:bg-neutral-800 text-neutral-500 hover:text-neutral-300 disabled:opacity-30 disabled:cursor-not-allowed transition-colors"
            title="后一天"
          >
            <ChevronRight className="w-3.5 h-3.5" />
          </button>
        </div>

        <button onClick={onRefresh} className="p-1.5 rounded hover:bg-neutral-800 text-neutral-500 hover:text-neutral-300 transition-colors">
          <RefreshCw className={cn('w-3.5 h-3.5', loading && 'animate-spin')} />
        </button>
      </div>
    </div>
  );
}
