'use client';

import { useState, useEffect, useCallback, useMemo } from 'react';
import {
  Zap, Brain, Search, AlertCircle, AlertTriangle, BarChart3,
  ChevronDown, ChevronRight, Send, X, Skull,
  Crosshair, BookOpen, Clock
} from 'lucide-react';
import { cn } from '@/lib/utils';
import { Badge } from '@/components/ui/badge';
import type { AgentQuestion, RoutingTrace, TierStep, DecisionStats } from '@/types';
import * as questionsApi from '@/questionsApi';

// ── Helpers ──────────────────────────────────────────────────

function timeAgo(iso: string): string {
  const diff = Date.now() - new Date(iso).getTime();
  const s = Math.floor(diff / 1000);
  if (s < 60) return `${s}s`;
  const m = Math.floor(s / 60);
  if (m < 60) return `${m}m`;
  const h = Math.floor(m / 60);
  if (h < 24) return `${h}h`;
  return `${Math.floor(h / 24)}d`;
}

function parseTrace(q: AgentQuestion): RoutingTrace | null {
  if (!q.routingTrace) return null;
  try {
    return JSON.parse(q.routingTrace);
  } catch {
    return null;
  }
}

function resolvedTier(q: AgentQuestion): string {
  const trace = parseTrace(q);
  if (trace) return trace.resolved_tier;
  if (q.status === 'pending') return 'pending';
  // Fallback: parse answer prefix
  if (q.answer?.startsWith('[主控裁决·肌肉记忆]')) return 'T1';
  if (q.answer?.startsWith('[主控裁决·Gemini]')) return 'T2';
  if (q.answer?.startsWith('[主控裁决·工位]')) return 'T3';
  if (q.target === 'user' && q.decisionType && q.decisionType !== 'implementation') return 'downgraded';
  return q.status === 'answered' ? 'unknown' : 'pending';
}

const TIER_COLORS: Record<string, { dot: string; bg: string; text: string; label: string }> = {
  T1: { dot: 'bg-green-400', bg: 'bg-green-500/5 border-green-500/20', text: 'text-green-400', label: 'T1·KB' },
  T2: { dot: 'bg-blue-400', bg: 'bg-blue-500/5 border-blue-500/20', text: 'text-blue-400', label: 'T2·Gemini' },
  T3: { dot: 'bg-purple-400', bg: 'bg-purple-500/5 border-purple-500/20', text: 'text-purple-400', label: 'T3·工位' },
  'T2-escalated': { dot: 'bg-blue-400', bg: 'bg-blue-500/5 border-blue-500/20', text: 'text-blue-400', label: 'T2→弃权' },
  downgraded: { dot: 'bg-red-400', bg: 'bg-red-500/5 border-red-500/20', text: 'text-red-400', label: '降级·人工' },
  pending: { dot: 'bg-amber-400 animate-pulse', bg: 'bg-amber-500/5 border-amber-500/20', text: 'text-amber-400', label: '处理中...' },
  error: { dot: 'bg-red-600', bg: 'bg-red-500/5 border-red-500/20', text: 'text-red-400', label: '错误' },
  unknown: { dot: 'bg-neutral-500', bg: 'bg-neutral-800 border-neutral-700', text: 'text-neutral-400', label: '未知' },
};

function tierStyle(tier: string) {
  return TIER_COLORS[tier] || TIER_COLORS.unknown;
}

const DT_LABELS: Record<string, string> = {
  architecture: '架构',
  risk: '风险',
  debug: '调试',
  investigation: '调查',
  preference: '偏好',
  implementation: '实施',
};

// ── KB Entry type (lightweight) ──────────────────────────────

interface KBEntry {
  key: string;
  category: string;
  summary: string;
  detail?: string;
  confidence?: number;
  accessCount?: number;
  createdAt?: string;
}

// ── Sub-components ───────────────────────────────────────────

function StatCard({ label, value, icon: Icon, color }: {
  label: string; value: string | number | undefined; icon: React.ComponentType<{ className?: string }>; color: string;
}) {
  return (
    <div className="flex items-center gap-2 px-3 py-1.5">
      <Icon className={cn('w-3.5 h-3.5', color)} />
      <div className="flex flex-col">
        <span className="text-[10px] text-neutral-500 leading-tight">{label}</span>
        <span className={cn('text-sm font-semibold leading-tight', color)}>{value ?? '-'}</span>
      </div>
    </div>
  );
}

function TelemetryBar({ stats }: { stats: DecisionStats | null }) {
  if (!stats) return null;
  return (
    <div className="flex items-center gap-1 bg-neutral-900 rounded-lg border border-neutral-800 mb-4 overflow-x-auto">
      <StatCard label="T1 命中率" value={stats.t1HitRate} icon={Zap} color="text-green-400" />
      <div className="w-px h-8 bg-neutral-800" />
      <StatCard label="T1" value={stats.t1Hits} icon={Zap} color="text-green-400" />
      <StatCard label="T2" value={stats.t2Hits} icon={Brain} color="text-blue-400" />
      <StatCard label="T3" value={stats.t3Hits} icon={Search} color="text-purple-400" />
      <div className="w-px h-8 bg-neutral-800" />
      <StatCard label="待处理" value={stats.pending} icon={AlertCircle} color={stats.pending > 0 ? 'text-amber-400' : 'text-neutral-500'} />
      <StatCard label="降级" value={stats.downgraded} icon={AlertTriangle} color={stats.downgraded > 0 ? 'text-red-400' : 'text-neutral-500'} />
      <StatCard label={`${stats.hours}h 总计`} value={stats.total} icon={BarChart3} color="text-neutral-400" />
    </div>
  );
}

function LiveRadar({ questions, selectedId, onSelect }: {
  questions: AgentQuestion[]; selectedId: string | null; onSelect: (id: string) => void;
}) {
  return (
    <div className="flex flex-col h-full">
      <div className="px-2 py-1.5 text-[10px] text-neutral-500 font-medium uppercase tracking-wider border-b border-neutral-800">
        决策雷达 ({questions.length})
      </div>
      <div className="flex-1 overflow-y-auto space-y-1 p-1.5">
        {questions.length === 0 && (
          <div className="text-xs text-neutral-600 text-center py-8">暂无决策记录</div>
        )}
        {questions.map(q => {
          const tier = resolvedTier(q);
          const style = tierStyle(tier);
          const trace = parseTrace(q);
          return (
            <button
              key={q.id}
              onClick={() => onSelect(q.id)}
              className={cn(
                'w-full text-left p-2 rounded-lg border transition-colors',
                selectedId === q.id
                  ? 'border-orange-500/40 bg-orange-500/5'
                  : 'border-neutral-800/50 hover:border-neutral-700 bg-neutral-900/30',
              )}
            >
              <div className="flex items-center gap-1.5">
                <div className={cn('w-2 h-2 rounded-full shrink-0', style.dot)} />
                <span className={cn('text-[10px] font-medium', style.text)}>{style.label}</span>
                {q.decisionType && (
                  <Badge variant="outline" className="text-[9px] px-1 py-0 h-3.5 border-neutral-700 text-neutral-500">
                    {DT_LABELS[q.decisionType] || q.decisionType}
                  </Badge>
                )}
                <span className="text-[10px] text-neutral-600 ml-auto shrink-0">{timeAgo(q.createdAt)}</span>
              </div>
              <p className="text-[11px] text-neutral-300 mt-1 line-clamp-2 leading-snug">{q.question}</p>
              {trace && (
                <span className="text-[9px] text-neutral-600 mt-0.5 block">{trace.latency_ms}ms</span>
              )}
            </button>
          );
        })}
      </div>
    </div>
  );
}

function ScoreBar({ label, value, threshold, color }: {
  label: string; value: number; threshold?: number; color: string;
}) {
  const pct = Math.min(100, value * 100);
  const passed = threshold ? value >= threshold : true;
  return (
    <div className="flex items-center gap-2">
      <span className="text-[9px] text-neutral-500 w-8 shrink-0 text-right font-mono">{label}</span>
      <div className="flex-1 h-1.5 bg-neutral-800 rounded-full overflow-hidden relative">
        <div className={cn('h-full rounded-full transition-all', color)} style={{ width: `${pct}%` }} />
        {threshold && (
          <div className="absolute top-0 h-full w-px bg-neutral-600" style={{ left: `${threshold * 100}%` }} />
        )}
      </div>
      <span className={cn('text-[9px] font-mono w-10 shrink-0', passed ? 'text-emerald-500' : 'text-red-400')}>
        {value.toFixed(3)}
      </span>
    </div>
  );
}

function HybridSearchPanel({ step }: { step: TierStep }) {
  const cosine = step.semantic_similarity ? parseFloat(step.semantic_similarity) : null;
  const rrf = step.rrf_score ? parseFloat(step.rrf_score) : null;

  return (
    <div className="mt-2 p-2 rounded bg-neutral-800/50 border border-neutral-700/50 space-y-1.5">
      <div className="flex items-center gap-1.5 mb-1">
        <span className="text-[9px] text-neutral-500 font-medium uppercase">混合检索透视</span>
        {step.method && (
          <Badge variant="outline" className={cn('text-[8px] px-1 py-0 h-3',
            step.method === 'hybrid' ? 'border-emerald-700 text-emerald-600' : 'border-neutral-700 text-neutral-600'
          )}>
            {step.method === 'hybrid' ? '混合搜索' : 'FTS5'}
          </Badge>
        )}
      </div>
      {cosine != null && (
        <ScoreBar label="cos" value={cosine} threshold={0.6} color="bg-emerald-500/70" />
      )}
      {rrf != null && (
        <ScoreBar label="rrf" value={Math.min(1, rrf * 20)} color="bg-amber-500/70" />
      )}
      <div className="flex items-center gap-3 text-[9px] mt-1">
        {step.fts_rank != null && (
          <span className="text-blue-500 font-mono">FTS5 #{step.fts_rank + 1}</span>
        )}
        {step.vec_rank != null && (
          <span className="text-purple-500 font-mono">Vec #{step.vec_rank + 1}</span>
        )}
      </div>
    </div>
  );
}

function RoutingTraceView({ trace }: { trace: RoutingTrace }) {
  return (
    <div className="p-3 rounded-lg bg-neutral-900 border border-neutral-800">
      <div className="flex items-center justify-between mb-2">
        <span className="text-[10px] text-neutral-500 font-medium uppercase tracking-wider">路由轨迹</span>
        <span className="text-[10px] text-neutral-600 font-mono">{trace.latency_ms}ms</span>
      </div>
      <div className="space-y-1.5">
        {trace.path.map((step, i) => {
          const style = tierStyle(step.tier);
          const hasHybrid = step.semantic_similarity || step.rrf_score;
          return (
            <div key={i} className="flex items-start gap-2">
              <div className="flex flex-col items-center mt-0.5">
                <div className={cn('w-2 h-2 rounded-full shrink-0', style.dot)} />
                {i < trace.path.length - 1 && <div className="w-px h-4 bg-neutral-700 mt-0.5" />}
              </div>
              <div className="min-w-0 flex-1">
                <div className="flex items-center gap-1.5">
                  <span className={cn('text-[11px] font-medium', style.text)}>{step.tier}</span>
                  <span className="text-[10px] text-neutral-500">{step.status}</span>
                  {step.coverage && <span className="text-[9px] text-neutral-600 font-mono">{step.coverage}</span>}
                </div>
                {step.reason && <p className="text-[10px] text-neutral-600 truncate">{step.reason}</p>}
                {step.reasoning && (
                  <div className="mt-1 p-2 rounded bg-blue-500/5 border border-blue-500/10">
                    <span className="text-[9px] text-blue-400/60 block mb-0.5">Reasoning</span>
                    <p className="text-[10px] text-neutral-400 line-clamp-4">{step.reasoning}</p>
                  </div>
                )}
                {step.rule_key && <span className="text-[9px] text-neutral-600 font-mono">rule: {step.rule_key}</span>}
                {hasHybrid && <HybridSearchPanel step={step} />}
              </div>
            </div>
          );
        })}
      </div>
    </div>
  );
}

function DecisionXRay({ question, onAnswer, onDismiss, onKillT3 }: {
  question: AgentQuestion | null;
  onAnswer: (id: string, answer: string) => void;
  onDismiss: (id: string) => void;
  onKillT3: () => void;
}) {
  const [answerText, setAnswerText] = useState('');

  if (!question) {
    return (
      <div className="flex items-center justify-center h-full text-neutral-600 text-xs">
        <div className="text-center">
          <Crosshair className="w-8 h-8 mx-auto mb-2 text-neutral-700" />
          <p>点击左侧条目查看详情</p>
        </div>
      </div>
    );
  }

  const tier = resolvedTier(question);
  const style = tierStyle(tier);
  const trace = parseTrace(question);
  const isPending = question.status === 'pending';
  const isDowngraded = tier === 'downgraded';

  let parsedOptions: string[] = [];
  if (question.options) {
    try { parsedOptions = JSON.parse(question.options); } catch { /* ignore */ }
  }

  return (
    <div className="flex flex-col h-full">
      <div className="px-2 py-1.5 text-[10px] text-neutral-500 font-medium uppercase tracking-wider border-b border-neutral-800">
        决策 X 光
      </div>
      <div className="flex-1 overflow-y-auto p-3 space-y-3">
        {/* Header */}
        <div>
          <div className="flex items-center gap-1.5 flex-wrap mb-2">
            <div className={cn('px-1.5 py-0.5 rounded text-[10px] font-medium border', style.bg, style.text)}>
              {style.label}
            </div>
            <Badge variant="outline" className="text-[9px] px-1 py-0 h-4 border-neutral-700 text-neutral-500">
              {DT_LABELS[question.decisionType] || question.decisionType}
            </Badge>
            {question.slotId && (
              <span className="text-[9px] font-mono text-neutral-600 bg-neutral-800 px-1 py-0.5 rounded">{question.slotId}</span>
            )}
            {question.retryCount > 0 && (
              <span className="text-[9px] text-red-400">retry: {question.retryCount}/3</span>
            )}
          </div>
          <p className="text-sm text-neutral-200 leading-relaxed">{question.question}</p>
        </div>

        {/* Context */}
        {question.context && (
          <div className="p-2.5 rounded bg-neutral-900/50 border border-neutral-800">
            <span className="text-[10px] text-neutral-500 block mb-1">上下文</span>
            <p className="text-[11px] text-neutral-400 whitespace-pre-wrap leading-relaxed max-h-32 overflow-y-auto">{question.context}</p>
          </div>
        )}

        {/* Options */}
        {parsedOptions.length > 0 && (
          <div className="p-2.5 rounded bg-neutral-900/50 border border-neutral-800">
            <span className="text-[10px] text-neutral-500 block mb-1">候选选项</span>
            <div className="space-y-1">
              {parsedOptions.map((opt, i) => (
                <div key={i} className="flex items-start gap-1.5">
                  <span className="text-[10px] text-neutral-600 font-mono shrink-0">{String.fromCharCode(65 + i)}.</span>
                  <span className="text-[11px] text-neutral-300">{opt}</span>
                </div>
              ))}
            </div>
          </div>
        )}

        {/* Routing Trace */}
        {trace && <RoutingTraceView trace={trace} />}

        {/* Answer */}
        {question.answer && (
          <div className={cn('p-2.5 rounded border', style.bg)}>
            <span className={cn('text-[10px] font-medium block mb-1', style.text)}>决策结果</span>
            <p className="text-[11px] text-neutral-200 whitespace-pre-wrap leading-relaxed">{question.answer}</p>
          </div>
        )}

        {/* Task Link */}
        {question.taskId && (
          <div className="flex items-center gap-1.5 text-[10px] text-neutral-600">
            <Clock className="w-3 h-3" />
            <span>任务: {question.taskId.slice(0, 8)}...</span>
            <span className="ml-auto">{new Date(question.createdAt).toLocaleString('zh-CN', { hour: '2-digit', minute: '2-digit', second: '2-digit' })}</span>
          </div>
        )}

        {/* Intervention Panel */}
        {(isPending || isDowngraded) && (
          <div className="p-2.5 rounded border border-amber-500/20 bg-amber-500/5 space-y-2">
            <span className="text-[10px] text-amber-400 font-medium">需要人工介入</span>
            <div className="flex gap-1.5">
              <input
                value={answerText}
                onChange={e => setAnswerText(e.target.value)}
                onKeyDown={e => {
                  if (e.key === 'Enter' && answerText.trim()) {
                    onAnswer(question.id, answerText.trim());
                    setAnswerText('');
                  }
                }}
                className="flex-1 px-2 py-1 text-xs bg-neutral-800 border border-neutral-700 rounded text-neutral-200 placeholder:text-neutral-600 outline-none focus:border-amber-500/50"
                placeholder="输入决策..."
              />
              <button
                onClick={() => { if (answerText.trim()) { onAnswer(question.id, answerText.trim()); setAnswerText(''); } }}
                className="p-1.5 text-amber-400 hover:text-amber-300 hover:bg-amber-500/10 rounded"
                title="回答"
              >
                <Send className="w-3.5 h-3.5" />
              </button>
              <button
                onClick={() => onDismiss(question.id)}
                className="p-1.5 text-neutral-600 hover:text-neutral-400 hover:bg-neutral-800 rounded"
                title="忽略"
              >
                <X className="w-3.5 h-3.5" />
              </button>
            </div>
            {tier === 'T3' && question.status === 'pending' && (
              <button
                onClick={onKillT3}
                className="flex items-center gap-1 text-[10px] text-red-500 hover:text-red-400 mt-1"
              >
                <Skull className="w-3 h-3" />
                终止 T3 工位
              </button>
            )}
          </div>
        )}
      </div>
    </div>
  );
}

function EvolutionForge({ rules, expanded, onToggle }: {
  rules: KBEntry[]; expanded: boolean; onToggle: () => void;
}) {
  const sortedRules = useMemo(
    () => [...rules].sort((a, b) => (b.accessCount ?? 0) - (a.accessCount ?? 0)),
    [rules],
  );
  const topHitCount = sortedRules[0]?.accessCount ?? 0;

  return (
    <div className="flex flex-col h-full">
      <button
        onClick={onToggle}
        className="flex items-center gap-1.5 px-2 py-1.5 text-[10px] text-neutral-500 font-medium uppercase tracking-wider border-b border-neutral-800 hover:text-neutral-400 w-full text-left"
      >
        <BookOpen className="w-3 h-3" />
        肌肉记忆 ({rules.length})
        {expanded ? <ChevronDown className="w-3 h-3 ml-auto" /> : <ChevronRight className="w-3 h-3 ml-auto" />}
      </button>
      {expanded && (
        <div className="flex-1 overflow-y-auto space-y-1.5 p-1.5">
          {sortedRules.length === 0 && (
            <div className="text-xs text-neutral-600 text-center py-8">暂无 policy:decision 规则</div>
          )}
          {sortedRules.map(rule => {
            const hits = rule.accessCount ?? 0;
            const isHot = hits >= 10;
            const isTop = hits > 0 && hits === topHitCount;
            return (
              <div
                key={rule.key}
                className={cn(
                  'p-2 rounded-lg border transition-all',
                  isHot
                    ? 'border-amber-500/30 bg-amber-500/5 shadow-[0_0_8px_rgba(245,158,11,0.08)]'
                    : 'border-neutral-800/50 bg-neutral-900/30',
                )}
              >
                <div className="flex items-center gap-1.5 mb-1">
                  <Zap className={cn('w-3 h-3', isHot ? 'text-amber-400' : 'text-green-500/50')} />
                  <span className="text-[10px] font-mono text-neutral-500 truncate flex-1">{rule.key}</span>
                  {isTop && hits > 0 && (
                    <span className="text-[8px] px-1 py-0.5 rounded bg-amber-500/20 text-amber-400 font-semibold uppercase tracking-wide">
                      Hot
                    </span>
                  )}
                  {isHot && !isTop && (
                    <span className="text-[8px] px-1 py-0.5 rounded bg-orange-500/10 text-orange-400/70 font-medium">
                      {hits}x
                    </span>
                  )}
                </div>
                <p className="text-[11px] text-neutral-300 leading-snug line-clamp-3">{rule.summary}</p>
                {hits > 0 && (
                  <div className="flex items-center gap-2 mt-1.5">
                    <div className="flex-1 h-1 bg-neutral-800 rounded-full overflow-hidden">
                      <div
                        className={cn('h-full rounded-full transition-all', isHot ? 'bg-amber-500/60' : 'bg-green-500/40')}
                        style={{ width: `${Math.min(100, (hits / Math.max(topHitCount, 1)) * 100)}%` }}
                      />
                    </div>
                    <span className={cn('text-[9px]', isHot ? 'text-amber-500' : 'text-neutral-600')}>
                      {hits} 次命中
                    </span>
                  </div>
                )}
              </div>
            );
          })}
        </div>
      )}
    </div>
  );
}

// ── Main Component ───────────────────────────────────────────

export function DecisionDashboard() {
  const [questions, setQuestions] = useState<AgentQuestion[]>([]);
  const [stats, setStats] = useState<DecisionStats | null>(null);
  const [rules, setRules] = useState<KBEntry[]>([]);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [forgeExpanded, setForgeExpanded] = useState(true);
  const [filter, setFilter] = useState<'all' | 'pending' | 'answered'>('all');

  const fetchData = useCallback(async () => {
    const [qRes, sRes] = await Promise.allSettled([
      questionsApi.fetchMasterQuestions(filter === 'all' ? undefined : filter, 100),
      questionsApi.fetchDecisionStats(24),
    ]);
    if (qRes.status === 'fulfilled' && Array.isArray(qRes.value)) setQuestions(qRes.value);
    if (sRes.status === 'fulfilled') setStats(sRes.value);
  }, [filter]);

  const fetchRules = useCallback(async () => {
    try {
      const res = await fetch('/api/kb?category=policy:decision');
      if (res.ok) {
        const data = await res.json();
        setRules(Array.isArray(data) ? data : []);
      }
    } catch { /* silent */ }
  }, []);

  useEffect(() => {
    fetchData();
    const id = setInterval(fetchData, 5000);
    return () => clearInterval(id);
  }, [fetchData]);

  useEffect(() => {
    fetchRules();
    const id = setInterval(fetchRules, 30000);
    return () => clearInterval(id);
  }, [fetchRules]);

  const selected = useMemo(
    () => questions.find(q => q.id === selectedId) || null,
    [questions, selectedId],
  );

  const handleAnswer = useCallback(async (id: string, answer: string) => {
    try {
      await questionsApi.answerQuestion(id, answer);
      fetchData();
    } catch { /* silent */ }
  }, [fetchData]);

  const handleDismiss = useCallback(async (id: string) => {
    try {
      await questionsApi.dismissQuestion(id);
      fetchData();
    } catch { /* silent */ }
  }, [fetchData]);

  const handleKillT3 = useCallback(async () => {
    try {
      await fetch('/api/pty/kill', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ slotId: 'slot-decision' }),
      });
    } catch { /* silent */ }
  }, []);

  return (
    <div className="flex-1 overflow-hidden px-4 sm:px-6 pb-4 flex flex-col">
      <TelemetryBar stats={stats} />

      {/* Filter bar */}
      <div className="flex items-center gap-1.5 mb-3">
        {(['all', 'pending', 'answered'] as const).map(f => (
          <button
            key={f}
            onClick={() => setFilter(f)}
            className={cn(
              'px-2 py-0.5 text-[10px] rounded transition-colors',
              filter === f ? 'bg-neutral-700 text-neutral-200' : 'text-neutral-500 hover:text-neutral-300',
            )}
          >
            {f === 'all' ? '全部' : f === 'pending' ? '待处理' : '已决策'}
          </button>
        ))}
      </div>

      {/* Three-column layout */}
      <div className="grid grid-cols-12 gap-3 flex-1 min-h-0">
        {/* Live Radar — 3 cols */}
        <div className="col-span-3 bg-neutral-900/50 rounded-lg border border-neutral-800 overflow-hidden">
          <LiveRadar questions={questions} selectedId={selectedId} onSelect={setSelectedId} />
        </div>

        {/* X-Ray — 5 cols */}
        <div className="col-span-5 bg-neutral-900/50 rounded-lg border border-neutral-800 overflow-hidden">
          <DecisionXRay question={selected} onAnswer={handleAnswer} onDismiss={handleDismiss} onKillT3={handleKillT3} />
        </div>

        {/* Evolution Forge — 4 cols */}
        <div className="col-span-4 bg-neutral-900/50 rounded-lg border border-neutral-800 overflow-hidden">
          <EvolutionForge rules={rules} expanded={forgeExpanded} onToggle={() => setForgeExpanded(!forgeExpanded)} />
        </div>
      </div>
    </div>
  );
}
