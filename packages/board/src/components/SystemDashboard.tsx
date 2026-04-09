'use client';

import { useState, useEffect, useCallback } from 'react';
import { ChevronDown, ChevronRight, Activity, Gauge, FolderKanban } from 'lucide-react';
import { cn } from '@/lib/utils';
import { MemoryDashboard } from './MemoryDashboard';
import { EngineDashboard } from './EngineDashboard';

function CollapsibleSection({
  title,
  icon: Icon,
  iconColor,
  expanded,
  onToggle,
  children,
}: {
  title: string;
  icon: React.ComponentType<{ className?: string }>;
  iconColor: string;
  expanded: boolean;
  onToggle: () => void;
  children: React.ReactNode;
}) {
  return (
    <div className="flex flex-col min-h-0">
      <button
        onClick={onToggle}
        className="flex items-center gap-2 px-4 sm:px-8 py-2.5 text-sm font-medium text-neutral-300 hover:text-white transition-colors border-b border-neutral-800/50 shrink-0"
      >
        {expanded ? (
          <ChevronDown className="w-4 h-4 text-neutral-500" />
        ) : (
          <ChevronRight className="w-4 h-4 text-neutral-500" />
        )}
        <Icon className={cn('w-4 h-4', iconColor)} />
        {title}
      </button>
      {expanded && (
        <div className="flex-1 min-h-0 overflow-auto">
          {children}
        </div>
      )}
    </div>
  );
}

interface ProjectInfo {
  id: string;
  path: string;
  intentPath?: string;
  active: boolean;
  slots: string[];
  conversationCount?: number;
}

function ProjectsPanel() {
  const [projects, setProjects] = useState<ProjectInfo[]>([]);
  const [loading, setLoading] = useState(true);

  const fetchProjects = useCallback(async () => {
    try {
      const res = await fetch('/api/projects');
      const data = await res.json();
      if (!Array.isArray(data)) return;
      // Fetch conversation counts per project
      const withCounts = await Promise.all(
        data.map(async (p: ProjectInfo) => {
          try {
            const cr = await fetch(`/api/conversations?project=${p.id}&limit=1`);
            const convs = await cr.json();
            // The response is an array; the backend returns up to limit items
            // We need total count — use a separate approach or just show "has data"
            return { ...p, conversationCount: Array.isArray(convs) ? convs.length : 0 };
          } catch {
            return { ...p, conversationCount: 0 };
          }
        }),
      );
      setProjects(withCounts.sort((a, b) => (b.conversationCount ?? 0) - (a.conversationCount ?? 0)));
    } catch {
      // ignore
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => { fetchProjects(); }, [fetchProjects]);

  if (loading) {
    return <div className="px-6 py-4 text-xs text-neutral-500">加载中...</div>;
  }

  const activeProjects = projects.filter((p) => p.active);
  const inactiveProjects = projects.filter((p) => !p.active);

  return (
    <div className="px-4 sm:px-8 py-4 space-y-3">
      <div className="flex items-center gap-3 text-xs text-neutral-500">
        <span>{projects.length} 个项目</span>
        <span>·</span>
        <span className="text-emerald-500">{activeProjects.length} active</span>
        <span className="text-neutral-600">{inactiveProjects.length} inactive</span>
      </div>

      <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 gap-3">
        {projects.map((p) => (
          <div
            key={p.id}
            className={cn(
              'border rounded-lg p-3 transition-colors',
              p.active
                ? 'border-neutral-800 hover:border-neutral-700 bg-neutral-900/50'
                : 'border-neutral-800/50 bg-neutral-950/50 opacity-60',
            )}
          >
            <div className="flex items-center justify-between mb-1.5">
              <span className="text-sm font-medium text-neutral-200">{p.id}</span>
              <span
                className={cn(
                  'px-1.5 py-0.5 text-[10px] rounded-full font-medium',
                  p.active
                    ? 'bg-emerald-500/10 text-emerald-400 border border-emerald-500/20'
                    : 'bg-neutral-800 text-neutral-500 border border-neutral-700',
                )}
              >
                {p.active ? 'active' : 'inactive'}
              </span>
            </div>
            <div className="text-[11px] text-neutral-500 truncate mb-2" title={p.path}>
              {p.path.replace(/^\/Users\/jinchen\//, '~/')}
            </div>
            <div className="flex items-center gap-3 text-[11px]">
              {p.intentPath && (
                <span className="text-amber-500/70" title={p.intentPath}>
                  ⚙ intent.lisp
                </span>
              )}
              {p.slots.length > 0 && (
                <span className="text-cyan-500/70">
                  ⊞ {p.slots.join(', ')}
                </span>
              )}
            </div>
          </div>
        ))}
      </div>
    </div>
  );
}

export function SystemDashboard() {
  const [projectsExpanded, setProjectsExpanded] = useState(true);
  const [memoryExpanded, setMemoryExpanded] = useState(true);
  const [engineExpanded, setEngineExpanded] = useState(true);

  return (
    <div className="flex-1 flex flex-col overflow-auto">
      <CollapsibleSection
        title="Projects"
        icon={FolderKanban}
        iconColor="text-cyan-400"
        expanded={projectsExpanded}
        onToggle={() => setProjectsExpanded((v) => !v)}
      >
        <ProjectsPanel />
      </CollapsibleSection>

      <CollapsibleSection
        title="Memory Pipeline"
        icon={Activity}
        iconColor="text-purple-400"
        expanded={memoryExpanded}
        onToggle={() => setMemoryExpanded((v) => !v)}
      >
        <MemoryDashboard />
      </CollapsibleSection>

      <CollapsibleSection
        title="System Metrics"
        icon={Gauge}
        iconColor="text-blue-400"
        expanded={engineExpanded}
        onToggle={() => setEngineExpanded((v) => !v)}
      >
        <EngineDashboard />
      </CollapsibleSection>
    </div>
  );
}
