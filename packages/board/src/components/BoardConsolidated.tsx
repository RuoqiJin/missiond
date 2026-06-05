'use client';

import { useState, useEffect } from 'react';
import { cn } from '@/lib/utils';
import { PendingQuestions } from './PendingQuestions';
import { QuickAdd } from './QuickAdd';
import { TaskFilters } from './TaskFilters';
import { TaskListView } from './TaskListView';
import { TaskDialog } from './TaskDialog';
import { DeployDashboard } from './DeployDashboard';
import { ResearchBoard } from './ResearchBoard';

type SubTab = 'all' | 'deploy' | 'research';

const SUB_TABS: { key: SubTab; label: string }[] = [
  { key: 'all', label: 'All' },
  { key: 'deploy', label: 'Deploy' },
  { key: 'research', label: 'Research' },
];

const STORAGE_KEY = 'board:consolidated:sub';

export function BoardConsolidated() {
  const [subTab, setSubTab] = useState<SubTab>(() => {
    if (typeof window === 'undefined') return 'all';
    return (localStorage.getItem(STORAGE_KEY) as SubTab) || 'all';
  });

  useEffect(() => {
    localStorage.setItem(STORAGE_KEY, subTab);
  }, [subTab]);

  return (
    <div className="flex-1 flex flex-col overflow-hidden">
      {/* Segmented control */}
      <div className="px-4 pt-3 sm:px-6">
        <div className="mission-subtab-rail inline-flex items-center gap-1 rounded-lg p-1">
          {SUB_TABS.map((t) => (
            <button
              key={t.key}
              onClick={() => setSubTab(t.key)}
              className={cn(
                'rounded-md px-3 py-1.5 text-xs font-medium transition-colors',
                subTab === t.key
                  ? 'mission-tab-trigger-active'
                  : 'text-stone-500 hover:bg-white/[0.04] hover:text-stone-200',
              )}
            >
              {t.label}
            </button>
          ))}
        </div>
      </div>

      {/* Content */}
      {subTab === 'all' ? (
        <div className="max-w-6xl flex-1 overflow-auto px-4 pb-8 pt-3 sm:px-6">
          <PendingQuestions />
          <div className="mb-4">
            <QuickAdd />
          </div>
          <TaskFilters />
          <TaskListView />
          <TaskDialog />
        </div>
      ) : subTab === 'deploy' ? (
        <DeployDashboard />
      ) : (
        <ResearchBoard />
      )}
    </div>
  );
}
