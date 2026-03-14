'use client';

import { useState, useEffect } from 'react';
import { cn } from '@/lib/utils';
import { Conversations } from './Conversations';
import { CognitiveTimeline } from './timeline';

type SubTab = 'sessions' | 'events';

const SUB_TABS: { key: SubTab; label: string }[] = [
  { key: 'sessions', label: 'Sessions' },
  { key: 'events', label: 'Events' },
];

const STORAGE_KEY = 'board:logs:sub';

export function LogsConsolidated() {
  const [subTab, setSubTab] = useState<SubTab>(() => {
    if (typeof window === 'undefined') return 'sessions';
    return (localStorage.getItem(STORAGE_KEY) as SubTab) || 'sessions';
  });

  useEffect(() => {
    localStorage.setItem(STORAGE_KEY, subTab);
  }, [subTab]);

  return (
    <div className="flex-1 flex flex-col overflow-hidden">
      {/* Toggle */}
      <div className="px-4 sm:px-8 pt-2 pb-1">
        <div className="inline-flex items-center gap-0.5 bg-neutral-900 rounded-lg p-0.5">
          {SUB_TABS.map((t) => (
            <button
              key={t.key}
              onClick={() => setSubTab(t.key)}
              className={cn(
                'px-3 py-1.5 text-xs font-medium rounded-md transition-colors',
                subTab === t.key
                  ? 'bg-neutral-800 text-white'
                  : 'text-neutral-500 hover:text-neutral-300',
              )}
            >
              {t.label}
            </button>
          ))}
        </div>
      </div>

      {/* Both mounted, toggle visibility with CSS to preserve state */}
      <div className={cn('flex-1 min-h-0', subTab === 'sessions' ? 'flex flex-col' : 'hidden')}>
        <Conversations />
      </div>
      <div className={cn('flex-1 min-h-0', subTab === 'events' ? 'flex flex-col' : 'hidden')}>
        <CognitiveTimeline />
      </div>
    </div>
  );
}
