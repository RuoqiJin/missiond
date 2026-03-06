'use client';

import { useState, useEffect, useCallback, useMemo } from 'react';
import {
  RefreshCw, Zap, Database, Brain, Settings2,
  ChevronDown, AlertTriangle, Filter, ArrowRight,
  RotateCcw, Loader2, Timer
} from 'lucide-react';
import { cn } from '@/lib/utils';
import { Badge } from '@/components/ui/badge';
import { useEventStreamStore } from '../eventStream';
import { useEventInvalidation, useConnectionState } from '../hooks/useEventStream';

// ── Types ──────────────────────────────────────────────────

interface EventStats {
  published: number;
  consumed: { extraction: number; submit: number; decision: number };
  lagged: { extraction: number; submit: number; decision: number; total_skipped: number };
  estimated_backlog: number;
}

interface DbExecStats {
  runs: number;
  avg_us: number;
  p50_us: number;
  p95_us: number;
  p99_us: number;
}

interface GeminiStats {
  requests: number;
  errors: number;
  retries: number;
  p50_ms: number;
  p95_ms: number;
  p99_ms: number;
}

interface AutopilotStats {
  ticks: number;
  avg_ms: number;
  p50_ms: number;
  p95_ms: number;
  p99_ms: number;
}

interface HealthData {
  status: string;
  pty: Array<{ slotId: string; state: string; pid: number | null }>;
  uptime_epoch: number;
  event_bus: { publish_count: number };
  gemini_mode: string;
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  memory?: any;
  stats: {
    events: EventStats;
    db_exec: DbExecStats;
    gemini: GeminiStats;
    autopilot: AutopilotStats;
    tasks: { dispatched: number; completed: number };
    decisions: { tier1_hit: number; tier2_hit: number; tier3_dispatched: number };
  };
}

interface GeminiTraceRow {
  id: string;
  caller: string;
  session_id: string | null;
  api_mode: string;
  model: string;
  prompt_chars: number;
  response_chars: number;
  queue_wait_ms: number;
  duration_ms: number;
  retry_count: number;
  status: string;
  error_msg: string | null;
  created_at: string;
}

interface GeminiAggStats {
  total: number;
  ok: number;
  errors: number;
  avg_duration_ms: number;
  avg_queue_wait_ms: number;
  total_prompt_chars: number;
  total_response_chars: number;
  by_caller: Array<{ caller: string; count: number; avg_duration_ms: number; errors: number }>;
}

interface LlmTracesResponse {
  traces: { count: number; requests: GeminiTraceRow[] } | null;
  stats: GeminiAggStats | null;
}

// ── Helpers ────────────────────────────────────────────────

function formatDuration(ms: number | null | undefined): string {
  if (ms == null) return '-';
  if (ms < 1) return '<1ms';
  if (ms < 1000) return `${Math.round(ms)}ms`;
  if (ms < 60000) return `${(ms / 1000).toFixed(1)}s`;
  return `${(ms / 60000).toFixed(1)}m`;
}

function formatMicros(us: number): string {
  if (us < 1000) return `${us}µs`;
  return `${(us / 1000).toFixed(1)}ms`;
}

function formatNumber(n: number): string {
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`;
  if (n >= 1_000) return `${(n / 1_000).toFixed(1)}K`;
  return String(n);
}

function formatChars(n: number): string {
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`;
  if (n >= 1_000) return `${(n / 1_000).toFixed(0)}K`;
  return String(n);
}

function timeAgo(dateStr: string): string {
  if (!dateStr) return '-';
  const diff = Date.now() - new Date(dateStr).getTime();
  const s = Math.floor(diff / 1000);
  if (s < 60) return `${s}s`;
  const m = Math.floor(s / 60);
  if (m < 60) return `${m}m`;
  const h = Math.floor(m / 60);
  if (h < 24) return `${h}h`;
  return `${Math.floor(h / 24)}d`;
}

function shortId(id: string): string {
  return id.length > 12 ? id.slice(0, 8) + '…' : id;
}

function thresholdColor(value: number, green: number, amber: number): string {
  if (value <= green) return 'text-green-400';
  if (value <= amber) return 'text-amber-400';
  return 'text-red-400';
}

function thresholdBg(value: number, green: number, amber: number): string {
  if (value <= green) return 'border-green-500/20 bg-green-500/5';
  if (value <= amber) return 'border-amber-500/20 bg-amber-500/5';
  return 'border-red-500/20 bg-red-500/5';
}

// ── Constants ──────────────────────────────────────────────

const REFRESH_OPTIONS = [
  { label: '5s', value: 5000 },
  { label: '10s', value: 10000 },
  { label: '30s', value: 30000 },
  { label: '1m', value: 60000 },
  { label: 'Off', value: 0 },
];

const TRACE_STATUS: Record<string, { color: string; bg: string; label: string }> = {
  ok: { color: 'text-green-400', bg: 'bg-green-500/10', label: 'ok' },
  error: { color: 'text-red-400', bg: 'bg-red-500/10', label: 'error' },
  timeout: { color: 'text-red-400', bg: 'bg-red-500/10', label: 'timeout' },
};

// ── Sub-components ─────────────────────────────────────────

function MetricCard({ icon, title, value, sub, color, borderColor, children }: {
  icon: React.ReactNode;
  title: string;
  value: string | number;
  sub?: string;
  color?: string;
  borderColor?: string;
  children?: React.ReactNode;
}) {
  return (
    <div className={cn(
      'flex-1 rounded-lg border p-3 min-w-0',
      borderColor || 'border-neutral-800 bg-neutral-900/50',
    )}>
      <div className="flex items-center gap-1.5 mb-1.5">
        {icon}
        <span className="text-[10px] text-neutral-500 uppercase tracking-wider">{title}</span>
      </div>
      <div className="flex items-end gap-1">
        <span className={cn('text-lg font-semibold', color || 'text-white')}>{value}</span>
      </div>
      {sub && <div className="text-[10px] text-neutral-500 mt-0.5">{sub}</div>}
      {children}
    </div>
  );
}

function LatencyBar({ p50, p95, p99, unit, maxVal }: {
  p50: number; p95: number; p99: number; unit: string; maxVal: number;
}) {
  const scale = (v: number) => Math.min(100, (v / maxVal) * 100);
  return (
    <div className="mt-2 space-y-1">
      {[
        { label: 'p50', value: p50, color: 'bg-green-500/60' },
        { label: 'p95', value: p95, color: 'bg-amber-500/60' },
        { label: 'p99', value: p99, color: 'bg-red-500/60' },
      ].map(({ label, value, color }) => (
        <div key={label} className="flex items-center gap-1.5">
          <span className="text-[9px] text-neutral-600 w-6 text-right font-mono">{label}</span>
          <div className="flex-1 h-1 bg-neutral-800 rounded-full overflow-hidden">
            <div className={cn('h-full rounded-full', color)} style={{ width: `${scale(value)}%` }} />
          </div>
          <span className="text-[9px] text-neutral-500 w-14 text-right font-mono">
            {unit === 'µs' ? formatMicros(value) : formatDuration(value)}
          </span>
        </div>
      ))}
    </div>
  );
}

function ConsumerBar({ label, value, total, color }: {
  label: string; value: number; total: number; color: string;
}) {
  const pct = total > 0 ? Math.min(100, (value / total) * 100) : 0;
  return (
    <div className="flex items-center gap-2">
      <span className="text-[10px] text-neutral-500 w-20 shrink-0">{label}</span>
      <div className="flex-1 h-1.5 bg-neutral-800 rounded-full overflow-hidden">
        <div className={cn('h-full rounded-full', color)} style={{ width: `${pct}%` }} />
      </div>
      <span className="text-[10px] text-neutral-400 w-10 text-right font-mono">{value}</span>
    </div>
  );
}

function TierBar({ t1, t2, t3 }: { t1: number; t2: number; t3: number }) {
  const total = t1 + t2 + t3;
  if (total === 0) return <div className="text-[10px] text-neutral-600">暂无决策记录</div>;
  const pct1 = (t1 / total * 100).toFixed(0);
  const pct2 = (t2 / total * 100).toFixed(0);
  const pct3 = (t3 / total * 100).toFixed(0);

  return (
    <div>
      <div className="flex h-2.5 rounded-full overflow-hidden">
        {t1 > 0 && <div className="bg-green-500/70 transition-all" style={{ width: `${pct1}%` }} />}
        {t2 > 0 && <div className="bg-blue-500/70 transition-all" style={{ width: `${pct2}%` }} />}
        {t3 > 0 && <div className="bg-purple-500/70 transition-all" style={{ width: `${pct3}%` }} />}
      </div>
      <div className="flex items-center gap-3 mt-1.5 text-[10px]">
        <span className="text-green-400">T1 {pct1}% <span className="text-neutral-600">({t1})</span></span>
        <span className="text-blue-400">T2 {pct2}% <span className="text-neutral-600">({t2})</span></span>
        <span className="text-purple-400">T3 {pct3}% <span className="text-neutral-600">({t3})</span></span>
      </div>
    </div>
  );
}

// ── Main Component ─────────────────────────────────────────

export function EngineDashboard() {
  const wsConnected = useConnectionState() === 'connected';
  const healthSnapshot = useEventStreamStore((s) => s.healthSnapshot);
  const engineVersion = useEventInvalidation('engine');

  const [health, setHealth] = useState<HealthData | null>(null);
  const [llmData, setLlmData] = useState<LlmTracesResponse | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [refreshInterval, setRefreshInterval] = useState(() => {
    if (typeof window === 'undefined') return 5000;
    return Number(localStorage.getItem('engine:refresh')) || 5000;
  });
  const [expandedTraceId, setExpandedTraceId] = useState<string | null>(null);
  const [traceFilter, setTraceFilter] = useState<string>('all');

  useEffect(() => {
    localStorage.setItem('engine:refresh', String(refreshInterval));
  }, [refreshInterval]);

  // When WS pushes healthSnapshot, merge it into local health state
  useEffect(() => {
    if (!healthSnapshot) return;
    setHealth((prev) => {
      // Merge: WS snapshot has stats + event_bus + memory but not pty/uptime/gemini_mode.
      // Keep prev fields for what WS doesn't push.
      const snap = healthSnapshot as Record<string, unknown>;
      return {
        status: (prev?.status || 'ok') as string,
        pty: (prev?.pty || []) as HealthData['pty'],
        uptime_epoch: (prev?.uptime_epoch || 0) as number,
        gemini_mode: (prev?.gemini_mode || 'http') as string,
        event_bus: (snap.event_bus || prev?.event_bus || { publish_count: 0 }) as HealthData['event_bus'],
        memory: snap.memory as HealthData['memory'],
        stats: (snap.stats || prev?.stats) as HealthData['stats'],
      } as HealthData;
    });
    setLoading(false);
  }, [healthSnapshot]);

  // Fetch full health + LLM traces via HTTP
  const fetchAll = useCallback(async () => {
    try {
      setLoading(true);
      const [healthRes, llmRes] = await Promise.allSettled([
        // Skip health fetch when WS is providing it
        wsConnected ? Promise.resolve(null) : fetch('/api/system/health'),
        fetch('/api/system/llm-traces'),
      ]);
      if (!wsConnected && healthRes.status === 'fulfilled' && healthRes.value && (healthRes.value as Response).ok) {
        const data = await (healthRes.value as Response).json();
        if (!data.error) setHealth(data);
        else setError(data.error);
      }
      if (llmRes.status === 'fulfilled' && llmRes.value && (llmRes.value as Response).ok) {
        setLlmData(await (llmRes.value as Response).json());
      }
      setError(null);
    } catch (err) {
      setError(String(err));
    } finally {
      setLoading(false);
    }
  }, [wsConnected]);

  // Initial fetch + periodic polling (health polling only when WS disconnected)
  useEffect(() => {
    fetchAll();
    if (refreshInterval > 0) {
      const id = setInterval(fetchAll, refreshInterval);
      return () => clearInterval(id);
    }
  }, [fetchAll, refreshInterval]);

  // Refetch LLM traces when engine events arrive (WS-driven invalidation)
  useEffect(() => {
    if (engineVersion === 0) return; // skip initial
    // Only refetch LLM traces — health comes from WS push
    fetch('/api/system/llm-traces')
      .then((r) => r.ok ? r.json() : null)
      .then((data) => { if (data) setLlmData(data); })
      .catch(() => {});
  }, [engineVersion]);

  // Derived
  const stats = health?.stats;

  const systemHealth = useMemo(() => {
    if (!stats) return 'unknown';
    const hasError = stats.events.estimated_backlog > 100
      || stats.events.lagged.total_skipped > 10;
    const hasWarn = stats.events.estimated_backlog > 10
      || stats.events.lagged.total_skipped > 0
      || stats.gemini.errors > 0;
    if (hasError) return 'error';
    if (hasWarn) return 'warning';
    return 'healthy';
  }, [stats]);

  const healthDot: Record<string, { color: string; bg: string; label: string }> = {
    healthy: { color: 'text-green-400', bg: 'bg-green-500', label: 'Healthy' },
    warning: { color: 'text-amber-400', bg: 'bg-amber-500', label: 'Warning' },
    error: { color: 'text-red-400', bg: 'bg-red-500', label: 'Error' },
    unknown: { color: 'text-neutral-500', bg: 'bg-neutral-600', label: '...' },
  };
  const hd = healthDot[systemHealth];

  const filteredTraces = useMemo(() => {
    const rows = llmData?.traces?.requests || [];
    if (traceFilter === 'all') return rows;
    if (traceFilter === 'error') return rows.filter(r => r.status !== 'ok');
    return rows.filter(r => r.status === 'ok');
  }, [llmData, traceFilter]);

  const traceFilterCounts = useMemo(() => {
    const rows = llmData?.traces?.requests || [];
    return {
      total: rows.length,
      ok: rows.filter(r => r.status === 'ok').length,
      error: rows.filter(r => r.status !== 'ok').length,
    };
  }, [llmData]);

  const totalConsumed = stats
    ? stats.events.consumed.extraction + stats.events.consumed.submit + stats.events.consumed.decision
    : 0;

  const errorRate = stats && stats.gemini.requests > 0
    ? (stats.gemini.errors / stats.gemini.requests * 100)
    : 0;

  // Loading/Error
  if (loading && !health) {
    return (
      <div className="flex-1 flex items-center justify-center">
        <Loader2 className="w-5 h-5 animate-spin text-neutral-600" />
      </div>
    );
  }

  if (error && !health) {
    return (
      <div className="flex-1 flex items-center justify-center text-sm text-red-400">
        <AlertTriangle className="w-4 h-4 mr-2" />
        {error}
      </div>
    );
  }

  if (!health || !stats) return null;

  return (
    <div className="flex-1 overflow-auto px-4 sm:px-8 pb-8 max-w-6xl space-y-4 pt-2">
      {/* ── Header ── */}
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-3">
          <div className="flex items-center gap-2">
            <div className={cn('w-2 h-2 rounded-full', hd.bg)} />
            <span className="text-sm font-medium text-neutral-300">核心引擎</span>
          </div>
          <Badge variant="outline" className={cn('text-[10px] px-1.5 py-0 border-0', hd.color, `${hd.bg}/10`)}>
            {hd.label}
          </Badge>
          <Badge variant="outline" className="text-[10px] px-1.5 py-0 border-0 text-neutral-500 bg-neutral-500/10">
            {health.gemini_mode === 'cli' ? 'Gemini CLI' : 'Gemini HTTP'}
          </Badge>
          {wsConnected && (
            <Badge variant="outline" className="text-[10px] px-1.5 py-0 border-0 text-emerald-500 bg-emerald-500/10">
              WS Push
            </Badge>
          )}
        </div>
        <div className="flex items-center gap-2">
          <div className="flex items-center gap-0.5 bg-neutral-900 rounded px-1 py-0.5">
            <Timer className="w-3 h-3 text-neutral-600 mr-0.5" />
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
          <button onClick={fetchAll} className="p-1 text-neutral-600 hover:text-neutral-300 transition-colors">
            <RefreshCw className={cn('w-3.5 h-3.5', loading && 'animate-spin')} />
          </button>
        </div>
      </div>

      {/* ── L0: 4 Core Metric Cards ── */}
      <div className="grid grid-cols-4 gap-3">
        {/* EventBus */}
        <MetricCard
          icon={<Zap className="w-3.5 h-3.5 text-yellow-400" />}
          title="EventBus"
          value={formatNumber(stats.events.published)}
          sub="事件已发布"
          color={thresholdColor(stats.events.estimated_backlog, 10, 100)}
          borderColor={thresholdBg(stats.events.estimated_backlog, 10, 100)}
        >
          <div className="mt-2 space-y-1 text-[10px]">
            <div className="flex justify-between">
              <span className="text-neutral-500">积压</span>
              <span className={thresholdColor(stats.events.estimated_backlog, 10, 100)}>
                {stats.events.estimated_backlog}
              </span>
            </div>
            <div className="flex justify-between">
              <span className="text-neutral-500">滞后</span>
              <span className={thresholdColor(stats.events.lagged.total_skipped, 0, 10)}>
                {stats.events.lagged.total_skipped} skipped
              </span>
            </div>
          </div>
        </MetricCard>

        {/* DB Executor */}
        <MetricCard
          icon={<Database className="w-3.5 h-3.5 text-cyan-400" />}
          title="DB Exec"
          value={formatMicros(stats.db_exec.avg_us)}
          sub={`${formatNumber(stats.db_exec.runs)} 次执行`}
          color={thresholdColor(stats.db_exec.p95_us, 500, 2000)}
          borderColor={thresholdBg(stats.db_exec.p95_us, 500, 2000)}
        >
          <LatencyBar
            p50={stats.db_exec.p50_us}
            p95={stats.db_exec.p95_us}
            p99={stats.db_exec.p99_us}
            unit="µs"
            maxVal={Math.max(stats.db_exec.p99_us * 1.2, 1000)}
          />
        </MetricCard>

        {/* LLM Gateway */}
        <MetricCard
          icon={<Brain className="w-3.5 h-3.5 text-purple-400" />}
          title="LLM 网关"
          value={stats.gemini.requests}
          sub={`${health.gemini_mode} 模式`}
          color={thresholdColor(errorRate, 2, 5)}
          borderColor={thresholdBg(errorRate, 2, 5)}
        >
          <div className="mt-2 space-y-1 text-[10px]">
            <div className="flex justify-between">
              <span className="text-neutral-500">错误</span>
              <span className={stats.gemini.errors > 0 ? 'text-red-400' : 'text-neutral-600'}>
                {stats.gemini.errors}
              </span>
            </div>
            <div className="flex justify-between">
              <span className="text-neutral-500">重试</span>
              <span className={stats.gemini.retries > 0 ? 'text-amber-400' : 'text-neutral-600'}>
                {stats.gemini.retries}
              </span>
            </div>
          </div>
          <LatencyBar
            p50={stats.gemini.p50_ms}
            p95={stats.gemini.p95_ms}
            p99={stats.gemini.p99_ms}
            unit="ms"
            maxVal={Math.max(stats.gemini.p99_ms * 1.2, 5000)}
          />
        </MetricCard>

        {/* Autopilot */}
        <MetricCard
          icon={<Settings2 className="w-3.5 h-3.5 text-orange-400" />}
          title="Autopilot"
          value={formatDuration(stats.autopilot.avg_ms)}
          sub={`${stats.autopilot.ticks} ticks`}
          color={thresholdColor(stats.autopilot.p95_ms, 200, 1000)}
          borderColor={thresholdBg(stats.autopilot.p95_ms, 200, 1000)}
        >
          <LatencyBar
            p50={stats.autopilot.p50_ms}
            p95={stats.autopilot.p95_ms}
            p99={stats.autopilot.p99_ms}
            unit="ms"
            maxVal={Math.max(stats.autopilot.p99_ms * 1.2, 500)}
          />
        </MetricCard>
      </div>

      {/* ── L1: Two-column detail panels ── */}
      <div className="grid grid-cols-2 gap-3">
        {/* Left: Event Pipeline Analysis */}
        <div className="rounded-lg border border-neutral-800 bg-neutral-900/50 p-3">
          <div className="text-xs font-medium text-neutral-400 mb-3 flex items-center gap-1.5">
            <Zap className="w-3 h-3 text-yellow-400" />
            事件管道分析
          </div>

          <div className="flex items-center gap-2 mb-3 text-xs">
            <span className="text-neutral-300 font-semibold">{formatNumber(stats.events.published)}</span>
            <span className="text-neutral-600">发布</span>
            <ArrowRight className="w-3 h-3 text-neutral-700" />
            <span className="text-neutral-300 font-semibold">{formatNumber(totalConsumed)}</span>
            <span className="text-neutral-600">消费</span>
            {stats.events.estimated_backlog > 0 && (
              <>
                <ArrowRight className="w-3 h-3 text-neutral-700" />
                <span className={cn('font-semibold', thresholdColor(stats.events.estimated_backlog, 10, 100))}>
                  {stats.events.estimated_backlog}
                </span>
                <span className="text-neutral-600">积压</span>
              </>
            )}
          </div>

          <div className="space-y-1.5 mb-3">
            <ConsumerBar label="Extraction" value={stats.events.consumed.extraction} total={totalConsumed} color="bg-yellow-500/60" />
            <ConsumerBar label="Submit" value={stats.events.consumed.submit} total={totalConsumed} color="bg-blue-500/60" />
            <ConsumerBar label="Decision" value={stats.events.consumed.decision} total={totalConsumed} color="bg-purple-500/60" />
          </div>

          {(stats.events.lagged.extraction > 0 || stats.events.lagged.submit > 0 || stats.events.lagged.decision > 0) && (
            <div className="pt-2 border-t border-neutral-800">
              <div className="text-[10px] text-neutral-500 mb-1">滞后计数</div>
              <div className="flex gap-3 text-[10px]">
                {stats.events.lagged.extraction > 0 && (
                  <span className="text-amber-400">Extraction: {stats.events.lagged.extraction}</span>
                )}
                {stats.events.lagged.submit > 0 && (
                  <span className="text-amber-400">Submit: {stats.events.lagged.submit}</span>
                )}
                {stats.events.lagged.decision > 0 && (
                  <span className="text-amber-400">Decision: {stats.events.lagged.decision}</span>
                )}
              </div>
            </div>
          )}
        </div>

        {/* Right: Decision & Task Throughput */}
        <div className="rounded-lg border border-neutral-800 bg-neutral-900/50 p-3">
          <div className="text-xs font-medium text-neutral-400 mb-3 flex items-center gap-1.5">
            <Brain className="w-3 h-3 text-purple-400" />
            决策引擎 & 任务吞吐
          </div>

          {/* Tasks */}
          <div className="flex items-center gap-3 mb-3 text-xs">
            <div className="flex items-center gap-1">
              <span className="text-neutral-500">分发</span>
              <span className="text-neutral-300 font-semibold">{stats.tasks.dispatched}</span>
            </div>
            <ArrowRight className="w-3 h-3 text-neutral-700" />
            <div className="flex items-center gap-1">
              <span className="text-neutral-500">完成</span>
              <span className="text-green-400 font-semibold">{stats.tasks.completed}</span>
            </div>
            {stats.tasks.dispatched > 0 && (
              <span className="text-[10px] text-neutral-600">
                ({(stats.tasks.completed / stats.tasks.dispatched * 100).toFixed(0)}%)
              </span>
            )}
          </div>

          {/* Tier distribution */}
          <div className="mb-3">
            <div className="text-[10px] text-neutral-500 mb-1.5">决策层级命中</div>
            <TierBar
              t1={stats.decisions.tier1_hit}
              t2={stats.decisions.tier2_hit}
              t3={stats.decisions.tier3_dispatched}
            />
          </div>

          {/* 7-day Gemini summary */}
          {llmData?.stats && (
            <div className="pt-2 border-t border-neutral-800">
              <div className="text-[10px] text-neutral-500 mb-1.5">7 天 LLM 汇总</div>
              <div className="grid grid-cols-3 gap-2 text-[10px]">
                <div>
                  <span className="text-neutral-500">请求</span>
                  <span className="text-neutral-300 ml-1 font-semibold">{llmData.stats.total}</span>
                </div>
                <div>
                  <span className="text-neutral-500">错误</span>
                  <span className={cn('ml-1 font-semibold', llmData.stats.errors > 0 ? 'text-red-400' : 'text-neutral-600')}>
                    {llmData.stats.errors}
                  </span>
                </div>
                <div>
                  <span className="text-neutral-500">平均</span>
                  <span className="text-neutral-300 ml-1 font-semibold">
                    {formatDuration(llmData.stats.avg_duration_ms)}
                  </span>
                </div>
              </div>
              {llmData.stats.by_caller && llmData.stats.by_caller.length > 0 && (
                <div className="mt-1.5 space-y-0.5">
                  {llmData.stats.by_caller.slice(0, 5).map((c) => (
                    <div key={c.caller} className="flex items-center justify-between text-[9px]">
                      <span className="text-neutral-500 font-mono">{c.caller}</span>
                      <span className="text-neutral-400">
                        {c.count}次 · {formatDuration(c.avg_duration_ms)}
                        {c.errors > 0 && <span className="text-red-400 ml-1">({c.errors} err)</span>}
                      </span>
                    </div>
                  ))}
                </div>
              )}
            </div>
          )}
        </div>
      </div>

      {/* ── L2: LLM Trace Console ── */}
      <div className="rounded-lg border border-neutral-800 overflow-hidden">
        <div className="px-3 py-2 bg-neutral-900/80 border-b border-neutral-800 flex items-center justify-between">
          <span className="text-xs font-medium text-neutral-400 flex items-center gap-1.5">
            <Filter className="w-3 h-3" />
            LLM 调用链路
            {llmData?.traces && (
              <Badge variant="outline" className="text-[10px] px-1.5 py-0 border-0 text-neutral-500 bg-neutral-500/10">
                {llmData.traces.count}
              </Badge>
            )}
          </span>
          <div className="flex items-center gap-1">
            {[
              { key: 'all', label: 'All', count: traceFilterCounts.total },
              { key: 'ok', label: 'OK', count: traceFilterCounts.ok },
              { key: 'error', label: 'Error', count: traceFilterCounts.error },
            ].filter(f => f.key === 'all' || f.count > 0).map(f => (
              <button
                key={f.key}
                onClick={() => setTraceFilter(f.key)}
                className={cn(
                  'px-2 py-0.5 text-[10px] rounded-full transition-colors',
                  traceFilter === f.key
                    ? f.key === 'error' ? 'bg-red-500/20 text-red-400'
                    : 'bg-neutral-700 text-neutral-200'
                    : 'text-neutral-500 hover:text-neutral-300 hover:bg-neutral-800',
                )}
              >
                {f.label} {f.count > 0 && f.key !== 'all' ? `(${f.count})` : ''}
              </button>
            ))}
          </div>
        </div>

        <div className="divide-y divide-neutral-800/50 max-h-[400px] overflow-y-auto">
          {filteredTraces.length === 0 ? (
            <div className="px-3 py-8 text-center text-xs text-neutral-600">暂无 LLM 调用记录</div>
          ) : (
            filteredTraces.map((trace) => {
              const st = TRACE_STATUS[trace.status] || { color: 'text-neutral-400', bg: 'bg-neutral-500/10', label: trace.status };
              const isExpanded = expandedTraceId === trace.id;
              return (
                <div key={trace.id}>
                  <div
                    className={cn(
                      'flex items-center gap-3 px-3 py-2 text-xs hover:bg-neutral-900/50 cursor-pointer',
                      isExpanded && 'bg-neutral-900/50',
                    )}
                    onClick={() => setExpandedTraceId(isExpanded ? null : trace.id)}
                  >
                    <span className="text-neutral-500 w-10 shrink-0 font-mono">{timeAgo(trace.created_at)}</span>
                    <span className="text-neutral-400 w-20 shrink-0 font-mono truncate">{trace.caller}</span>
                    <span className="text-neutral-300 w-36 shrink-0 font-mono truncate" title={trace.model}>
                      {trace.model}
                    </span>
                    <span className={cn('w-16 text-right shrink-0 font-mono', trace.duration_ms > 10000 ? 'text-amber-400' : 'text-neutral-400')}>
                      {formatDuration(trace.duration_ms)}
                    </span>
                    <Badge variant="outline" className={cn('text-[10px] px-1.5 py-0 border-0 shrink-0', st.color, st.bg)}>
                      {st.label}
                    </Badge>
                    {trace.retry_count > 0 && (
                      <span className="text-amber-400 text-[10px] shrink-0 flex items-center gap-0.5">
                        <RotateCcw className="w-2.5 h-2.5" />{trace.retry_count}
                      </span>
                    )}
                    <ChevronDown className={cn(
                      'w-3 h-3 text-neutral-600 shrink-0 transition-transform ml-auto',
                      !isExpanded && '-rotate-90',
                    )} />
                  </div>

                  {isExpanded && (
                    <div className="px-3 py-2 bg-neutral-950/50 border-t border-neutral-800/50">
                      <div className="grid grid-cols-2 gap-x-6 gap-y-1 text-[10px]">
                        <div className="flex justify-between">
                          <span className="text-neutral-500">ID</span>
                          <span className="text-neutral-400 font-mono">{shortId(trace.id)}</span>
                        </div>
                        <div className="flex justify-between">
                          <span className="text-neutral-500">模式</span>
                          <span className="text-neutral-400">{trace.api_mode}</span>
                        </div>
                        {trace.session_id && (
                          <div className="flex justify-between">
                            <span className="text-neutral-500">Session</span>
                            <span className="text-neutral-400 font-mono">{shortId(trace.session_id)}</span>
                          </div>
                        )}
                        <div className="flex justify-between">
                          <span className="text-neutral-500">队列等待</span>
                          <span className="text-neutral-400">{formatDuration(trace.queue_wait_ms)}</span>
                        </div>
                        <div className="flex justify-between">
                          <span className="text-neutral-500">Prompt</span>
                          <span className="text-neutral-400">{formatChars(trace.prompt_chars)} chars</span>
                        </div>
                        <div className="flex justify-between">
                          <span className="text-neutral-500">Response</span>
                          <span className="text-neutral-400">{formatChars(trace.response_chars)} chars</span>
                        </div>
                        <div className="flex justify-between">
                          <span className="text-neutral-500">时间</span>
                          <span className="text-neutral-400">{new Date(trace.created_at).toLocaleString('zh-CN')}</span>
                        </div>
                      </div>
                      {trace.error_msg && (
                        <div className="mt-2 p-2 rounded bg-red-500/5 border border-red-500/20">
                          <span className="text-[10px] text-red-400 font-medium">Error: </span>
                          <span className="text-[10px] text-red-300">{trace.error_msg}</span>
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
