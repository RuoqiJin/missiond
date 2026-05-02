'use client';

import { useState, useEffect, useCallback } from 'react';
import type { ElementType } from 'react';
import { Plus, ClipboardList, Loader2, MonitorUp, Brain, MessageSquareText, Gauge, Crosshair, Sparkles } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Skeleton } from '@/components/ui/skeleton';
import { cn } from '@/lib/utils';
import { useTaskCenterStore } from './store';
import { Terminal } from './components/Terminal';
import { BoardConsolidated } from './components/BoardConsolidated';
import { ExecDashboard } from './components/ExecDashboard';
import { SystemDashboard } from './components/SystemDashboard';
import { KnowledgeConsolidated } from './components/KnowledgeConsolidated';
import { LogsConsolidated } from './components/LogsConsolidated';
import { JarvisChat } from './components/JarvisChat';
import { useEventStream, useConnectionState } from './hooks/useEventStream';
import { BOARD_TABS, DEFAULT_TAB, TAB_MIGRATION, type BoardTabId } from './generated/board-frontend-config';
import type { SlotDef } from './types';

type Tab = BoardTabId;

const TAB_ICON_MAP: Record<string, ElementType> = {
  Brain,
  ClipboardList,
  Crosshair,
  Gauge,
  MessageSquareText,
  MonitorUp,
  Sparkles,
};
const TAB_MIGRATION_LOOKUP = TAB_MIGRATION as Readonly<Record<string, BoardTabId>>;

function isBoardTabId(value: string): value is BoardTabId {
  return BOARD_TABS.some((tab) => tab.id === value);
}

function slotStateLabel(slot: SlotDef) {
  return slot.state || (slot.running ? 'running' : 'stopped');
}

function slotProviderLabel(slot: SlotDef) {
  return slot.provider || slot.engine || slot.role;
}

const ACTIVE_PTY_STATES = new Set(['running', 'thinking', 'responding', 'tool_running', 'confirming', 'blocked', 'starting', 'idle', 'slash_menu']);
function slotActiveRank(slot: SlotDef): number {
  if (slot.running) return 0;
  const state = slot.state?.toLowerCase();
  if (state && ACTIVE_PTY_STATES.has(state)) return 1;
  return 2;
}

export default function App() {
  useEventStream(); // Global EventBus WS connection
  const wsState = useConnectionState();
  const openAddDialog = useTaskCenterStore((s) => s.openAddDialog);
  const fetchTasks = useTaskCenterStore((s) => s.fetchTasks);
  const isLoading = useTaskCenterStore((s) => s.isLoading);
  const taskCount = useTaskCenterStore((s) => s.tasks.filter((t) => t.status === 'open' && t.category !== 'research').length);
  const tasksById = useTaskCenterStore((s) => {
    const map = new Map<string, (typeof s.tasks)[number]>();
    for (const t of s.tasks) map.set(t.id, t);
    return map;
  });
  const tasksByExecutor = useTaskCenterStore((s) => {
    const map = new Map<string, (typeof s.tasks)[number]>();
    for (const t of s.tasks) {
      if (t.status !== 'running') continue;
      const key = t.claimExecutorId || t.assignee;
      if (key && !map.has(key)) map.set(key, t);
    }
    return map;
  });
  const [mounted, setMounted] = useState(false);
  const [tab, setTab] = useState<Tab>(() => {
    if (typeof window === 'undefined') return DEFAULT_TAB;
    const saved = localStorage.getItem('board:tab') || DEFAULT_TAB;
    const migrated = TAB_MIGRATION_LOOKUP[saved] ?? saved;
    return isBoardTabId(migrated) ? migrated : DEFAULT_TAB;
  });
  const [slots, setSlots] = useState<SlotDef[]>([]);
  const [activeSlot, setActiveSlot] = useState<string>(() => {
    if (typeof window === 'undefined') return '';
    return localStorage.getItem('board:slot') || '';
  });

  // Persist tab & slot to localStorage
  useEffect(() => { localStorage.setItem('board:tab', tab); }, [tab]);
  useEffect(() => { if (activeSlot) localStorage.setItem('board:slot', activeSlot); }, [activeSlot]);

  const fetchSlots = useCallback(() => {
    fetch('/api/slots')
      .then((r) => r.json())
      .then((data: SlotDef[]) => {
        if (Array.isArray(data) && data.length > 0) {
          setSlots(data);
          setActiveSlot((prev) => {
            if (prev && data.some((s) => s.id === prev)) return prev;
            const running = data.find((s) => s.running);
            return running?.id ?? data[0].id;
          });
        }
      })
      .catch(() => {});
  }, []);

  useEffect(() => {
    setMounted(true);
    fetchTasks();
    fetchSlots();
  }, [fetchTasks, fetchSlots]);

  // Refresh slots when on Terminal or Exec tab
  useEffect(() => {
    if (tab !== 'terminal' && tab !== 'exec') return;
    const id = setInterval(fetchSlots, 5000);
    return () => clearInterval(id);
  }, [tab, fetchSlots]);

  if (!mounted) {
    return (
      <div className="min-h-screen bg-background p-4 sm:p-8">
        <Skeleton className="h-7 w-32 bg-neutral-800 mb-6" />
        <Skeleton className="h-10 bg-neutral-800/50 rounded-lg mb-4" />
        <div className="space-y-2">
          {[1, 2, 3, 4].map((i) => (
            <Skeleton key={i} className="h-16 bg-neutral-800/30 rounded-lg" />
          ))}
        </div>
      </div>
    );
  }

  return (
    <div className="h-screen flex flex-col bg-background">
      {/* Top bar with tabs */}
      <div className="flex items-center justify-between px-4 sm:px-8 pt-4 pb-2">
        <div className="flex items-center gap-4">
          <div className="flex items-center gap-3">
            <div className="p-2 rounded-lg bg-orange-500/10">
              <ClipboardList className="w-5 h-5 text-orange-400" />
            </div>
            <div>
              <h1 className="text-xl font-semibold text-white">Mission Board</h1>
              <p className="text-xs text-neutral-500 mt-0.5 flex items-center gap-1.5">
                {isLoading && <Loader2 className="w-3 h-3 animate-spin" />}
                {taskCount} 个待办
              </p>
            </div>
          </div>

          {/* Tabs */}
          <div className="flex items-center gap-1 ml-4 bg-neutral-900 rounded-lg p-0.5 overflow-x-auto overflow-y-hidden">
            {BOARD_TABS.map(({ id, label, icon }) => {
              const Icon = TAB_ICON_MAP[icon] ?? ClipboardList;
              return (
                <button
                  key={id}
                  onClick={() => setTab(id)}
                  className={cn(
                    'px-3 py-1.5 text-xs font-medium rounded-md transition-colors flex items-center gap-1.5',
                    tab === id ? 'bg-neutral-800 text-white' : 'text-neutral-500 hover:text-neutral-300',
                  )}
                >
                  <Icon className="w-3 h-3" />
                  {label}
                </button>
              );
            })}
          </div>
        </div>

        <div className="flex items-center gap-2">
          {/* WS connection indicator */}
          <div
            className={cn(
              'w-1.5 h-1.5 rounded-full transition-colors',
              wsState === 'connected' ? 'bg-emerald-500' : wsState === 'connecting' ? 'bg-amber-500 animate-pulse' : 'bg-neutral-600',
            )}
            title={`EventBus: ${wsState}`}
          />
          {tab === 'board' && (
            <Button size="sm" variant="outline" onClick={() => openAddDialog()} className="gap-1 border-neutral-800 text-neutral-400 hover:text-white">
              <Plus className="w-4 h-4" />
              详细创建
            </Button>
          )}
        </div>
      </div>

      {/* Content — 7 tabs */}
      {tab === 'jarvis' ? (
        <JarvisChat />
      ) : tab === 'board' ? (
        <BoardConsolidated />
      ) : tab === 'terminal' ? (
        <div className="flex-1 min-h-0 mx-4 sm:mx-8 mb-4 rounded-lg border border-neutral-800 overflow-hidden flex flex-col">
          <div className="shrink-0 border-b border-neutral-800 bg-neutral-950/70 px-2 py-2">
            {slots.length > 0 ? (
              <div className="flex items-center gap-1 overflow-x-auto overflow-y-hidden whitespace-nowrap">
                {[...slots].sort((a, b) => slotActiveRank(a) - slotActiveRank(b)).map((slot) => {
                  const isActive = activeSlot === slot.id;
                  const stateLabel = slotStateLabel(slot);
                  const providerLabel = slotProviderLabel(slot);
                  const activeTaskId = slot.activeBoardTaskId ?? slot.currentTaskId;
                  const slotTask = (activeTaskId && tasksById.get(activeTaskId)) || tasksByExecutor.get(slot.id);
                  return (
                    <button
                      key={slot.id}
                      onClick={() => setActiveSlot(slot.id)}
                      title={[
                        slot.label,
                        `state: ${stateLabel}`,
                        providerLabel ? `provider: ${providerLabel}` : null,
                        slotTask ? `task: ${slotTask.id} ${slotTask.title}` : null,
                        slot.activeTool ? `tool: ${slot.activeTool}` : null,
                        slot.blockedKind ? `blocked: ${slot.blockedKind}` : null,
                      ].filter(Boolean).join('\n')}
                      className={cn(
                        'shrink-0 min-w-[160px] max-w-[280px] rounded-md border px-2.5 py-1 text-left font-mono text-[10px] leading-none transition-colors',
                        isActive
                          ? 'border-orange-500/40 bg-orange-500/15 text-orange-300'
                          : slot.running
                            ? 'border-emerald-500/30 bg-emerald-500/5 text-neutral-300 hover:border-emerald-400/50'
                            : 'border-neutral-800 bg-neutral-900/70 text-neutral-500 hover:border-neutral-700 hover:text-neutral-300',
                      )}
                    >
                      <span className="flex min-w-0 items-center gap-1.5">
                        <span className={cn(
                          'h-1.5 w-1.5 shrink-0 rounded-full',
                          slot.running ? 'bg-emerald-400 animate-pulse' : slot.state ? 'bg-emerald-400/30' : 'bg-neutral-600',
                        )} />
                        <span className="min-w-0 flex-1 truncate">{slot.label}</span>
                        {slotTask ? (
                          <span className="shrink-0 text-orange-300/80 font-mono" title={slotTask.title}>
                            #{slotTask.id.slice(0, 6)}
                          </span>
                        ) : null}
                        <span className="shrink-0 text-neutral-600">{stateLabel}</span>
                      </span>
                    </button>
                  );
                })}
              </div>
            ) : (
              <div className="px-1 text-xs text-neutral-600">Loading slots...</div>
            )}
          </div>
          <div className="min-h-0 flex-1">
            {activeSlot ? (
              (() => {
                const slot = slots.find((s) => s.id === activeSlot);
                const activeTaskId = slot?.activeBoardTaskId ?? slot?.currentTaskId;
                const slotTask = (activeTaskId && tasksById.get(activeTaskId)) || tasksByExecutor.get(activeSlot);
                const activeTask = slotTask
                  ? { id: slotTask.id, title: slotTask.title, status: slotTask.status }
                  : null;
                return <Terminal key={activeSlot} slotId={activeSlot} slot={slot} activeTask={activeTask} />;
              })()
            ) : (
              <div className="flex items-center justify-center h-full text-neutral-500 text-sm">Loading slots...</div>
            )}
          </div>
        </div>
      ) : tab === 'exec' ? (
        <ExecDashboard slots={slots} />
      ) : tab === 'system' ? (
        <SystemDashboard />
      ) : tab === 'knowledge' ? (
        <KnowledgeConsolidated />
      ) : (
        <LogsConsolidated />
      )}
    </div>
  );
}
