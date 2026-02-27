'use client';

import { useState, useEffect, useCallback } from 'react';
import { Activity, Pause, Play, RefreshCw, Zap, Clock, Database, AlertCircle, AlertTriangle, ChevronDown, ChevronRight, TrendingUp } from 'lucide-react';
import { cn } from '@/lib/utils';
import { Badge } from '@/components/ui/badge';

interface LaneStatus {
  slotId: string;
  phase: string;
  activeType: string | null;
  phaseAge: number;
  busySince: number;
  busyDuration: number;
  currentTargets?: string[];
  currentConvId?: string | null;
  currentTaskId?: string | null;
}

interface RecentTask {
  id: string;
  slotId: string;
  taskType: string;
  status: string;
  durationMs: number | null;
  createdAt: string;
  error: string | null;
  outputCount: number;
  sourceSessions: string | null;
  conversationId: string | null;
}

interface RealtimeDetail {
  sessionId: string;
  msgCount: number;
  oldest: string;
}

interface DeepDetail {
  conversationId: string;
  endedAt: string;
  retries: number;
}

interface KBStats {
  total: number;
  categories: Record<string, number> | null;
  subcategories: Record<string, number> | null;
  neverAccessed: number;
  mostAccessed: { category: string; key: string; accessCount: number } | null;
  oldest: { category: string; key: string; updatedAt: string } | null;
}

interface MemoryStatus {
  paused: boolean;
  fastLane: LaneStatus;
  slowLane: LaneStatus;
  pendingRealtime: number;
  pendingDeep: number;
  realtimeDetail: RealtimeDetail[];
  deepDetail: DeepDetail[];
  lastKbConsolidation: string;
  lastAutoGc: string;
  kbStats: KBStats | null;
  recentTasks: RecentTask[];
}

function formatAge(seconds: number): string {
  if (seconds < 60) return `${seconds}s`;
  if (seconds < 3600) return `${Math.floor(seconds / 60)}m ${seconds % 60}s`;
  return `${Math.floor(seconds / 3600)}h ${Math.floor((seconds % 3600) / 60)}m`;
}

function formatDuration(ms: number | null): string {
  if (ms == null) return '-';
  if (ms < 1000) return `${ms}ms`;
  if (ms < 60000) return `${(ms / 1000).toFixed(1)}s`;
  return `${(ms / 60000).toFixed(1)}m`;
}

function timeAgo(dateStr: string): string {
  if (!dateStr) return '-';
  const diff = Date.now() - new Date(dateStr).getTime();
  const mins = Math.floor(diff / 60000);
  if (mins < 1) return '刚刚';
  if (mins < 60) return `${mins}分前`;
  const hours = Math.floor(mins / 60);
  if (hours < 24) return `${hours}时前`;
  const days = Math.floor(hours / 24);
  return `${days}天前`;
}

function shortSessionId(id: string): string {
  return id.length > 12 ? id.slice(0, 8) + '...' : id;
}

const PHASE_STYLES: Record<string, { label: string; color: string; bg: string }> = {
  Idle: { label: 'Idle', color: 'text-neutral-400', bg: 'bg-neutral-500/10' },
  Sending: { label: 'Sending', color: 'text-yellow-400', bg: 'bg-yellow-500/10' },
  WaitingForSlotIdle: { label: 'Running', color: 'text-orange-400', bg: 'bg-orange-500/10' },
};

const STATUS_STYLES: Record<string, { color: string; bg: string }> = {
  completed: { color: 'text-green-400', bg: 'bg-green-500/10' },
  running: { color: 'text-orange-400', bg: 'bg-orange-500/10' },
  failed: { color: 'text-red-400', bg: 'bg-red-500/10' },
  pending: { color: 'text-neutral-400', bg: 'bg-neutral-500/10' },
};

const TYPE_LABELS: Record<string, string> = {
  realtime_extract: 'Realtime',
  deep_analysis: 'Deep Analysis',
  kb_consolidation: 'KB Consolidation',
  kb_gc: 'KB GC',
};

const STUCK_THRESHOLD_SECS = 900; // 15 min

function LaneCard({ label, lane, icon }: { label: string; lane: LaneStatus; icon: 'fast' | 'slow' }) {
  const phase = PHASE_STYLES[lane.phase] || PHASE_STYLES.Idle;
  const isActive = lane.phase !== 'Idle';
  const isStuck = isActive && lane.busyDuration > STUCK_THRESHOLD_SECS;

  return (
    <div className={cn(
      'flex-1 rounded-lg border p-3',
      isStuck ? 'border-red-500/40 bg-red-500/5' :
      isActive ? 'border-orange-500/30 bg-orange-500/5' : 'border-neutral-800 bg-neutral-900/50',
    )}>
      <div className="flex items-center justify-between mb-2">
        <span className="text-xs font-medium text-neutral-400 flex items-center gap-1.5">
          {icon === 'fast' ? <Zap className="w-3 h-3 text-yellow-400" /> : <Clock className="w-3 h-3 text-blue-400" />}
          {label}
        </span>
        <div className="flex items-center gap-1">
          {isStuck && <AlertTriangle className="w-3 h-3 text-red-400" />}
          <Badge variant="outline" className={cn(
            'text-[10px] px-1.5 py-0 border-0',
            isStuck ? 'text-red-400 bg-red-500/10' : phase.color, !isStuck && phase.bg,
          )}>
            {isStuck ? 'Stuck' : phase.label}
          </Badge>
        </div>
      </div>
      {isActive && (
        <div className="text-xs text-neutral-500 space-y-0.5">
          <div>类型: <span className="text-neutral-300">{lane.activeType || '-'}</span></div>
          <div>已运行: <span className={cn('text-neutral-300', isStuck && 'text-red-400')}>{formatAge(lane.phaseAge)}</span></div>
          {lane.busyDuration > 0 && (
            <div>总占用: <span className={cn('text-neutral-300', isStuck && 'text-red-400')}>{formatAge(lane.busyDuration)}</span></div>
          )}
          {/* Fast lane: show target sessions */}
          {lane.currentTargets && lane.currentTargets.length > 0 && (
            <div className="mt-1 pt-1 border-t border-neutral-800">
              <span className="text-neutral-500">目标:</span>
              <div className="flex flex-wrap gap-1 mt-0.5">
                {lane.currentTargets.map(sid => (
                  <span key={sid} className="text-[9px] px-1 py-0 rounded bg-neutral-800 text-neutral-400 font-mono" title={sid}>
                    {shortSessionId(sid)}
                  </span>
                ))}
              </div>
            </div>
          )}
          {/* Slow lane: show current conversation */}
          {lane.currentConvId && (
            <div className="mt-1 pt-1 border-t border-neutral-800">
              <span className="text-neutral-500">会话: </span>
              <span className="text-neutral-400 font-mono text-[9px]" title={lane.currentConvId}>
                {shortSessionId(lane.currentConvId)}
              </span>
            </div>
          )}
        </div>
      )}
    </div>
  );
}

function QueueDetailPanel({ realtimeDetail, deepDetail }: { realtimeDetail: RealtimeDetail[]; deepDetail: DeepDetail[] }) {
  const [expanded, setExpanded] = useState(false);
  const hasDetail = realtimeDetail.length > 0 || deepDetail.length > 0;

  if (!hasDetail) return null;

  return (
    <div className="rounded-lg border border-neutral-800 overflow-hidden mb-4">
      <button
        onClick={() => setExpanded(!expanded)}
        className="w-full flex items-center justify-between px-3 py-2 bg-neutral-900/80 text-xs text-neutral-400 hover:text-neutral-300 transition-colors"
      >
        <span className="font-medium">队列明细</span>
        {expanded ? <ChevronDown className="w-3 h-3" /> : <ChevronRight className="w-3 h-3" />}
      </button>
      {expanded && (
        <div className="px-3 py-2 space-y-3">
          {realtimeDetail.length > 0 && (
            <div>
              <div className="text-[10px] text-neutral-500 mb-1 uppercase tracking-wider">Realtime — 按 session</div>
              <div className="space-y-0.5">
                {realtimeDetail.map(d => (
                  <div key={d.sessionId} className="flex items-center gap-2 text-xs">
                    <span className="text-neutral-400 font-mono w-24 shrink-0 truncate" title={d.sessionId}>
                      {shortSessionId(d.sessionId)}
                    </span>
                    <span className="text-neutral-300">{d.msgCount} 条</span>
                    <span className="text-neutral-500">最早 {timeAgo(d.oldest)}</span>
                  </div>
                ))}
              </div>
            </div>
          )}
          {deepDetail.length > 0 && (
            <div>
              <div className="text-[10px] text-neutral-500 mb-1 uppercase tracking-wider">Deep Analysis — 按会话</div>
              <div className="space-y-0.5">
                {deepDetail.map(d => (
                  <div key={d.conversationId} className="flex items-center gap-2 text-xs">
                    <span className="text-neutral-400 font-mono w-24 shrink-0 truncate" title={d.conversationId}>
                      {shortSessionId(d.conversationId)}
                    </span>
                    <span className="text-neutral-500">结束于 {timeAgo(d.endedAt)}</span>
                    {d.retries > 0 && (
                      <Badge variant="outline" className="text-[9px] px-1 py-0 border-0 text-yellow-400 bg-yellow-500/10">
                        retry {d.retries}
                      </Badge>
                    )}
                  </div>
                ))}
              </div>
            </div>
          )}
        </div>
      )}
    </div>
  );
}

export function MemoryDashboard() {
  const [status, setStatus] = useState<MemoryStatus | null>(null);
  const [loading, setLoading] = useState(false);
  const [toggling, setToggling] = useState(false);
  const [showSubcategories, setShowSubcategories] = useState(false);

  const fetchStatus = useCallback(async () => {
    try {
      setLoading(true);
      const res = await fetch('/api/memory/status');
      if (res.ok) {
        const data = await res.json();
        setStatus(data);
      }
    } catch { /* ignore */ }
    finally { setLoading(false); }
  }, []);

  useEffect(() => {
    fetchStatus();
    const id = setInterval(fetchStatus, 5000);
    return () => clearInterval(id);
  }, [fetchStatus]);

  const togglePause = async () => {
    if (!status || toggling) return;
    setToggling(true);
    try {
      await fetch('/api/memory/pause', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ paused: !status.paused }),
      });
      await fetchStatus();
    } catch { /* ignore */ }
    finally { setToggling(false); }
  };

  if (!status) {
    return (
      <div className="flex-1 flex items-center justify-center text-neutral-500 text-sm">
        <RefreshCw className="w-4 h-4 animate-spin mr-2" /> Loading...
      </div>
    );
  }

  const categories = (showSubcategories ? status.kbStats?.subcategories : status.kbStats?.categories) || {};
  const totalKb = status.kbStats?.total || 0;

  // Compute recent output stats
  const completedTasks = status.recentTasks.filter(t => t.status === 'completed');
  const totalOutput = completedTasks.reduce((sum, t) => sum + (t.outputCount || 0), 0);

  return (
    <div className="flex-1 overflow-auto px-4 sm:px-8 pb-8 max-w-5xl">
      {/* Header */}
      <div className="flex items-center justify-between mb-4 mt-2">
        <div className="flex items-center gap-2">
          <Activity className="w-4 h-4 text-orange-400" />
          <span className="text-sm font-medium text-neutral-300">记忆流水线</span>
          {status.paused && (
            <Badge variant="outline" className="text-[10px] px-1.5 py-0 border-red-500/30 text-red-400 bg-red-500/10">
              已暂停
            </Badge>
          )}
        </div>
        <div className="flex items-center gap-2">
          <button
            onClick={togglePause}
            disabled={toggling}
            className={cn(
              'flex items-center gap-1 px-2 py-1 text-xs rounded transition-colors',
              status.paused
                ? 'text-green-400 hover:bg-green-500/10'
                : 'text-red-400 hover:bg-red-500/10',
            )}
          >
            {status.paused ? <Play className="w-3 h-3" /> : <Pause className="w-3 h-3" />}
            {status.paused ? '恢复' : '暂停'}
          </button>
          <button
            onClick={fetchStatus}
            disabled={loading}
            className="text-neutral-500 hover:text-neutral-300 transition-colors p-1"
          >
            <RefreshCw className={cn('w-3.5 h-3.5', loading && 'animate-spin')} />
          </button>
        </div>
      </div>

      {/* Lane Status Cards */}
      <div className="flex gap-3 mb-4">
        <LaneCard label="Fast Lane" lane={status.fastLane} icon="fast" />
        <LaneCard label="Slow Lane" lane={status.slowLane} icon="slow" />

        {/* Pending Queue */}
        <div className="flex-1 rounded-lg border border-neutral-800 bg-neutral-900/50 p-3">
          <div className="text-xs font-medium text-neutral-400 mb-2 flex items-center gap-1.5">
            <AlertCircle className="w-3 h-3" />
            待处理队列
          </div>
          <div className="flex gap-4">
            <div>
              <div className="text-lg font-semibold text-white">{status.pendingRealtime}</div>
              <div className="text-[10px] text-neutral-500">Realtime</div>
            </div>
            <div>
              <div className="text-lg font-semibold text-white">{status.pendingDeep}</div>
              <div className="text-[10px] text-neutral-500">Deep Analysis</div>
            </div>
          </div>
        </div>
      </div>

      {/* Queue Detail (collapsible) */}
      <QueueDetailPanel
        realtimeDetail={status.realtimeDetail || []}
        deepDetail={status.deepDetail || []}
      />

      {/* KB Stats + Timers */}
      <div className="flex gap-3 mb-4">
        <div className="flex-1 rounded-lg border border-neutral-800 bg-neutral-900/50 p-3">
          <div className="text-xs font-medium text-neutral-400 mb-2 flex items-center gap-1.5">
            <Database className="w-3 h-3" />
            知识库
          </div>
          <div className="flex items-end gap-4">
            <div>
              <div className="text-lg font-semibold text-white">{totalKb}</div>
              <div className="text-[10px] text-neutral-500">总条目</div>
            </div>
            <div>
              <div className="text-sm font-medium text-neutral-300">{status.kbStats?.neverAccessed || 0}</div>
              <div className="text-[10px] text-neutral-500">未访问</div>
            </div>
            {totalOutput > 0 && (
              <div className="flex items-center gap-1">
                <TrendingUp className="w-3 h-3 text-green-400" />
                <div>
                  <div className="text-sm font-medium text-green-400">+{totalOutput}</div>
                  <div className="text-[10px] text-neutral-500">近期产出</div>
                </div>
              </div>
            )}
          </div>
          {/* Most accessed + oldest */}
          {(status.kbStats?.mostAccessed || status.kbStats?.oldest) && (
            <div className="mt-2 pt-2 border-t border-neutral-800 flex gap-4 text-[10px]">
              {status.kbStats?.mostAccessed && (
                <div className="text-neutral-500">
                  热门: <span className="text-neutral-400">{status.kbStats.mostAccessed.category}/{status.kbStats.mostAccessed.key}</span>
                  <span className="text-neutral-500 ml-1">({status.kbStats.mostAccessed.accessCount}次)</span>
                </div>
              )}
              {status.kbStats?.oldest && (
                <div className="text-neutral-500">
                  最旧: <span className="text-neutral-400">{status.kbStats.oldest.category}/{status.kbStats.oldest.key}</span>
                  <span className="text-neutral-500 ml-1">({timeAgo(status.kbStats.oldest.updatedAt)})</span>
                </div>
              )}
            </div>
          )}
          {Object.keys(categories).length > 0 && (
            <div className="mt-2">
              <button
                onClick={() => setShowSubcategories(!showSubcategories)}
                className="text-[10px] text-neutral-500 hover:text-neutral-400 transition-colors mb-1"
              >
                {showSubcategories ? '收起子分类' : '展开子分类'}
              </button>
              <div className="flex flex-wrap gap-1.5">
                {Object.entries(categories).sort(([,a], [,b]) => (b as number) - (a as number)).map(([cat, count]) => (
                  <span key={cat} className="text-[10px] px-1.5 py-0.5 rounded bg-neutral-800 text-neutral-400">
                    {cat}: {count as number}
                  </span>
                ))}
              </div>
            </div>
          )}
        </div>
        <div className="w-48 rounded-lg border border-neutral-800 bg-neutral-900/50 p-3">
          <div className="text-xs font-medium text-neutral-400 mb-2">定时任务</div>
          <div className="space-y-1.5 text-xs">
            <div className="flex justify-between">
              <span className="text-neutral-500">KB 整理</span>
              <span className="text-neutral-300">{status.lastKbConsolidation ? timeAgo(status.lastKbConsolidation) : '未运行'}</span>
            </div>
            <div className="flex justify-between">
              <span className="text-neutral-500">Auto GC</span>
              <span className="text-neutral-300">{status.lastAutoGc ? timeAgo(status.lastAutoGc) : '未运行'}</span>
            </div>
          </div>
        </div>
      </div>

      {/* Recent Tasks Table */}
      <div className="rounded-lg border border-neutral-800 overflow-hidden">
        <div className="px-3 py-2 bg-neutral-900/80 border-b border-neutral-800">
          <span className="text-xs font-medium text-neutral-400">最近任务</span>
        </div>
        <div className="divide-y divide-neutral-800/50">
          {status.recentTasks.length === 0 ? (
            <div className="px-3 py-4 text-center text-xs text-neutral-600">暂无任务记录</div>
          ) : (
            status.recentTasks.map((task) => {
              const st = STATUS_STYLES[task.status] || STATUS_STYLES.pending;
              return (
                <div key={task.id} className="flex items-center gap-3 px-3 py-2 text-xs hover:bg-neutral-900/50">
                  <span className="text-neutral-500 w-16 shrink-0">{timeAgo(task.createdAt)}</span>
                  <span className="text-neutral-500 w-24 shrink-0 font-mono">{task.slotId.replace('slot-', '')}</span>
                  <span className="text-neutral-300 w-28 shrink-0">{TYPE_LABELS[task.taskType] || task.taskType}</span>
                  <Badge variant="outline" className={cn('text-[10px] px-1.5 py-0 border-0 shrink-0', st.color, st.bg)}>
                    {task.status}
                  </Badge>
                  <span className="text-neutral-500 w-12 text-right shrink-0">{formatDuration(task.durationMs)}</span>
                  {task.status === 'completed' && task.outputCount > 0 ? (
                    <span className="text-green-400 w-10 text-right shrink-0">+{task.outputCount}</span>
                  ) : (
                    <span className="w-10 shrink-0" />
                  )}
                  {task.error ? (
                    <span className="text-red-400 truncate flex-1" title={task.error}>{task.error}</span>
                  ) : task.sourceSessions ? (
                    <span className="text-neutral-600 truncate flex-1 font-mono text-[9px]" title={task.sourceSessions}>
                      {task.sourceSessions}
                    </span>
                  ) : (
                    <span className="flex-1" />
                  )}
                </div>
              );
            })
          )}
        </div>
      </div>
    </div>
  );
}
