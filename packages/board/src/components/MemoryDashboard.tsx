'use client';

import { useState, useEffect, useCallback, useMemo } from 'react';
import {
  Activity, Pause, Play, RefreshCw, Zap, Clock, Database,
  AlertCircle, AlertTriangle, ChevronDown, ChevronRight,
  TrendingUp, CheckCircle2, XCircle, Timer, Coins,
  BarChart3, Filter
} from 'lucide-react';
import { cn } from '@/lib/utils';
import { Badge } from '@/components/ui/badge';

// ── Types ──────────────────────────────────────────────────

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

interface TokenStatsRow {
  group_key?: string;
  total_input: number;
  total_cache_creation: number;
  total_cache_read: number;
  total_output: number;
  record_count: number;
}

interface TaskStats {
  total: number;
  completed: number;
  failed: number;
  running: number;
  totalOutput: number;
  byType: Array<{
    taskType: string;
    count: number;
    avgDurationMs: number | null;
  }>;
}

// ── Helpers ────────────────────────────────────────────────

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

function shortId(id: string): string {
  return id.length > 12 ? id.slice(0, 8) + '...' : id;
}

function formatTokens(n: number): string {
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`;
  if (n >= 1_000) return `${(n / 1_000).toFixed(1)}K`;
  return String(n);
}

// ── Constants ──────────────────────────────────────────────

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

const STUCK_THRESHOLD_SECS = 900;

const REFRESH_OPTIONS = [
  { label: '5s', value: 5000 },
  { label: '10s', value: 10000 },
  { label: '30s', value: 30000 },
  { label: '1m', value: 60000 },
  { label: 'Off', value: 0 },
];

// ── Sub-components ─────────────────────────────────────────

function LaneCard({ label, lane, icon }: { label: string; lane: LaneStatus; icon: 'fast' | 'slow' }) {
  const phase = PHASE_STYLES[lane.phase] || PHASE_STYLES.Idle;
  const isActive = lane.phase !== 'Idle';
  const isStuck = isActive && lane.busyDuration > STUCK_THRESHOLD_SECS;

  return (
    <div className={cn(
      'flex-1 rounded-lg border p-3 min-w-0',
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
          {lane.currentTargets && lane.currentTargets.length > 0 && (
            <div className="mt-1 pt-1 border-t border-neutral-800">
              <span className="text-neutral-500">目标:</span>
              <div className="flex flex-wrap gap-1 mt-0.5">
                {lane.currentTargets.map(sid => (
                  <span key={sid} className="text-[9px] px-1 py-0 rounded bg-neutral-800 text-neutral-400 font-mono" title={sid}>
                    {shortId(sid)}
                  </span>
                ))}
              </div>
            </div>
          )}
          {lane.currentConvId && (
            <div className="mt-1 pt-1 border-t border-neutral-800">
              <span className="text-neutral-500">会话: </span>
              <span className="text-neutral-400 font-mono text-[9px]" title={lane.currentConvId}>
                {shortId(lane.currentConvId)}
              </span>
            </div>
          )}
        </div>
      )}
    </div>
  );
}

function QueuePressureBar({ count, label, max = 100 }: { count: number; label: string; max?: number }) {
  const pct = Math.min((count / max) * 100, 100);
  const color = count === 0 ? 'bg-neutral-700' :
    count < 30 ? 'bg-blue-500' :
    count < 70 ? 'bg-yellow-500' : 'bg-red-500';
  const textColor = count === 0 ? 'text-neutral-500' :
    count < 30 ? 'text-blue-400' :
    count < 70 ? 'text-yellow-400' : 'text-red-400';

  return (
    <div className="flex-1">
      <div className="flex items-end justify-between mb-1">
        <span className={cn('text-lg font-semibold', textColor)}>{count}</span>
        <span className="text-[10px] text-neutral-500">{label}</span>
      </div>
      <div className="h-1.5 rounded-full bg-neutral-800 overflow-hidden">
        <div className={cn('h-full rounded-full transition-all duration-500', color)} style={{ width: `${pct}%` }} />
      </div>
    </div>
  );
}

function MetricCard({ icon, label, value, sub, trend }: {
  icon: React.ReactNode;
  label: string;
  value: string | number;
  sub?: string;
  trend?: 'up' | 'down' | null;
}) {
  return (
    <div className="flex-1 rounded-lg border border-neutral-800 bg-neutral-900/50 p-3">
      <div className="flex items-center gap-1.5 mb-1.5">
        {icon}
        <span className="text-[10px] text-neutral-500 uppercase tracking-wider">{label}</span>
      </div>
      <div className="flex items-end gap-1">
        <span className="text-lg font-semibold text-white">{value}</span>
        {trend === 'up' && <TrendingUp className="w-3 h-3 text-green-400 mb-0.5" />}
        {trend === 'down' && <TrendingUp className="w-3 h-3 text-red-400 mb-0.5 rotate-180" />}
      </div>
      {sub && <div className="text-[10px] text-neutral-500 mt-0.5">{sub}</div>}
    </div>
  );
}

function CategoryTreemap({ categories }: { categories: Record<string, number> }) {
  const entries = Object.entries(categories).sort(([, a], [, b]) => b - a);
  const total = entries.reduce((s, [, v]) => s + v, 0);
  if (total === 0) return null;

  const colors = [
    'bg-blue-500/20 text-blue-300',
    'bg-purple-500/20 text-purple-300',
    'bg-green-500/20 text-green-300',
    'bg-orange-500/20 text-orange-300',
    'bg-cyan-500/20 text-cyan-300',
    'bg-pink-500/20 text-pink-300',
    'bg-yellow-500/20 text-yellow-300',
    'bg-red-500/20 text-red-300',
  ];

  return (
    <div className="flex flex-wrap gap-1">
      {entries.map(([cat, count], i) => {
        const pct = ((count / total) * 100).toFixed(0);
        return (
          <div
            key={cat}
            className={cn(
              'rounded px-2 py-1 text-[10px] flex items-center gap-1',
              colors[i % colors.length],
            )}
            style={{ flexBasis: `${Math.max(Number(pct) * 1.5, 12)}%` }}
          >
            <span className="truncate font-medium">{cat}</span>
            <span className="opacity-60 shrink-0">{count}</span>
          </div>
        );
      })}
    </div>
  );
}

function QueueDetailPanel({ realtimeDetail, deepDetail }: { realtimeDetail: RealtimeDetail[]; deepDetail: DeepDetail[] }) {
  const [expanded, setExpanded] = useState(false);
  const hasDetail = realtimeDetail.length > 0 || deepDetail.length > 0;

  if (!hasDetail) return null;

  return (
    <div className="rounded-lg border border-neutral-800 overflow-hidden">
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
                    <span className="text-neutral-400 font-mono w-24 shrink-0 truncate" title={d.sessionId}>{shortId(d.sessionId)}</span>
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
                    <span className="text-neutral-400 font-mono w-24 shrink-0 truncate" title={d.conversationId}>{shortId(d.conversationId)}</span>
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

// ── Main Component ─────────────────────────────────────────

export function MemoryDashboard() {
  const [status, setStatus] = useState<MemoryStatus | null>(null);
  const [tokenStats, setTokenStats] = useState<TokenStatsRow[] | null>(null);
  const [taskStats, setTaskStats] = useState<TaskStats | null>(null);
  const [loading, setLoading] = useState(false);
  const [toggling, setToggling] = useState(false);
  const [showSubcategories, setShowSubcategories] = useState(false);
  const [taskFilter, setTaskFilter] = useState<string>('all');
  const [expandedTaskId, setExpandedTaskId] = useState<string | null>(null);
  const [refreshInterval, setRefreshInterval] = useState(5000);
  const [showRefreshMenu, setShowRefreshMenu] = useState(false);

  const fetchAll = useCallback(async () => {
    try {
      setLoading(true);
      const [statusRes, tokenRes, taskStatsRes] = await Promise.allSettled([
        fetch('/api/memory/status'),
        fetch('/api/memory/token-stats'),
        fetch('/api/memory/task-stats'),
      ]);
      if (statusRes.status === 'fulfilled' && statusRes.value.ok) {
        setStatus(await statusRes.value.json());
      }
      if (tokenRes.status === 'fulfilled' && tokenRes.value.ok) {
        setTokenStats(await tokenRes.value.json());
      }
      if (taskStatsRes.status === 'fulfilled' && taskStatsRes.value.ok) {
        setTaskStats(await taskStatsRes.value.json());
      }
    } catch { /* ignore */ }
    finally { setLoading(false); }
  }, []);

  useEffect(() => {
    fetchAll();
    if (refreshInterval > 0) {
      const id = setInterval(fetchAll, refreshInterval);
      return () => clearInterval(id);
    }
  }, [fetchAll, refreshInterval]);

  const togglePause = async () => {
    if (!status || toggling) return;
    setToggling(true);
    try {
      await fetch('/api/memory/pause', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ paused: !status.paused }),
      });
      await fetchAll();
    } catch { /* ignore */ }
    finally { setToggling(false); }
  };

  // Derived data
  const totalTokens24h = useMemo(() => {
    if (!tokenStats) return 0;
    return tokenStats.reduce((sum, r) => sum + (r.total_input || 0) + (r.total_output || 0), 0);
  }, [tokenStats]);

  const successRate = useMemo(() => {
    if (!taskStats || taskStats.total === 0) return null;
    return ((taskStats.completed / taskStats.total) * 100).toFixed(1);
  }, [taskStats]);

  const avgDuration = useMemo(() => {
    if (!taskStats?.byType) return null;
    const completed = taskStats.byType.filter(t => t.avgDurationMs != null);
    if (completed.length === 0) return null;
    const avg = completed.reduce((s, t) => s + (t.avgDurationMs || 0), 0) / completed.length;
    return formatDuration(avg);
  }, [taskStats]);

  const filteredTasks = useMemo(() => {
    if (!status) return [];
    if (taskFilter === 'all') return status.recentTasks;
    return status.recentTasks.filter(t => t.status === taskFilter);
  }, [status, taskFilter]);

  const taskFilterCounts = useMemo(() => {
    if (!status) return { failed: 0, running: 0, completed: 0, pending: 0 };
    const tasks = status.recentTasks;
    return {
      failed: tasks.filter(t => t.status === 'failed').length,
      running: tasks.filter(t => t.status === 'running').length,
      completed: tasks.filter(t => t.status === 'completed').length,
      pending: tasks.filter(t => t.status === 'pending').length,
    };
  }, [status]);

  // System health
  const systemHealth = useMemo(() => {
    if (!status) return 'unknown';
    const hasStuck = (status.fastLane.phase !== 'Idle' && status.fastLane.busyDuration > STUCK_THRESHOLD_SECS)
      || (status.slowLane.phase !== 'Idle' && status.slowLane.busyDuration > STUCK_THRESHOLD_SECS);
    if (status.paused) return 'paused';
    if (hasStuck) return 'error';
    if (taskFilterCounts.failed > 3) return 'warning';
    return 'healthy';
  }, [status, taskFilterCounts]);

  const healthConfig = {
    healthy: { color: 'text-green-400', bg: 'bg-green-500', label: 'Healthy' },
    warning: { color: 'text-yellow-400', bg: 'bg-yellow-500', label: 'Warning' },
    error: { color: 'text-red-400', bg: 'bg-red-500', label: 'Error' },
    paused: { color: 'text-neutral-400', bg: 'bg-neutral-500', label: 'Paused' },
    unknown: { color: 'text-neutral-500', bg: 'bg-neutral-600', label: '...' },
  };
  const health = healthConfig[systemHealth];

  if (!status) {
    return (
      <div className="flex-1 flex items-center justify-center text-neutral-500 text-sm">
        <RefreshCw className="w-4 h-4 animate-spin mr-2" /> Loading...
      </div>
    );
  }

  const categories = (showSubcategories ? status.kbStats?.subcategories : status.kbStats?.categories) || {};
  const completedTasks = status.recentTasks.filter(t => t.status === 'completed');
  const totalOutput = completedTasks.reduce((sum, t) => sum + (t.outputCount || 0), 0);

  return (
    <div className="flex-1 overflow-auto px-4 sm:px-8 pb-8 max-w-6xl">
      {/* ── L0: Header ── */}
      <div className="flex items-center justify-between mb-4 mt-2">
        <div className="flex items-center gap-3">
          <div className="flex items-center gap-2">
            <div className={cn('w-2 h-2 rounded-full', health.bg)} />
            <span className="text-sm font-medium text-neutral-300">记忆流水线</span>
          </div>
          <Badge variant="outline" className={cn('text-[10px] px-1.5 py-0 border-0', health.color, `${health.bg}/10`)}>
            {health.label}
          </Badge>
          {status.paused && (
            <Badge variant="outline" className="text-[10px] px-1.5 py-0 border-red-500/30 text-red-400 bg-red-500/10">
              已暂停
            </Badge>
          )}
        </div>
        <div className="flex items-center gap-2">
          {/* Auto-refresh control */}
          <div className="relative">
            <button
              onClick={() => setShowRefreshMenu(!showRefreshMenu)}
              className="flex items-center gap-1 px-2 py-1 text-[10px] text-neutral-500 hover:text-neutral-300 rounded transition-colors"
            >
              <Timer className="w-3 h-3" />
              {REFRESH_OPTIONS.find(o => o.value === refreshInterval)?.label || '5s'}
            </button>
            {showRefreshMenu && (
              <>
                <div className="fixed inset-0 z-10" onClick={() => setShowRefreshMenu(false)} />
                <div className="absolute right-0 top-full mt-1 bg-neutral-900 border border-neutral-700 rounded-md shadow-lg z-20 py-1">
                  {REFRESH_OPTIONS.map(opt => (
                    <button
                      key={opt.value}
                      onClick={() => { setRefreshInterval(opt.value); setShowRefreshMenu(false); }}
                      className={cn(
                        'block w-full text-left px-3 py-1 text-xs transition-colors',
                        opt.value === refreshInterval ? 'text-blue-400 bg-blue-500/10' : 'text-neutral-400 hover:text-neutral-200 hover:bg-neutral-800',
                      )}
                    >
                      {opt.label}
                    </button>
                  ))}
                </div>
              </>
            )}
          </div>
          <button
            onClick={togglePause}
            disabled={toggling}
            className={cn(
              'flex items-center gap-1 px-2 py-1 text-xs rounded transition-colors',
              status.paused ? 'text-green-400 hover:bg-green-500/10' : 'text-red-400 hover:bg-red-500/10',
            )}
          >
            {status.paused ? <Play className="w-3 h-3" /> : <Pause className="w-3 h-3" />}
            {status.paused ? '恢复' : '暂停'}
          </button>
          <button onClick={fetchAll} disabled={loading} className="text-neutral-500 hover:text-neutral-300 transition-colors p-1">
            <RefreshCw className={cn('w-3.5 h-3.5', loading && 'animate-spin')} />
          </button>
        </div>
      </div>

      {/* ── L0: Metrics Row ── */}
      <div className="grid grid-cols-4 gap-3 mb-4">
        <MetricCard
          icon={<Activity className="w-3 h-3 text-orange-400" />}
          label="运行中"
          value={
            (status.fastLane.phase !== 'Idle' ? 1 : 0)
            + (status.slowLane.phase !== 'Idle' ? 1 : 0)
          }
          sub={`Fast ${status.fastLane.phase !== 'Idle' ? '1' : '0'} / Slow ${status.slowLane.phase !== 'Idle' ? '1' : '0'}`}
        />
        <MetricCard
          icon={<CheckCircle2 className="w-3 h-3 text-green-400" />}
          label="成功率"
          value={successRate ? `${successRate}%` : '-'}
          sub={taskStats ? `${taskStats.completed}/${taskStats.total} 任务` : undefined}
          trend={successRate && Number(successRate) >= 90 ? 'up' : successRate && Number(successRate) < 70 ? 'down' : null}
        />
        <MetricCard
          icon={<Timer className="w-3 h-3 text-blue-400" />}
          label="平均耗时"
          value={avgDuration || '-'}
          sub={taskStats?.byType?.map(t => `${TYPE_LABELS[t.taskType] || t.taskType}: ${formatDuration(t.avgDurationMs)}`).join(' / ')}
        />
        <MetricCard
          icon={<Coins className="w-3 h-3 text-yellow-400" />}
          label="24h Token"
          value={formatTokens(totalTokens24h)}
          sub={tokenStats?.map(r => `${r.group_key || '?'}: ${formatTokens(r.total_input + r.total_output)}`).join(' / ')}
        />
      </div>

      {/* ── L1: Pipeline ── */}
      <div className="flex gap-3 mb-3">
        <LaneCard label="Fast Lane" lane={status.fastLane} icon="fast" />
        <LaneCard label="Slow Lane" lane={status.slowLane} icon="slow" />
        {/* Queue pressure */}
        <div className="flex-1 rounded-lg border border-neutral-800 bg-neutral-900/50 p-3">
          <div className="text-xs font-medium text-neutral-400 mb-3 flex items-center gap-1.5">
            <AlertCircle className="w-3 h-3" />
            待处理队列
          </div>
          <div className="flex gap-4">
            <QueuePressureBar count={status.pendingRealtime} label="Realtime" />
            <QueuePressureBar count={status.pendingDeep} label="Deep" />
          </div>
        </div>
      </div>

      {/* Queue Detail (collapsible) */}
      <div className="mb-4">
        <QueueDetailPanel realtimeDetail={status.realtimeDetail || []} deepDetail={status.deepDetail || []} />
      </div>

      {/* ── L2: Knowledge + Timers ── */}
      <div className="flex gap-3 mb-4">
        <div className="flex-1 rounded-lg border border-neutral-800 bg-neutral-900/50 p-3">
          <div className="flex items-center justify-between mb-2">
            <div className="text-xs font-medium text-neutral-400 flex items-center gap-1.5">
              <Database className="w-3 h-3" />
              知识库
            </div>
            <button
              onClick={() => setShowSubcategories(!showSubcategories)}
              className="text-[10px] text-neutral-500 hover:text-neutral-400 transition-colors"
            >
              {showSubcategories ? '收起子分类' : '展开子分类'}
            </button>
          </div>

          {/* KB metrics row */}
          <div className="flex items-end gap-4 mb-3">
            <div>
              <div className="text-lg font-semibold text-white">{status.kbStats?.total || 0}</div>
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

          {/* Category treemap */}
          {Object.keys(categories).length > 0 && (
            <CategoryTreemap categories={categories} />
          )}

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
        </div>

        {/* Timers + Token breakdown */}
        <div className="w-52 space-y-3">
          <div className="rounded-lg border border-neutral-800 bg-neutral-900/50 p-3">
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
          {/* Token by model */}
          {tokenStats && tokenStats.length > 0 && (
            <div className="rounded-lg border border-neutral-800 bg-neutral-900/50 p-3">
              <div className="text-xs font-medium text-neutral-400 mb-2 flex items-center gap-1.5">
                <BarChart3 className="w-3 h-3" />
                Token 明细
              </div>
              <div className="space-y-1.5">
                {tokenStats.map((row, i) => {
                  const total = (row.total_input || 0) + (row.total_output || 0);
                  return (
                    <div key={i} className="text-xs">
                      <div className="flex justify-between mb-0.5">
                        <span className="text-neutral-500 truncate">{row.group_key || 'unknown'}</span>
                        <span className="text-neutral-300">{formatTokens(total)}</span>
                      </div>
                      <div className="flex gap-0.5 text-[9px] text-neutral-600">
                        <span>in:{formatTokens(row.total_input)}</span>
                        <span>out:{formatTokens(row.total_output)}</span>
                      </div>
                    </div>
                  );
                })}
              </div>
            </div>
          )}
        </div>
      </div>

      {/* ── L3: Recent Tasks ── */}
      <div className="rounded-lg border border-neutral-800 overflow-hidden">
        <div className="px-3 py-2 bg-neutral-900/80 border-b border-neutral-800 flex items-center justify-between">
          <span className="text-xs font-medium text-neutral-400 flex items-center gap-1.5">
            <Filter className="w-3 h-3" />
            最近任务
          </span>
          {/* Filter pills */}
          <div className="flex items-center gap-1">
            {[
              { key: 'all', label: 'All', count: status.recentTasks.length },
              { key: 'failed', label: 'Failed', count: taskFilterCounts.failed },
              { key: 'running', label: 'Running', count: taskFilterCounts.running },
              { key: 'completed', label: 'Done', count: taskFilterCounts.completed },
            ].filter(f => f.key === 'all' || f.count > 0).map(f => (
              <button
                key={f.key}
                onClick={() => setTaskFilter(f.key)}
                className={cn(
                  'px-2 py-0.5 text-[10px] rounded-full transition-colors',
                  taskFilter === f.key
                    ? f.key === 'failed' ? 'bg-red-500/20 text-red-400'
                    : f.key === 'running' ? 'bg-orange-500/20 text-orange-400'
                    : 'bg-neutral-700 text-neutral-200'
                    : 'text-neutral-500 hover:text-neutral-300 hover:bg-neutral-800',
                )}
              >
                {f.label} {f.count > 0 && f.key !== 'all' ? `(${f.count})` : ''}
              </button>
            ))}
          </div>
        </div>
        <div className="divide-y divide-neutral-800/50">
          {filteredTasks.length === 0 ? (
            <div className="px-3 py-4 text-center text-xs text-neutral-600">暂无任务记录</div>
          ) : (
            filteredTasks.map((task) => {
              const st = STATUS_STYLES[task.status] || STATUS_STYLES.pending;
              const isExpanded = expandedTaskId === task.id;
              return (
                <div key={task.id}>
                  <div
                    className={cn(
                      'flex items-center gap-3 px-3 py-2 text-xs hover:bg-neutral-900/50 cursor-pointer',
                      isExpanded && 'bg-neutral-900/50',
                    )}
                    onClick={() => setExpandedTaskId(isExpanded ? null : task.id)}
                  >
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
                      <XCircle className="w-3 h-3 text-red-400 shrink-0" />
                    ) : (
                      <span className="w-3 shrink-0" />
                    )}
                    {(task.error || task.sourceSessions) && (
                      <ChevronDown className={cn('w-3 h-3 text-neutral-600 shrink-0 transition-transform', !isExpanded && '-rotate-90')} />
                    )}
                  </div>
                  {isExpanded && (task.error || task.sourceSessions) && (
                    <div className="px-3 py-2 bg-neutral-950/50 border-t border-neutral-800/50">
                      {task.error && (
                        <div className="text-xs text-red-400 mb-1">
                          <span className="text-red-500 font-medium">Error: </span>{task.error}
                        </div>
                      )}
                      {task.sourceSessions && (
                        <div className="text-[10px] text-neutral-600 font-mono break-all">
                          Sessions: {task.sourceSessions}
                        </div>
                      )}
                      {task.conversationId && (
                        <div className="text-[10px] text-neutral-600 font-mono mt-0.5">
                          Conv: {task.conversationId}
                        </div>
                      )}
                    </div>
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
