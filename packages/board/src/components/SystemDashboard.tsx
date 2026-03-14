'use client';

import { useState } from 'react';
import { ChevronDown, ChevronRight, Activity, Gauge } from 'lucide-react';
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

export function SystemDashboard() {
  const [memoryExpanded, setMemoryExpanded] = useState(true);
  const [engineExpanded, setEngineExpanded] = useState(true);

  return (
    <div className="flex-1 flex flex-col overflow-auto">
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
