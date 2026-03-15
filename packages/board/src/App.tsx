'use client';

import { useState, useEffect, useCallback } from 'react';
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

type Tab = 'jarvis' | 'board' | 'terminal' | 'exec' | 'system' | 'knowledge' | 'logs';

interface SlotDef { id: string; label: string; role: string; running?: boolean }

export default function App() {
  useEventStream(); // Global EventBus WS connection
  const wsState = useConnectionState();
  const openAddDialog = useTaskCenterStore((s) => s.openAddDialog);
  const fetchTasks = useTaskCenterStore((s) => s.fetchTasks);
  const isLoading = useTaskCenterStore((s) => s.isLoading);
  const taskCount = useTaskCenterStore((s) => s.tasks.filter((t) => t.status === 'open' && t.category !== 'research').length);
  const [mounted, setMounted] = useState(false);
  const [tab, setTab] = useState<Tab>(() => {
    if (typeof window === 'undefined') return 'board';
    // Migrate old tab values to new ones
    const saved = localStorage.getItem('board:tab') as string || 'jarvis';
    const migration: Record<string, Tab> = {
      'autopilot': 'exec', 'decisions': 'exec',
      'memory': 'system', 'engine': 'system',
      'conversations': 'logs', 'timeline': 'logs',
      'architecture': 'knowledge',
      'deploy': 'board', 'research': 'board',
    };
    return (migration[saved] || saved) as Tab;
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

  const tabs: { id: Tab; label: string; icon: React.ElementType }[] = [
    { id: 'jarvis', label: 'Jarvis', icon: Sparkles },
    { id: 'board', label: 'Board', icon: ClipboardList },
    { id: 'terminal', label: 'Terminal', icon: MonitorUp },
    { id: 'exec', label: 'Exec', icon: Crosshair },
    { id: 'system', label: 'System', icon: Gauge },
    { id: 'knowledge', label: 'Knowledge', icon: Brain },
    { id: 'logs', label: 'Logs', icon: MessageSquareText },
  ];

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

          {/* Tabs — 6 consolidated */}
          <div className="flex items-center gap-1 ml-4 bg-neutral-900 rounded-lg p-0.5 overflow-x-auto overflow-y-hidden">
            {tabs.map(({ id, label, icon: Icon }) => (
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
            ))}
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
          {tab === 'terminal' && slots.filter((s) => s.running).length > 0 && (
            <div className="flex items-center gap-1 mr-2">
              {slots.filter((s) => s.running).map((slot) => (
                <button
                  key={slot.id}
                  onClick={() => setActiveSlot(slot.id)}
                  className={cn(
                    'px-2 py-1 text-[10px] rounded transition-colors font-mono',
                    activeSlot === slot.id
                      ? 'bg-orange-500/20 text-orange-400 border border-orange-500/30'
                      : 'text-neutral-600 hover:text-neutral-400 border border-transparent',
                  )}
                >
                  {slot.label}
                </button>
              ))}
            </div>
          )}
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
        <div className="flex-1 min-h-0 mx-4 sm:mx-8 mb-4 rounded-lg border border-neutral-800 overflow-hidden">
          {activeSlot ? (
            <Terminal key={activeSlot} slotId={activeSlot} />
          ) : (
            <div className="flex items-center justify-center h-full text-neutral-500 text-sm">Loading slots...</div>
          )}
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
