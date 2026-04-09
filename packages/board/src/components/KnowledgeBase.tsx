'use client';

import { useState, useEffect, useCallback, useMemo } from 'react';
import { Brain, Search, ChevronDown, ChevronRight, Trash2, RefreshCw, Server, Settings, FolderKanban, CalendarClock, GitBranch } from 'lucide-react';
import { cn } from '@/lib/utils';
import { Badge } from '@/components/ui/badge';

interface KBEntry {
  id: string;
  category: string;
  key: string;
  summary: string;
  detail?: Record<string, unknown>;
  source: string;
  confidence: number;
  accessCount: number;
  createdAt: string;
  updatedAt: string;
  contextSnippet?: string;
}

interface Project {
  id: string;
  path: string;
  active: boolean;
}

const CATEGORY_CONFIG: Record<string, { label: string; icon: typeof Brain; color: string; bg: string }> = {
  memory: { label: '记忆', icon: Brain, color: 'text-purple-400', bg: 'bg-purple-500/10' },
  architecture: { label: '架构', icon: Server, color: 'text-cyan-400', bg: 'bg-cyan-500/10' },
  policy: { label: '策略', icon: Settings, color: 'text-rose-400', bg: 'bg-rose-500/10' },
  preference: { label: '偏好', icon: Settings, color: 'text-amber-400', bg: 'bg-amber-500/10' },
  infra: { label: '基础设施', icon: Server, color: 'text-blue-400', bg: 'bg-blue-500/10' },
  project: { label: '项目', icon: FolderKanban, color: 'text-green-400', bg: 'bg-green-500/10' },
  decision: { label: '决策', icon: Brain, color: 'text-orange-400', bg: 'bg-orange-500/10' },
  feature: { label: '功能', icon: FolderKanban, color: 'text-emerald-400', bg: 'bg-emerald-500/10' },
  ops: { label: '运维', icon: Server, color: 'text-indigo-400', bg: 'bg-indigo-500/10' },
};

const RENEWAL_KEYWORDS = [
  '续费', '到期', '月付', '年付', '包月', '包年', '自动续费',
  'monthly', 'yearly', '月$', '元/月', '元/年',
  '$20', '$25', '$22', '¥68', '¥118', '¥40',
  'expir', 'renew', 'billing', 'subscription',
];

function isRenewalEntry(entry: KBEntry): boolean {
  const text = `${entry.summary} ${JSON.stringify(entry.detail || '')}`.toLowerCase();
  return RENEWAL_KEYWORDS.some((kw) => text.includes(kw.toLowerCase()));
}

type ViewMode = 'knowledge' | 'renewals';

const SOURCE_LABELS: Record<string, string> = {
  conversation: '对话提取',
  import: '导入',
  discovery: '自动发现',
  manual: '手动',
};

function timeAgo(dateStr: string): string {
  const diff = Date.now() - new Date(dateStr).getTime();
  const mins = Math.floor(diff / 60000);
  if (mins < 1) return '刚刚';
  if (mins < 60) return `${mins}分钟前`;
  const hours = Math.floor(mins / 60);
  if (hours < 24) return `${hours}小时前`;
  const days = Math.floor(hours / 24);
  if (days < 30) return `${days}天前`;
  return new Date(dateStr).toLocaleDateString('zh-CN');
}

function DetailView({ detail }: { detail: Record<string, unknown> }) {
  return (
    <div className="mt-2 p-3 rounded-md bg-neutral-950 border border-neutral-800 text-xs font-mono overflow-x-auto">
      <pre className="text-neutral-400 whitespace-pre-wrap break-words">
        {JSON.stringify(detail, null, 2)}
      </pre>
    </div>
  );
}

function KBEntryCard({ entry, onDelete }: { entry: KBEntry; onDelete: (key: string) => void }) {
  const [expanded, setExpanded] = useState(false);
  const config = CATEGORY_CONFIG[entry.category] || CATEGORY_CONFIG.memory;

  return (
    <div className="group border border-neutral-800 rounded-lg hover:border-neutral-700 transition-colors">
      <div
        className="flex items-start gap-3 p-3 cursor-pointer"
        onClick={() => entry.detail && setExpanded(!expanded)}
      >
        <div className="flex-shrink-0 mt-0.5">
          {entry.detail ? (
            expanded ? (
              <ChevronDown className="w-3.5 h-3.5 text-neutral-500" />
            ) : (
              <ChevronRight className="w-3.5 h-3.5 text-neutral-500" />
            )
          ) : (
            <div className="w-3.5" />
          )}
        </div>

        <div className="flex-1 min-w-0">
          <div className="flex items-center gap-2 mb-1">
            <span className="text-[11px] font-mono text-neutral-500 bg-neutral-800/50 px-1.5 py-0.5 rounded">
              {entry.key}
            </span>
            {entry.confidence < 1 && (
              <span className="text-[10px] text-neutral-600">
                {Math.round(entry.confidence * 100)}%
              </span>
            )}
          </div>
          <p className="text-sm text-neutral-300 leading-relaxed">{entry.summary}</p>
          {entry.contextSnippet && (
            <p className="text-xs text-indigo-300/70 mt-1 italic line-clamp-2">{entry.contextSnippet}</p>
          )}
          <div className="flex items-center gap-3 mt-1.5 text-[11px] text-neutral-600">
            <span>{SOURCE_LABELS[entry.source] || entry.source}</span>
            <span>{timeAgo(entry.updatedAt)}</span>
          </div>
        </div>

        <button
          onClick={(e) => {
            e.stopPropagation();
            onDelete(entry.key);
          }}
          className="flex-shrink-0 p-1 rounded opacity-0 group-hover:opacity-100 hover:bg-red-500/10 hover:text-red-400 text-neutral-600 transition-all"
          title="删除"
        >
          <Trash2 className="w-3.5 h-3.5" />
        </button>
      </div>

      {expanded && entry.detail && (
        <div className="px-3 pb-3 pl-9">
          <DetailView detail={entry.detail} />
        </div>
      )}
    </div>
  );
}

export function KnowledgeBase() {
  const [entries, setEntries] = useState<KBEntry[]>([]);
  const [loading, setLoading] = useState(true);
  const [search, setSearch] = useState('');
  const [searchResults, setSearchResults] = useState<KBEntry[] | null>(null);
  const [searchMode, setSearchMode] = useState<'local' | 'hybrid'>('local');
  const [activeCategory, setActiveCategory] = useState<string | null>(null);
  const [activeProject, setActiveProject] = useState<string | null>(null);
  const [projects, setProjects] = useState<Project[]>([]);
  const [viewMode, setViewMode] = useState<ViewMode>('knowledge');

  const fetchEntries = useCallback(async () => {
    setLoading(true);
    try {
      const params = new URLSearchParams();
      if (activeProject) params.set('project', activeProject);
      const qs = params.toString();
      const res = await fetch(`/api/kb${qs ? `?${qs}` : ''}`);
      if (res.ok) {
        const data = await res.json();
        if (Array.isArray(data)) setEntries(data);
      }
    } catch {
      // silent
    }
    setLoading(false);
  }, [activeProject]);

  useEffect(() => {
    fetchEntries();
  }, [fetchEntries]);

  useEffect(() => {
    fetch('/api/projects')
      .then((res) => res.ok ? res.json() : [])
      .then((data) => { if (Array.isArray(data)) setProjects(data.filter((p: Project) => p.active)); })
      .catch(() => {});
  }, []);

  // Backend hybrid search (Enter key triggers)
  const handleSearch = useCallback(async () => {
    if (!search.trim()) {
      setSearchResults(null);
      setSearchMode('local');
      return;
    }
    setLoading(true);
    setSearchMode('hybrid');
    try {
      const params = new URLSearchParams({ query: search, limit: '30' });
      if (activeCategory) params.set('category', activeCategory);
      if (activeProject) params.set('project', activeProject);
      const res = await fetch(`/api/kb?${params}`);
      if (res.ok) {
        const data = await res.json();
        setSearchResults(Array.isArray(data) ? data : data.entries || []);
      }
    } catch {
      setSearchResults([]);
    }
    setLoading(false);
  }, [search, activeCategory, activeProject]);

  const handleDelete = useCallback(async (key: string) => {
    setEntries((prev) => prev.filter((e) => e.key !== key));
    try {
      await fetch(`/api/kb?key=${encodeURIComponent(key)}`, { method: 'DELETE' });
    } catch {
      fetchEntries(); // rollback
    }
  }, [fetchEntries]);

  const renewalCount = useMemo(() => entries.filter(isRenewalEntry).length, [entries]);

  const filtered = useMemo(() => {
    // Use backend search results when available
    if (searchResults !== null) return searchResults;

    let result = entries;
    // Split by view mode: knowledge hides renewals, renewals shows only renewals
    if (viewMode === 'knowledge') {
      result = result.filter((e) => !isRenewalEntry(e));
    } else {
      result = result.filter(isRenewalEntry);
    }
    if (activeCategory) {
      result = result.filter((e) =>
        e.category === activeCategory ||
        e.category.startsWith(`${activeCategory}:`)
      );
    }
    if (search) {
      const q = search.toLowerCase();
      result = result.filter(
        (e) =>
          e.key.toLowerCase().includes(q) ||
          e.summary.toLowerCase().includes(q) ||
          e.category.toLowerCase().includes(q),
      );
    }
    return result;
  }, [entries, searchResults, activeCategory, search, viewMode]);

  // Group by root category (memory:debug → memory)
  const grouped = useMemo(() => {
    const groups: Record<string, KBEntry[]> = {};
    for (const entry of filtered) {
      const root = entry.category.split(':')[0];
      if (!groups[root]) groups[root] = [];
      groups[root].push(entry);
    }
    const order = Object.keys(CATEGORY_CONFIG);
    // Known categories first, then any remaining
    const known = order.filter((cat) => groups[cat]);
    const unknown = Object.keys(groups).filter((cat) => !order.includes(cat));
    return [...known, ...unknown]
      .map((cat) => ({ category: cat, entries: groups[cat] }));
  }, [filtered]);

  // Category counts (scoped to current view mode, before category/search filter)
  const { categoryCounts, viewTotal } = useMemo(() => {
    const base = viewMode === 'knowledge'
      ? entries.filter((e) => !isRenewalEntry(e))
      : entries.filter(isRenewalEntry);
    const counts: Record<string, number> = {};
    for (const entry of base) {
      const root = entry.category.split(':')[0];
      counts[root] = (counts[root] || 0) + 1;
    }
    return { categoryCounts: counts, viewTotal: base.length };
  }, [entries, viewMode]);

  return (
    <div className="flex-1 overflow-auto px-4 sm:px-8 pb-8 max-w-4xl">
      {/* View mode toggle */}
      <div className="flex items-center gap-1 mb-4 bg-neutral-900 rounded-lg p-0.5 w-fit">
        <button
          onClick={() => { setViewMode('knowledge'); setActiveCategory(null); }}
          className={cn(
            'px-3 py-1.5 text-xs font-medium rounded-md transition-colors flex items-center gap-1.5',
            viewMode === 'knowledge' ? 'bg-neutral-800 text-white' : 'text-neutral-500 hover:text-neutral-300',
          )}
        >
          <Brain className="w-3 h-3" />
          知识
        </button>
        <button
          onClick={() => { setViewMode('renewals'); setActiveCategory(null); }}
          className={cn(
            'px-3 py-1.5 text-xs font-medium rounded-md transition-colors flex items-center gap-1.5',
            viewMode === 'renewals' ? 'bg-neutral-800 text-white' : 'text-neutral-500 hover:text-neutral-300',
          )}
        >
          <CalendarClock className="w-3 h-3" />
          续费 {renewalCount > 0 && <span className="text-neutral-600">{renewalCount}</span>}
        </button>
      </div>

      {/* Search + filters */}
      <div className="flex items-center gap-3 mb-4">
        <div className="relative flex-1">
          <Search className="absolute left-3 top-1/2 -translate-y-1/2 w-4 h-4 text-neutral-500" />
          <input
            type="text"
            placeholder={searchMode === 'hybrid' ? "语义搜索 (Enter 检索)" : "搜索知识... (Enter 语义检索)"}
            value={search}
            onChange={(e) => {
              setSearch(e.target.value);
              if (!e.target.value) { setSearchResults(null); setSearchMode('local'); }
            }}
            onKeyDown={(e) => { if (e.key === 'Enter') handleSearch(); }}
            className="w-full pl-9 pr-3 py-2 bg-neutral-900 border border-neutral-800 rounded-lg text-sm text-neutral-300 placeholder:text-neutral-600 focus:outline-none focus:border-neutral-700"
          />
        </div>
        {searchMode === 'hybrid' && (
          <button
            onClick={() => { setSearch(''); setSearchResults(null); setSearchMode('local'); }}
            className="px-2 py-1.5 text-[11px] rounded-lg border border-indigo-500/30 bg-indigo-500/10 text-indigo-300 hover:bg-indigo-500/20 transition-colors"
          >
            语义搜索 · {searchResults?.length ?? 0} 条 ✕
          </button>
        )}
        <button
          onClick={fetchEntries}
          className="p-2 rounded-lg border border-neutral-800 text-neutral-500 hover:text-neutral-300 hover:border-neutral-700 transition-colors"
          title="刷新"
        >
          <RefreshCw className={cn('w-4 h-4', loading && 'animate-spin')} />
        </button>
      </div>

      {/* Category pills + Project filter */}
      <div className="flex items-center gap-2 mb-4 flex-wrap">
        <button
          onClick={() => setActiveCategory(null)}
          className={cn(
            'px-3 py-1 text-xs rounded-full border transition-colors',
            !activeCategory
              ? 'bg-neutral-800 text-white border-neutral-700'
              : 'text-neutral-500 border-neutral-800 hover:text-neutral-300',
          )}
        >
          全部 {viewTotal}
        </button>
        {Object.entries(CATEGORY_CONFIG).map(([key, config]) => {
          const Icon = config.icon;
          const count = categoryCounts[key] || 0;
          if (count === 0) return null;
          return (
            <button
              key={key}
              onClick={() => setActiveCategory(activeCategory === key ? null : key)}
              className={cn(
                'px-3 py-1 text-xs rounded-full border transition-colors flex items-center gap-1.5',
                activeCategory === key
                  ? `${config.bg} ${config.color} border-current/20`
                  : 'text-neutral-500 border-neutral-800 hover:text-neutral-300',
              )}
            >
              <Icon className="w-3 h-3" />
              {config.label} {count}
            </button>
          );
        })}

        {/* Project filter pills */}
        {projects.length > 0 && (
          <>
            <div className="w-px h-4 bg-neutral-800 mx-1" />
            <button
              onClick={() => setActiveProject(null)}
              className={cn(
                'px-3 py-1 text-xs rounded-full border transition-colors flex items-center gap-1.5',
                !activeProject
                  ? 'bg-cyan-500/10 text-cyan-400 border-cyan-500/20'
                  : 'text-neutral-500 border-neutral-800 hover:text-neutral-300',
              )}
            >
              <GitBranch className="w-3 h-3" />
              全部项目
            </button>
            {projects.map((proj) => (
              <button
                key={proj.id}
                onClick={() => setActiveProject(activeProject === proj.id ? null : proj.id)}
                className={cn(
                  'px-3 py-1 text-xs rounded-full border transition-colors flex items-center gap-1.5',
                  activeProject === proj.id
                    ? 'bg-cyan-500/10 text-cyan-400 border-cyan-500/20'
                    : 'text-neutral-500 border-neutral-800 hover:text-neutral-300',
                )}
              >
                {proj.id}
              </button>
            ))}
          </>
        )}
      </div>

      {/* Entries grouped by category */}
      {loading && entries.length === 0 ? (
        <div className="text-center py-12 text-neutral-600">加载中...</div>
      ) : filtered.length === 0 ? (
        <div className="text-center py-12 text-neutral-600">
          {search ? '没有匹配的知识条目' : '知识库为空'}
        </div>
      ) : (
        <div className="space-y-6">
          {grouped.map(({ category, entries: catEntries }) => {
            const config = CATEGORY_CONFIG[category] || CATEGORY_CONFIG.memory;
            const Icon = config.icon;
            return (
              <div key={category}>
                <div className="flex items-center gap-2 mb-2">
                  <Icon className={cn('w-4 h-4', config.color)} />
                  <h3 className={cn('text-sm font-medium', config.color)}>
                    {config.label}
                  </h3>
                  <Badge variant="outline" className="text-[10px] text-neutral-600 border-neutral-800">
                    {catEntries.length}
                  </Badge>
                </div>
                <div className="space-y-1.5">
                  {catEntries.map((entry) => (
                    <KBEntryCard key={entry.id} entry={entry} onDelete={handleDelete} />
                  ))}
                </div>
              </div>
            );
          })}
        </div>
      )}
    </div>
  );
}
