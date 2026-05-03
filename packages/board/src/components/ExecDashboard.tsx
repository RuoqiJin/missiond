'use client';

import { useEffect, useMemo, useState } from 'react';
import type { ReactNode } from 'react';
import { Activity, AlertTriangle, Bot, BrainCircuit, CheckCircle2, Circle, TerminalSquare, Wrench } from 'lucide-react';
import { Terminal } from './Terminal';
import type { SlotDef } from '../types';
import { cn } from '@/lib/utils';

const PROVIDER_LABELS: Record<string, string> = {
  codex: 'Codex',
  codex_cli: 'Codex',
  claude: 'ClaudeCode',
  claude_code: 'ClaudeCode',
  gemini: 'Gemini',
  gemini_cli: 'Gemini',
};

function providerKey(slot: SlotDef) {
  const raw = (slot.provider || slot.engine || slot.id).toLowerCase();
  if (raw.includes('codex')) return 'codex';
  if (raw.includes('gemini')) return 'gemini';
  if (raw.includes('claude')) return 'claude';
  return 'other';
}

function stateTone(slot: SlotDef) {
  const state = (slot.state || '').toLowerCase();
  if (slot.running || ['running', 'thinking', 'responding', 'tool_running', 'confirming', 'blocked'].includes(state)) {
    return 'text-emerald-400';
  }
  if (['idle', 'slash_menu'].includes(state)) return 'text-blue-400';
  if (['exited', 'error', 'stopped', 'not_running'].includes(state)) return 'text-neutral-600';
  return 'text-amber-400';
}

function stateLabel(slot: SlotDef) {
  if (slot.blockedKind) return `blocked:${slot.blockedKind}`;
  return slot.state || (slot.running ? 'running' : 'unknown');
}

export function ExecDashboard({ slots }: { slots: SlotDef[] }) {
  const [selectedId, setSelectedId] = useState<string | null>(null);

  const sortedSlots = useMemo(() => {
    return [...slots].sort((a, b) => {
      const rank = (s: SlotDef) => {
        if (s.id === 'slot-codex-master-control') return 0;
        if (s.running) return 1;
        if ((s.state || '').toLowerCase() === 'idle') return 2;
        return 3;
      };
      return rank(a) - rank(b) || providerKey(a).localeCompare(providerKey(b)) || a.id.localeCompare(b.id);
    });
  }, [slots]);

  useEffect(() => {
    if (selectedId && sortedSlots.some((s) => s.id === selectedId)) return;
    setSelectedId(sortedSlots[0]?.id ?? null);
  }, [selectedId, sortedSlots]);

  const selected = sortedSlots.find((s) => s.id === selectedId) || sortedSlots[0] || null;
  const grouped = useMemo(() => {
    const groups = new Map<string, SlotDef[]>();
    for (const slot of sortedSlots) {
      const key = providerKey(slot);
      groups.set(key, [...(groups.get(key) || []), slot]);
    }
    return [...groups.entries()];
  }, [sortedSlots]);

  return (
    <div className="flex-1 grid grid-cols-[260px_minmax(0,1fr)_340px] min-h-0 overflow-hidden bg-neutral-950">
      <aside className="min-h-0 border-r border-neutral-800 flex flex-col">
        <div className="px-3 py-3 border-b border-neutral-800">
          <div className="flex items-center gap-2 text-sm font-medium text-neutral-200">
            <TerminalSquare className="w-4 h-4 text-cyan-400" />
            Workstation Cockpit
          </div>
          <div className="mt-1 text-[11px] text-neutral-500">
            V3 projected slots only
          </div>
        </div>
        <div className="flex-1 min-h-0 overflow-y-auto p-2 space-y-3">
          {grouped.map(([provider, providerSlots]) => (
            <section key={provider}>
              <div className="px-2 pb-1 text-[10px] uppercase tracking-wider text-neutral-600">
                {PROVIDER_LABELS[provider] || provider}
              </div>
              <div className="space-y-1">
                {providerSlots.map((slot) => (
                  <button
                    key={slot.id}
                    onClick={() => setSelectedId(slot.id)}
                    className={cn(
                      'w-full text-left rounded-md border px-2.5 py-2 transition-colors',
                      selected?.id === slot.id
                        ? 'border-cyan-500/40 bg-cyan-500/10'
                        : 'border-neutral-850 bg-neutral-900/40 hover:border-neutral-700 hover:bg-neutral-900',
                    )}
                  >
                    <div className="flex items-center gap-2 min-w-0">
                      <Circle className={cn('w-2.5 h-2.5 fill-current shrink-0', stateTone(slot))} />
                      <span className="text-xs text-neutral-200 truncate">{slot.label || slot.id}</span>
                    </div>
                    <div className="mt-1 flex items-center gap-2 text-[10px] text-neutral-500">
                      <span className="truncate">{stateLabel(slot)}</span>
                      {slot.activeBoardTaskId && <span className="text-amber-400/80">task</span>}
                      {slot.mcpReady === false && <span className="text-red-400">mcp</span>}
                    </div>
                  </button>
                ))}
              </div>
            </section>
          ))}
        </div>
      </aside>

      <main className="min-h-0 min-w-0 flex flex-col">
        {selected ? (
          <Terminal
            slotId={selected.id}
            slot={selected}
            activeTask={selected.activeBoardTaskId ? {
              id: selected.activeBoardTaskId,
              title: 'Active BoardTask',
              status: selected.state,
            } : null}
          />
        ) : (
          <div className="flex-1 flex items-center justify-center text-sm text-neutral-600">
            No workstation slots projected.
          </div>
        )}
      </main>

      <aside className="min-h-0 border-l border-neutral-800 flex flex-col bg-neutral-950">
        <div className="px-4 py-3 border-b border-neutral-800">
          <div className="flex items-center gap-2 text-sm font-medium text-neutral-200">
            <BrainCircuit className="w-4 h-4 text-purple-400" />
            Evidence
          </div>
          <div className="mt-1 text-[11px] text-neutral-500 truncate">
            {selected?.id || 'No slot selected'}
          </div>
        </div>

        <div className="flex-1 min-h-0 overflow-y-auto p-4 space-y-4 text-xs">
          <DiagnosticBlock icon={<Bot className="w-4 h-4" />} title="Provider">
            <KeyValue label="provider" value={selected?.provider || '-'} />
            <KeyValue label="engine" value={selected?.engine || '-'} />
            <KeyValue label="model" value={selected?.modelProfile || '-'} />
            <KeyValue label="task class" value={selected?.taskClass || '-'} />
          </DiagnosticBlock>

          <DiagnosticBlock icon={<Activity className="w-4 h-4" />} title="PTY Recognition">
            <KeyValue label="state" value={selected ? stateLabel(selected) : '-'} tone={selected ? stateTone(selected) : undefined} />
            <KeyValue label="confidence" value={selected?.confidence == null ? '-' : String(selected.confidence)} />
            <KeyValue label="tool" value={selected?.activeTool || '-'} />
            <KeyValue label="reason" value={selected?.reason || '-'} multiline />
          </DiagnosticBlock>

          <DiagnosticBlock icon={<CheckCircle2 className="w-4 h-4" />} title="Durable Conversation">
            <KeyValue label="id" value={selected?.latestConversation?.id || '-'} mono />
            <KeyValue label="source" value={selected?.latestConversation?.source || '-'} />
            <KeyValue label="messages" value={selected?.latestConversation?.messageCount == null ? '-' : String(selected.latestConversation.messageCount)} />
            <KeyValue label="updated" value={selected?.latestConversation?.updatedAt || '-'} />
          </DiagnosticBlock>

          <DiagnosticBlock icon={<Wrench className="w-4 h-4" />} title="Control Plane">
            <KeyValue label="mcp ready" value={selected?.mcpReady == null ? '-' : String(selected.mcpReady)} tone={selected?.mcpReady ? 'text-emerald-400' : 'text-amber-400'} />
            <KeyValue label="approval ready" value={selected?.mcpApprovalReady == null ? '-' : String(selected.mcpApprovalReady)} />
            <KeyValue label="active task" value={selected?.activeBoardTaskId || '-'} mono />
            {selected?.mcpApprovalMissingTools?.length ? (
              <div className="mt-2 flex items-start gap-2 rounded border border-amber-500/20 bg-amber-500/5 p-2 text-amber-300">
                <AlertTriangle className="w-3.5 h-3.5 mt-0.5 shrink-0" />
                <span>{selected.mcpApprovalMissingTools.join(', ')}</span>
              </div>
            ) : null}
          </DiagnosticBlock>
        </div>
      </aside>
    </div>
  );
}

function DiagnosticBlock({ icon, title, children }: { icon: ReactNode; title: string; children: ReactNode }) {
  return (
    <section className="rounded-md border border-neutral-800 bg-neutral-900/30 p-3">
      <div className="mb-3 flex items-center gap-2 text-neutral-300">
        <span className="text-neutral-500">{icon}</span>
        <span className="text-xs font-medium">{title}</span>
      </div>
      <div className="space-y-1.5">{children}</div>
    </section>
  );
}

function KeyValue({ label, value, tone, mono, multiline }: {
  label: string;
  value: string;
  tone?: string;
  mono?: boolean;
  multiline?: boolean;
}) {
  return (
    <div className={cn('grid grid-cols-[90px_minmax(0,1fr)] gap-2', multiline && 'items-start')}>
      <span className="text-neutral-600">{label}</span>
      <span className={cn('text-neutral-300 break-words', tone, mono && 'font-mono text-[11px]', !multiline && 'truncate')} title={value}>
        {value}
      </span>
    </div>
  );
}
