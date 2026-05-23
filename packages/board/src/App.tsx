'use client';

import { useState, useEffect, useCallback, useMemo } from 'react';
import type { ElementType } from 'react';
import { Plus, ClipboardList, Loader2, MonitorUp, Brain, MessageSquareText, Gauge, Crosshair, Sparkles, Repeat2, Compass } from 'lucide-react';
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
import { CodexReplayDashboard } from './components/CodexReplayDashboard';
import { AgentNavigationDashboard } from './components/AgentNavigationDashboard';
import { useEventStream, useConnectionState, useEventInvalidation } from './hooks/useEventStream';
import { BOARD_TABS, DEFAULT_TAB, TAB_MIGRATION, type BoardTabId } from './generated/board-frontend-config';
import type { SlotDef } from './types';

type Tab = BoardTabId;

const TAB_ICON_MAP: Record<string, ElementType> = {
  Brain,
  Compass,
  ClipboardList,
  Crosshair,
  Gauge,
  MessageSquareText,
  MonitorUp,
  Repeat2,
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

function slotProviderGroup(slot: SlotDef): string {
  const raw = `${slot.provider || ''} ${slot.engine || ''} ${slot.id || ''}`.toLowerCase();
  if (raw.includes('codex')) return 'Codex';
  if (raw.includes('gemini')) return 'Gemini';
  if (raw.includes('claude')) return 'ClaudeCode';
  return slotProviderLabel(slot) || 'Other';
}

function slotStateTone(slot: SlotDef): string {
  const state = slotStateLabel(slot).toLowerCase();
  if (slot.running || ['thinking', 'responding', 'tool_running', 'starting'].includes(state)) return 'bg-emerald-400';
  if (['confirming', 'blocked'].includes(state)) return 'bg-amber-400';
  if (['error'].includes(state)) return 'bg-red-400';
  if (['idle', 'slash_menu'].includes(state)) return 'bg-sky-400';
  return 'bg-neutral-600';
}

function slotStateTextTone(slot: SlotDef): string {
  const state = slotStateLabel(slot).toLowerCase();
  if (slot.running || ['thinking', 'responding', 'tool_running', 'starting'].includes(state)) return 'text-emerald-300';
  if (['confirming', 'blocked'].includes(state)) return 'text-amber-300';
  if (['error'].includes(state)) return 'text-red-300';
  if (['idle', 'slash_menu'].includes(state)) return 'text-sky-300';
  return 'text-neutral-500';
}

function formatSlotTime(value?: string | null): string | null {
  if (!value) return null;
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return value;
  return date.toLocaleString();
}

export default function App() {
  useEventStream(); // Global EventBus WS connection
  const wsState = useConnectionState();
  const slotVersion = useEventInvalidation('slot');
  const openAddDialog = useTaskCenterStore((s) => s.openAddDialog);
  const fetchTasks = useTaskCenterStore((s) => s.fetchTasks);
  const isLoading = useTaskCenterStore((s) => s.isLoading);
  const tasks = useTaskCenterStore((s) => s.tasks);
  const taskCount = useMemo(
    () => tasks.filter((t) => t.status === 'open' && t.category !== 'research').length,
    [tasks],
  );
  const tasksById = useMemo(() => {
    const map = new Map<string, (typeof tasks)[number]>();
    for (const t of tasks) map.set(t.id, t);
    return map;
  }, [tasks]);
  const tasksByExecutor = useMemo(() => {
    const map = new Map<string, (typeof tasks)[number]>();
    for (const t of tasks) {
      if (t.status !== 'running') continue;
      const key = t.claimExecutorId || t.assignee;
      if (key && !map.has(key)) map.set(key, t);
    }
    return map;
  }, [tasks]);
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
  const sortedSlots = useMemo(
    () => [...slots].sort((a, b) => slotActiveRank(a) - slotActiveRank(b) || a.id.localeCompare(b.id)),
    [slots],
  );
  const slotsByProvider = useMemo(() => {
    const groups = new Map<string, SlotDef[]>();
    for (const slot of sortedSlots) {
      const key = slotProviderGroup(slot);
      const existing = groups.get(key) ?? [];
      existing.push(slot);
      groups.set(key, existing);
    }
    return Array.from(groups.entries());
  }, [sortedSlots]);
  const activeSlotDef = useMemo(
    () => slots.find((s) => s.id === activeSlot) ?? null,
    [activeSlot, slots],
  );
  const activeTaskId = activeSlotDef?.activeBoardTaskId ?? activeSlotDef?.currentTaskId;
  const activeSlotTask = (activeTaskId && tasksById.get(activeTaskId)) || (activeSlot ? tasksByExecutor.get(activeSlot) : undefined);
  const terminalActiveTask = activeSlotTask
    ? { id: activeSlotTask.id, title: activeSlotTask.title, status: activeSlotTask.status }
    : null;

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

  // EventBus drives slot refresh; the interval is only a bounded fallback
  // when a backend event is missed or the browser reconnects late.
  useEffect(() => {
    if (tab !== 'terminal' && tab !== 'exec') return;
    fetchSlots();
  }, [tab, slotVersion, fetchSlots]);

  useEffect(() => {
    if (tab !== 'terminal' && tab !== 'exec') return;
    const id = setInterval(fetchSlots, 30000);
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

      {/* Content */}
      {tab === 'jarvis' ? (
        <JarvisChat />
      ) : tab === 'board' ? (
        <BoardConsolidated />
      ) : tab === 'navigator' ? (
        <AgentNavigationDashboard />
      ) : tab === 'terminal' ? (
        <div className="mx-4 mb-4 flex min-h-0 flex-1 flex-col overflow-hidden rounded-lg border border-neutral-800 bg-neutral-950/60 sm:mx-8 lg:flex-row">
          <aside className="flex max-h-48 shrink-0 flex-col border-b border-neutral-800 bg-neutral-950/80 lg:max-h-none lg:w-72 lg:border-b-0 lg:border-r">
            <div className="shrink-0 border-b border-neutral-800 px-3 py-2">
              <div className="text-xs font-medium text-neutral-300">Workstations</div>
              <div className="mt-0.5 text-[10px] text-neutral-600">{slots.length} projected slots</div>
            </div>
            <div className="min-h-0 flex-1 overflow-auto p-2">
              {slotsByProvider.length > 0 ? (
                <div className="space-y-3">
                  {slotsByProvider.map(([provider, providerSlots]) => (
                    <div key={provider}>
                      <div className="mb-1 flex items-center justify-between px-1 text-[10px] uppercase tracking-wide text-neutral-600">
                        <span>{provider}</span>
                        <span>{providerSlots.length}</span>
                      </div>
                      <div className="space-y-1">
                        {providerSlots.map((slot) => {
                          const isActive = activeSlot === slot.id;
                          const stateLabel = slotStateLabel(slot);
                          const activeTaskForSlotId = slot.activeBoardTaskId ?? slot.currentTaskId;
                          const slotTask = (activeTaskForSlotId && tasksById.get(activeTaskForSlotId)) || tasksByExecutor.get(slot.id);
                          return (
                            <button
                              key={slot.id}
                              onClick={() => setActiveSlot(slot.id)}
                              title={[
                                slot.id,
                                slot.label,
                                `state: ${stateLabel}`,
                                slotProviderLabel(slot) ? `provider: ${slotProviderLabel(slot)}` : null,
                                slot.mcpReady !== undefined ? `MCP: ${slot.mcpReady ? 'ready' : 'not unattended-ready'}` : null,
                                slot.mcpApprovalMissingTools?.length
                                  ? `missing MCP approvals: ${slot.mcpApprovalMissingTools.join(', ')}`
                                  : null,
                                slotTask ? `task: ${slotTask.id} ${slotTask.title}` : null,
                                slot.activeTool ? `tool: ${slot.activeTool}` : null,
                                slot.blockedKind ? `blocked: ${slot.blockedKind}` : null,
                              ].filter(Boolean).join('\n')}
                              className={cn(
                                'w-full rounded-md border px-2 py-2 text-left transition-colors',
                                isActive
                                  ? 'border-orange-500/45 bg-orange-500/15 text-orange-200'
                                  : 'border-neutral-900 bg-neutral-900/40 text-neutral-400 hover:border-neutral-700 hover:bg-neutral-900/80 hover:text-neutral-200',
                              )}
                            >
                              <div className="flex min-w-0 items-center gap-2">
                                <span className={cn('h-2 w-2 shrink-0 rounded-full', slotStateTone(slot), slot.running && 'animate-pulse')} />
                                <span className="min-w-0 flex-1 truncate text-xs font-medium">{slot.label || slot.id}</span>
                                <span className={cn('shrink-0 text-[10px] font-mono', slotStateTextTone(slot))}>{stateLabel}</span>
                              </div>
                              <div className="mt-1 flex min-w-0 items-center gap-1 text-[10px] text-neutral-600">
                                <span className="truncate">{slot.modelProfile || slot.engine || slot.role}</span>
                                {slotTask ? <span className="shrink-0 text-orange-300/80">#{slotTask.id.slice(0, 6)}</span> : null}
                              </div>
                            </button>
                          );
                        })}
                      </div>
                    </div>
                  ))}
                </div>
              ) : (
                <div className="px-1 text-xs text-neutral-600">Loading slots...</div>
              )}
            </div>
          </aside>

          <main className="min-h-0 flex-1">
            {activeSlot ? (
              <Terminal key={activeSlot} slotId={activeSlot} slot={activeSlotDef ?? undefined} activeTask={terminalActiveTask} />
            ) : (
              <div className="flex h-full items-center justify-center text-sm text-neutral-500">Loading slots...</div>
            )}
          </main>

          <aside className="hidden w-80 shrink-0 flex-col border-l border-neutral-800 bg-neutral-950/80 xl:flex">
            <div className="border-b border-neutral-800 px-3 py-2">
              <div className="text-xs font-medium text-neutral-300">Evidence</div>
              <div className="mt-0.5 truncate text-[10px] text-neutral-600" title={activeSlotDef?.id}>{activeSlotDef?.id || 'No slot selected'}</div>
            </div>
            <div className="min-h-0 flex-1 space-y-4 overflow-auto p-3 text-xs">
              <section>
                <div className="mb-1 text-[10px] uppercase tracking-wide text-neutral-600">Runtime</div>
                <dl className="space-y-1.5 text-neutral-400">
                  <div className="flex justify-between gap-3"><dt>Provider</dt><dd className="truncate text-neutral-300">{activeSlotDef ? slotProviderLabel(activeSlotDef) : '—'}</dd></div>
                  <div className="flex justify-between gap-3"><dt>State</dt><dd className={cn('font-mono', activeSlotDef && slotStateTextTone(activeSlotDef))}>{activeSlotDef ? slotStateLabel(activeSlotDef) : '—'}</dd></div>
                  <div className="flex justify-between gap-3"><dt>Model</dt><dd className="truncate text-neutral-300">{activeSlotDef?.modelProfile || activeSlotDef?.engine || '—'}</dd></div>
                  <div className="flex justify-between gap-3"><dt>Confidence</dt><dd className="font-mono text-neutral-300">{activeSlotDef?.confidence !== undefined ? activeSlotDef.confidence.toFixed(2) : '—'}</dd></div>
                  <div className="flex justify-between gap-3"><dt>Reason</dt><dd className="max-w-44 truncate text-neutral-300" title={activeSlotDef?.reason}>{activeSlotDef?.reason || '—'}</dd></div>
                </dl>
              </section>

              <section>
                <div className="mb-1 text-[10px] uppercase tracking-wide text-neutral-600">BoardTask</div>
                {terminalActiveTask ? (
                  <div className="rounded-md border border-neutral-800 bg-neutral-900/50 p-2">
                    <div className="font-mono text-[10px] text-orange-300">#{terminalActiveTask.id}</div>
                    <div className="mt-1 text-neutral-200">{terminalActiveTask.title}</div>
                    <div className="mt-1 text-[10px] text-neutral-600">{terminalActiveTask.status}</div>
                  </div>
                ) : (
                  <div className="text-neutral-600">No active BoardTask binding.</div>
                )}
              </section>

              <section>
                <div className="mb-1 text-[10px] uppercase tracking-wide text-neutral-600">Durable Conversation</div>
                {activeSlotDef?.latestConversation ? (
                  <dl className="space-y-1.5 text-neutral-400">
                    <div className="flex justify-between gap-3"><dt>Source</dt><dd className="text-neutral-300">{activeSlotDef.latestConversation.source || '—'}</dd></div>
                    <div className="flex justify-between gap-3"><dt>Status</dt><dd className="text-neutral-300">{activeSlotDef.latestConversation.status || '—'}</dd></div>
                    <div className="flex justify-between gap-3"><dt>Messages</dt><dd className="font-mono text-neutral-300">{activeSlotDef.latestConversation.messageCount ?? '—'}</dd></div>
                    <div className="flex justify-between gap-3"><dt>Updated</dt><dd className="max-w-44 truncate text-neutral-300">{formatSlotTime(activeSlotDef.latestConversation.updatedAt) || '—'}</dd></div>
                    <div className="truncate font-mono text-[10px] text-neutral-600" title={activeSlotDef.latestConversation.id}>{activeSlotDef.latestConversation.id}</div>
                  </dl>
                ) : (
                  <div className="text-neutral-600">No durable conversation yet.</div>
                )}
              </section>

              <section>
                <div className="mb-1 text-[10px] uppercase tracking-wide text-neutral-600">Diagnostics</div>
                <dl className="space-y-1.5 text-neutral-400">
                  <div className="flex justify-between gap-3"><dt>MCP</dt><dd className={activeSlotDef?.mcpReady ? 'text-emerald-300' : 'text-neutral-500'}>{activeSlotDef?.mcpReady === undefined ? '—' : activeSlotDef.mcpReady ? 'ready' : 'not ready'}</dd></div>
                  <div className="flex justify-between gap-3"><dt>Approvals</dt><dd className={activeSlotDef?.mcpApprovalReady ? 'text-emerald-300' : 'text-neutral-500'}>{activeSlotDef?.mcpApprovalReady === undefined ? '—' : activeSlotDef.mcpApprovalReady ? 'ready' : 'missing'}</dd></div>
                  <div className="flex justify-between gap-3"><dt>Tool</dt><dd className="max-w-44 truncate text-neutral-300" title={activeSlotDef?.activeTool}>{activeSlotDef?.activeTool || '—'}</dd></div>
                  <div className="flex justify-between gap-3"><dt>Blocked</dt><dd className="max-w-44 truncate text-neutral-300" title={activeSlotDef?.blockedKind}>{activeSlotDef?.blockedKind || '—'}</dd></div>
                </dl>
                {activeSlotDef?.mcpApprovalMissingTools?.length ? (
                  <div className="mt-2 rounded-md border border-amber-500/20 bg-amber-500/5 p-2 text-[10px] text-amber-200">
                    {activeSlotDef.mcpApprovalMissingTools.join(', ')}
                  </div>
                ) : null}
              </section>
            </div>
          </aside>
        </div>
      ) : tab === 'exec' ? (
        <ExecDashboard slots={slots} tasks={tasks} />
      ) : tab === 'codex' ? (
        <CodexReplayDashboard />
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
