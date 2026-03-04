'use client';

import { useState, useEffect, useCallback, useMemo } from 'react';
import {
  Rocket, RefreshCw, Clock,
  ChevronDown, AlertTriangle, Filter, RotateCcw,
  TrendingUp, Loader2, CircleDot
} from 'lucide-react';
import { cn } from '@/lib/utils';
import { Badge } from '@/components/ui/badge';

// ── Types ──────────────────────────────────────────────────

interface DeployTask {
  id: string;
  title: string;
  description: string;
  status: string;
  priority: string;
  category: string;
  project?: string;
  assignee?: string;
  autoExecute: boolean;
  retryCount: number;
  maxRetries: number;
  claimExecutorId?: string;
  claimExecutorType?: string;
  claimedAt?: string;
  leaseExpiresAt?: string;
  flowPhase?: string;
  dependsOn?: string[];
  hidden: boolean;
  createdAt: string;
  updatedAt: string;
}

interface TaskNote {
  id: string;
  taskId: string;
  content: string;
  noteType: string;
  author?: string;
  createdAt: string;
}

interface SlotStatus {
  slotId: string;
  state: string;
  taskTitle?: string;
  duration?: number;
}

interface DeployStats {
  todayCount: number;
  successRate: number;
  avgDurationMs: number | null;
  failedCount: number;
  runningCount: number;
  totalRetries: number;
  totalTasks: number;
}

interface DeployStatusResponse {
  stats: DeployStats;
  slots: SlotStatus[];
  tasks: DeployTask[];
}

// ── Helpers ────────────────────────────────────────────────

function formatDuration(ms: number | null | undefined): string {
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

function taskDuration(task: DeployTask): number | null {
  if (!task.claimedAt) return null;
  const end = task.status === 'running' || task.status === 'verifying' ? Date.now() : new Date(task.updatedAt).getTime();
  const start = new Date(task.claimedAt).getTime();
  return end - start > 0 ? end - start : null;
}

function shortId(id: string): string {
  return id.slice(0, 8);
}

// ── Constants ──────────────────────────────────────────────

const STATUS_STYLES: Record<string, { color: string; bg: string; label: string }> = {
  open: { color: 'text-neutral-400', bg: 'bg-neutral-500/10', label: 'open' },
  running: { color: 'text-blue-400', bg: 'bg-blue-500/10', label: 'running' },
  verifying: { color: 'text-yellow-400', bg: 'bg-yellow-500/10', label: 'verifying' },
  done: { color: 'text-green-400', bg: 'bg-green-500/10', label: 'done' },
  failed: { color: 'text-red-400', bg: 'bg-red-500/10', label: 'failed' },
  blocked: { color: 'text-orange-400', bg: 'bg-orange-500/10', label: 'blocked' },
  skipped: { color: 'text-neutral-500', bg: 'bg-neutral-500/10', label: 'skipped' },
};

const PRIORITY_DOT: Record<string, string> = {
  high: 'bg-red-400',
  medium: 'bg-yellow-400',
  low: 'bg-neutral-500',
};

const REFRESH_OPTIONS = [
  { label: '10s', value: 10000 },
  { label: '30s', value: 30000 },
  { label: '1m', value: 60000 },
  { label: 'Off', value: 0 },
];

// ── Sub-components ─────────────────────────────────────────

function MetricCard({ icon, label, value, sub, alert }: {
  icon: React.ReactNode;
  label: string;
  value: string | number;
  sub?: string;
  alert?: boolean;
}) {
  return (
    <div className={cn(
      'flex-1 rounded-lg border p-3',
      alert ? 'border-red-500/30 bg-red-500/5' : 'border-neutral-800 bg-neutral-900/50',
    )}>
      <div className="flex items-center gap-1.5 mb-1.5">
        {icon}
        <span className="text-[10px] text-neutral-500 uppercase tracking-wider">{label}</span>
      </div>
      <div className="flex items-end gap-1">
        <span className={cn('text-lg font-semibold', alert ? 'text-red-400' : 'text-white')}>{value}</span>
      </div>
      {sub && <div className="text-[10px] text-neutral-500 mt-0.5">{sub}</div>}
    </div>
  );
}

function SlotCard({ slot }: { slot: SlotStatus }) {
  const isActive = slot.state !== 'Idle' && slot.state !== 'idle' && slot.state !== 'Exited' && slot.state !== 'stopped';
  return (
    <div className={cn(
      'flex-1 rounded-lg border p-3 min-w-0',
      isActive ? 'border-blue-500/30 bg-blue-500/5' : 'border-neutral-800 bg-neutral-900/50',
    )}>
      <div className="flex items-center justify-between mb-1">
        <span className="text-xs font-medium text-neutral-300 font-mono">{slot.slotId.replace('slot-', '')}</span>
        <Badge variant="outline" className={cn(
          'text-[10px] px-1.5 py-0 border-0',
          isActive ? 'text-blue-400 bg-blue-500/10' : 'text-neutral-500 bg-neutral-500/10',
        )}>
          {isActive ? 'Active' : 'Idle'}
        </Badge>
      </div>
      {slot.taskTitle && (
        <div className="text-xs text-neutral-400 truncate mt-1">{slot.taskTitle}</div>
      )}
      {slot.duration != null && (
        <div className="text-[10px] text-neutral-500 mt-0.5">已运行 {formatDuration(slot.duration)}</div>
      )}
    </div>
  );
}

function NotesTimeline({ notes }: { notes: TaskNote[] }) {
  if (notes.length === 0) return <div className="text-[10px] text-neutral-600">暂无执行记录</div>;
  return (
    <div className="space-y-1.5">
      {notes.map((note) => (
        <div key={note.id} className="flex gap-2">
          <div className="flex flex-col items-center">
            <div className={cn(
              'w-1.5 h-1.5 rounded-full mt-1.5 shrink-0',
              note.noteType === 'summary' ? 'bg-green-400' :
              note.noteType === 'progress' ? 'bg-blue-400' : 'bg-neutral-500',
            )} />
            <div className="w-px flex-1 bg-neutral-800 mt-0.5" />
          </div>
          <div className="min-w-0 pb-1">
            <div className="text-[10px] text-neutral-500">{timeAgo(note.createdAt)}</div>
            <div className="text-xs text-neutral-300 whitespace-pre-wrap break-words">{note.content}</div>
            {note.author && <div className="text-[9px] text-neutral-600 font-mono mt-0.5">{note.author}</div>}
          </div>
        </div>
      ))}
    </div>
  );
}

// ── Main Component ─────────────────────────────────────────

export function DeployDashboard() {
  const [data, setData] = useState<DeployStatusResponse | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [refreshInterval, setRefreshInterval] = useState(() => {
    if (typeof window === 'undefined') return 10000;
    return Number(localStorage.getItem('deploy:refresh')) || 10000;
  });
  const [taskFilter, setTaskFilter] = useState('all');
  const [expandedTaskId, setExpandedTaskId] = useState<string | null>(null);
  const [taskNotes, setTaskNotes] = useState<Record<string, TaskNote[]>>({});
  const [loadingNotes, setLoadingNotes] = useState<string | null>(null);

  // Persist refresh interval
  useEffect(() => {
    localStorage.setItem('deploy:refresh', String(refreshInterval));
  }, [refreshInterval]);

  const fetchData = useCallback(async () => {
    try {
      const res = await fetch('/api/deploy/status');
      if (!res.ok) throw new Error(`HTTP ${res.status}`);
      const json = await res.json();
      if (json.error) throw new Error(json.error);
      setData(json);
      setError(null);
    } catch (err) {
      setError(String(err));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    fetchData();
    if (refreshInterval > 0) {
      const id = setInterval(fetchData, refreshInterval);
      return () => clearInterval(id);
    }
  }, [fetchData, refreshInterval]);

  // Fetch notes when expanding a task
  const fetchNotes = useCallback(async (taskId: string) => {
    if (taskNotes[taskId]) return; // Already cached
    setLoadingNotes(taskId);
    try {
      const res = await fetch(`/api/tasks?action=get&id=${taskId}`);
      if (res.ok) {
        const taskData = await res.json();
        if (taskData.notes) {
          setTaskNotes((prev) => ({ ...prev, [taskId]: taskData.notes }));
        }
      }
    } catch {
      // Notes are optional
    } finally {
      setLoadingNotes(null);
    }
  }, [taskNotes]);

  const handleExpand = useCallback((taskId: string) => {
    if (expandedTaskId === taskId) {
      setExpandedTaskId(null);
    } else {
      setExpandedTaskId(taskId);
      fetchNotes(taskId);
    }
  }, [expandedTaskId, fetchNotes]);

  // Filter tasks
  const taskFilterCounts = useMemo(() => {
    if (!data) return { failed: 0, running: 0, done: 0 };
    return {
      failed: data.tasks.filter((t) => t.status === 'failed').length,
      running: data.tasks.filter((t) => t.status === 'running' || t.status === 'verifying').length,
      done: data.tasks.filter((t) => t.status === 'done').length,
    };
  }, [data]);

  const filteredTasks = useMemo(() => {
    if (!data) return [];
    if (taskFilter === 'failed') return data.tasks.filter((t) => t.status === 'failed');
    if (taskFilter === 'running') return data.tasks.filter((t) => t.status === 'running' || t.status === 'verifying');
    if (taskFilter === 'done') return data.tasks.filter((t) => t.status === 'done');
    return data.tasks;
  }, [data, taskFilter]);

  if (loading && !data) {
    return (
      <div className="flex-1 flex items-center justify-center">
        <Loader2 className="w-5 h-5 animate-spin text-neutral-600" />
      </div>
    );
  }

  if (error && !data) {
    return (
      <div className="flex-1 flex items-center justify-center text-sm text-red-400">
        <AlertTriangle className="w-4 h-4 mr-2" />
        {error}
      </div>
    );
  }

  if (!data) return null;

  const { stats, slots } = data;

  return (
    <div className="flex-1 overflow-auto px-4 sm:px-8 pb-8 max-w-5xl space-y-4 pt-2">
      {/* ── Header ── */}
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-2">
          <Rocket className="w-4 h-4 text-blue-400" />
          <span className="text-sm font-medium text-neutral-300">部署流水线</span>
          {stats.runningCount > 0 && (
            <Badge variant="outline" className="text-[10px] px-1.5 py-0 border-0 text-blue-400 bg-blue-500/10">
              {stats.runningCount} running
            </Badge>
          )}
        </div>
        <div className="flex items-center gap-2">
          {/* Refresh interval selector */}
          <div className="flex items-center gap-0.5 bg-neutral-900 rounded px-1 py-0.5">
            <Clock className="w-3 h-3 text-neutral-600 mr-0.5" />
            {REFRESH_OPTIONS.map((opt) => (
              <button
                key={opt.value}
                onClick={() => setRefreshInterval(opt.value)}
                className={cn(
                  'px-1.5 py-0.5 text-[10px] rounded transition-colors',
                  refreshInterval === opt.value
                    ? 'bg-neutral-800 text-neutral-200'
                    : 'text-neutral-600 hover:text-neutral-400',
                )}
              >
                {opt.label}
              </button>
            ))}
          </div>
          <button
            onClick={fetchData}
            className="p-1 text-neutral-600 hover:text-neutral-300 transition-colors"
          >
            <RefreshCw className="w-3.5 h-3.5" />
          </button>
        </div>
      </div>

      {/* ── L1: Stats Cards ── */}
      <div className="flex gap-3">
        <MetricCard
          icon={<Rocket className="w-3.5 h-3.5 text-blue-400" />}
          label="今日任务"
          value={stats.todayCount}
          sub={`总计 ${stats.totalTasks} 个部署任务`}
        />
        <MetricCard
          icon={<TrendingUp className="w-3.5 h-3.5 text-green-400" />}
          label="成功率"
          value={`${stats.successRate}%`}
          alert={stats.successRate < 80}
          sub={`${stats.failedCount} 个失败`}
        />
        <MetricCard
          icon={<Clock className="w-3.5 h-3.5 text-purple-400" />}
          label="平均耗时"
          value={formatDuration(stats.avgDurationMs)}
        />
        <MetricCard
          icon={<RotateCcw className="w-3.5 h-3.5 text-orange-400" />}
          label="失败/重试"
          value={stats.failedCount}
          alert={stats.failedCount > 0}
          sub={stats.totalRetries > 0 ? `${stats.totalRetries} 次重试` : undefined}
        />
      </div>

      {/* ── L2: Slot Status ── */}
      {slots.length > 0 && (
        <div className="flex gap-3">
          {slots.map((slot) => (
            <SlotCard key={slot.slotId} slot={slot} />
          ))}
        </div>
      )}

      {/* ── L3: Task List ── */}
      <div className="rounded-lg border border-neutral-800 overflow-hidden">
        <div className="px-3 py-2 bg-neutral-900/80 border-b border-neutral-800 flex items-center justify-between">
          <span className="text-xs font-medium text-neutral-400 flex items-center gap-1.5">
            <Filter className="w-3 h-3" />
            最近部署任务
          </span>
          <div className="flex items-center gap-1">
            {[
              { key: 'all', label: 'All', count: data.tasks.length },
              { key: 'failed', label: 'Failed', count: taskFilterCounts.failed },
              { key: 'running', label: 'Running', count: taskFilterCounts.running },
              { key: 'done', label: 'Done', count: taskFilterCounts.done },
            ].filter((f) => f.key === 'all' || f.count > 0).map((f) => (
              <button
                key={f.key}
                onClick={() => setTaskFilter(f.key)}
                className={cn(
                  'px-2 py-0.5 text-[10px] rounded-full transition-colors',
                  taskFilter === f.key
                    ? f.key === 'failed' ? 'bg-red-500/20 text-red-400'
                    : f.key === 'running' ? 'bg-blue-500/20 text-blue-400'
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
            <div className="px-3 py-8 text-center text-xs text-neutral-600">暂无部署任务</div>
          ) : (
            filteredTasks.map((task) => {
              const st = STATUS_STYLES[task.status] || STATUS_STYLES.open;
              const isExpanded = expandedTaskId === task.id;
              const duration = taskDuration(task);
              const notes = taskNotes[task.id];
              const isRunning = task.status === 'running' || task.status === 'verifying';

              return (
                <div key={task.id}>
                  {/* Main row */}
                  <div
                    className={cn(
                      'flex items-center gap-3 px-3 py-2 text-xs hover:bg-neutral-900/50 cursor-pointer',
                      isExpanded && 'bg-neutral-900/50',
                    )}
                    onClick={() => handleExpand(task.id)}
                  >
                    <span className="text-neutral-500 w-16 shrink-0">{timeAgo(task.updatedAt)}</span>
                    <div className={cn('w-1.5 h-1.5 rounded-full shrink-0', PRIORITY_DOT[task.priority] || PRIORITY_DOT.medium)} />
                    <span className="text-neutral-300 flex-1 min-w-0 truncate" title={task.title}>
                      {task.title}
                    </span>
                    {task.assignee && (
                      <span className="text-neutral-600 font-mono text-[10px] shrink-0">
                        {task.assignee.replace('slot-', '')}
                      </span>
                    )}
                    <Badge variant="outline" className={cn('text-[10px] px-1.5 py-0 border-0 shrink-0', st.color, st.bg)}>
                      {isRunning && <CircleDot className="w-2.5 h-2.5 mr-0.5 animate-pulse" />}
                      {st.label}
                    </Badge>
                    <span className={cn(
                      'w-14 text-right shrink-0',
                      task.status === 'failed' ? 'text-red-400' : 'text-neutral-500',
                    )}>
                      {formatDuration(duration)}
                    </span>
                    {task.retryCount > 0 && (
                      <span className="text-orange-400 text-[10px] shrink-0">
                        <RotateCcw className="w-2.5 h-2.5 inline mr-0.5" />{task.retryCount}
                      </span>
                    )}
                    <ChevronDown className={cn(
                      'w-3 h-3 text-neutral-600 shrink-0 transition-transform',
                      !isExpanded && '-rotate-90',
                    )} />
                  </div>

                  {/* Expanded detail */}
                  {isExpanded && (
                    <div className="px-3 py-3 bg-neutral-950/50 border-t border-neutral-800/50">
                      <div className="grid grid-cols-2 gap-4">
                        {/* Left: Notes timeline */}
                        <div>
                          <div className="text-[10px] text-neutral-500 uppercase tracking-wider mb-2">执行记录</div>
                          {loadingNotes === task.id ? (
                            <div className="flex items-center gap-1 text-[10px] text-neutral-600">
                              <Loader2 className="w-3 h-3 animate-spin" /> 加载中...
                            </div>
                          ) : notes ? (
                            <NotesTimeline notes={notes} />
                          ) : (
                            <div className="text-[10px] text-neutral-600">暂无执行记录</div>
                          )}
                        </div>

                        {/* Right: Metadata */}
                        <div className="space-y-2">
                          <div className="text-[10px] text-neutral-500 uppercase tracking-wider mb-2">任务信息</div>
                          {task.description && (
                            <div className="text-xs text-neutral-400 leading-relaxed">{task.description}</div>
                          )}
                          <div className="space-y-1 text-[10px]">
                            <div className="flex justify-between">
                              <span className="text-neutral-500">ID</span>
                              <span className="text-neutral-400 font-mono">{shortId(task.id)}</span>
                            </div>
                            {task.project && (
                              <div className="flex justify-between">
                                <span className="text-neutral-500">项目</span>
                                <span className="text-neutral-300">{task.project}</span>
                              </div>
                            )}
                            {task.assignee && (
                              <div className="flex justify-between">
                                <span className="text-neutral-500">工位</span>
                                <span className="text-neutral-300 font-mono">{task.assignee}</span>
                              </div>
                            )}
                            {task.claimExecutorId && (
                              <div className="flex justify-between">
                                <span className="text-neutral-500">执行者</span>
                                <span className="text-neutral-300 font-mono">{task.claimExecutorId}</span>
                              </div>
                            )}
                            {task.claimedAt && (
                              <div className="flex justify-between">
                                <span className="text-neutral-500">开始时间</span>
                                <span className="text-neutral-400">{new Date(task.claimedAt).toLocaleString('zh-CN')}</span>
                              </div>
                            )}
                            {task.dependsOn && task.dependsOn.length > 0 && (
                              <div className="flex justify-between">
                                <span className="text-neutral-500">依赖</span>
                                <span className="text-neutral-400 font-mono">{task.dependsOn.map(shortId).join(', ')}</span>
                              </div>
                            )}
                            <div className="flex justify-between">
                              <span className="text-neutral-500">自动执行</span>
                              <span className={task.autoExecute ? 'text-green-400' : 'text-neutral-600'}>
                                {task.autoExecute ? '是' : '否'}
                              </span>
                            </div>
                            <div className="flex justify-between">
                              <span className="text-neutral-500">重试</span>
                              <span className={task.retryCount > 0 ? 'text-orange-400' : 'text-neutral-600'}>
                                {task.retryCount} / {task.maxRetries}
                              </span>
                            </div>
                          </div>
                        </div>
                      </div>
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
