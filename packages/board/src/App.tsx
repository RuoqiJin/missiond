'use client';

import { useState, useEffect } from 'react';
import { Plus, ClipboardList, Loader2, MonitorUp, Brain, MessageSquareText } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Skeleton } from '@/components/ui/skeleton';
import { cn } from '@/lib/utils';
import { useTaskCenterStore } from './store';
import { QuickAdd } from './components/QuickAdd';
import { TaskFilters } from './components/TaskFilters';
import { TaskListView } from './components/TaskListView';
import { TaskDialog } from './components/TaskDialog';
import { Terminal } from './components/Terminal';
import { KnowledgeBase } from './components/KnowledgeBase';
import { Conversations } from './components/Conversations';
import { PendingQuestions } from './components/PendingQuestions';

type Tab = 'board' | 'terminal' | 'knowledge' | 'conversations';

const SLOTS = [
  { id: 'slot-deploy-1', label: 'Deploy', role: 'deploy' },
  { id: 'slot-coder-1', label: 'Coder', role: 'coder' },
  { id: 'slot-secret-1', label: 'Secret', role: 'secret' },
] as const;

export default function App() {
  const openAddDialog = useTaskCenterStore((s) => s.openAddDialog);
  const fetchTasks = useTaskCenterStore((s) => s.fetchTasks);
  const isLoading = useTaskCenterStore((s) => s.isLoading);
  const taskCount = useTaskCenterStore((s) => s.tasks.filter((t) => t.status === 'open').length);
  const [mounted, setMounted] = useState(false);
  const [tab, setTab] = useState<Tab>('board');
  const [activeSlot, setActiveSlot] = useState<string>(SLOTS[0].id);

  useEffect(() => {
    setMounted(true);
    fetchTasks();
  }, [fetchTasks]);

  if (!mounted) {
    return (
      <div className="min-h-screen bg-[#0a0a0a] p-4 sm:p-8">
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
    <div className="h-screen flex flex-col bg-[#0a0a0a]">
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
          <div className="flex items-center gap-1 ml-4 bg-neutral-900 rounded-lg p-0.5">
            <button
              onClick={() => setTab('board')}
              className={cn(
                'px-3 py-1.5 text-xs font-medium rounded-md transition-colors',
                tab === 'board' ? 'bg-neutral-800 text-white' : 'text-neutral-500 hover:text-neutral-300',
              )}
            >
              Tasks
            </button>
            <button
              onClick={() => setTab('terminal')}
              className={cn(
                'px-3 py-1.5 text-xs font-medium rounded-md transition-colors flex items-center gap-1.5',
                tab === 'terminal' ? 'bg-neutral-800 text-white' : 'text-neutral-500 hover:text-neutral-300',
              )}
            >
              <MonitorUp className="w-3 h-3" />
              Terminal
            </button>
            <button
              onClick={() => setTab('knowledge')}
              className={cn(
                'px-3 py-1.5 text-xs font-medium rounded-md transition-colors flex items-center gap-1.5',
                tab === 'knowledge' ? 'bg-neutral-800 text-white' : 'text-neutral-500 hover:text-neutral-300',
              )}
            >
              <Brain className="w-3 h-3" />
              Knowledge
            </button>
            <button
              onClick={() => setTab('conversations')}
              className={cn(
                'px-3 py-1.5 text-xs font-medium rounded-md transition-colors flex items-center gap-1.5',
                tab === 'conversations' ? 'bg-neutral-800 text-white' : 'text-neutral-500 hover:text-neutral-300',
              )}
            >
              <MessageSquareText className="w-3 h-3" />
              Logs
            </button>
          </div>
        </div>

        <div className="flex items-center gap-2">
          {tab === 'terminal' && (
            <div className="flex items-center gap-1 mr-2">
              {SLOTS.map((slot) => (
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

      {/* Content */}
      {tab === 'board' ? (
        <div className="flex-1 overflow-auto px-4 sm:px-8 pb-8 max-w-4xl">
          <PendingQuestions />
          <div className="mb-4">
            <QuickAdd />
          </div>
          <TaskFilters />
          <TaskListView />
          <TaskDialog />
        </div>
      ) : tab === 'terminal' ? (
        <div className="flex-1 min-h-0 mx-4 sm:mx-8 mb-4 rounded-lg border border-neutral-800 overflow-hidden">
          <Terminal key={activeSlot} slotId={activeSlot} />
        </div>
      ) : tab === 'knowledge' ? (
        <KnowledgeBase />
      ) : (
        <Conversations />
      )}
    </div>
  );
}
