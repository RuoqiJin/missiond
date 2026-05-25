'use client';

import { useEffect, useMemo, useState } from 'react';
import type { ReactNode } from 'react';
import {
  BrainCircuit,
  CheckCircle2,
  Circle,
  GitBranch,
  ListChecks,
  Radio,
  TerminalSquare,
} from 'lucide-react';
import { Terminal } from './Terminal';
import { fetchTaskWithNotes } from '../api';
import type { SlotDef, Task, TaskNote } from '../types';
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

function stateTone(slot?: SlotDef | null) {
  const state = (slot?.state || '').toLowerCase();
  if (slot?.running || ['running', 'thinking', 'responding', 'tool_running', 'confirming', 'blocked'].includes(state)) {
    return 'text-emerald-400';
  }
  if (['idle', 'slash_menu'].includes(state)) return 'text-blue-400';
  if (['exited', 'error', 'stopped', 'not_running'].includes(state)) return 'text-neutral-600';
  return 'text-amber-400';
}

function stateLabel(slot?: SlotDef | null) {
  if (!slot) return '-';
  if (slot.blockedKind) return `blocked:${slot.blockedKind}`;
  return slot.state || (slot.running ? 'running' : 'unknown');
}

function taskStatusRank(task: Task) {
  if (task.status === 'running') return 0;
  if (task.status === 'blocked') return 1;
  if (task.status === 'open') return 2;
  if (task.status === 'verifying') return 3;
  return 4;
}

function slotForTask(task: Task | null, slots: SlotDef[]) {
  if (!task) return null;
  return slots.find((slot) =>
    slot.activeBoardTaskId === task.id
    || slot.currentTaskId === task.id
    || task.claimExecutorId === slot.id
    || task.assignee === slot.id
  ) ?? null;
}

function formatTime(value?: string | null) {
  if (!value) return '-';
  const date = new Date(value);
  return Number.isNaN(date.getTime()) ? value : date.toLocaleString();
}

function extractExecutionStepDigest(task: Task | null, notes: TaskNote[], slot: SlotDef | null): string[] {
  const sources = [
    task?.description,
    ...notes
      .slice()
      .sort((a, b) => b.createdAt.localeCompare(a.createdAt))
      .map((note) => note.content),
  ].filter((value): value is string => !!value);
  const stepLike: string[] = [];
  const seen = new Set<string>();
  for (const source of sources) {
    for (const rawLine of source.split(/\r?\n/)) {
      const line = rawLine.replace(/^[-*\s#>]+/, '').trim();
      if (!line) continue;
      if (!/(step|phase|stage|acceptance|验收|阶段|步骤|evidence|verification|blocked|done|failed|summary)/i.test(line)) {
        continue;
      }
      const compact = line.length > 160 ? `${line.slice(0, 159)}...` : line;
      if (seen.has(compact)) continue;
      seen.add(compact);
      stepLike.push(compact);
      if (stepLike.length >= 12) return stepLike;
    }
  }
  return [
    task ? `BoardTask ${task.status}: ${task.title}` : 'No BoardTask selected.',
    slot ? `slot ${slot.id}: ${stateLabel(slot)}` : 'No linked slot.',
    slot?.latestConversation?.id
      ? `conversation ${slot.latestConversation.id}: ${slot.latestConversation.status || 'unknown'}`
      : 'No durable conversation linked.',
  ];
}

function metadataValue(value: unknown): string {
  if (value == null) return '-';
  if (typeof value === 'string') return value;
  if (typeof value === 'number' || typeof value === 'boolean') return String(value);
  if (Array.isArray(value)) return value.map(metadataValue).join(', ');
  try {
    return JSON.stringify(value);
  } catch {
    return String(value);
  }
}

function interactionChainRows(task: Task | null): Array<[string, string]> {
  const meta = task?.runtimeMetadata || {};
  const keys = [
    'interaction_id',
    'permission_context',
    'grounding_context_id',
    'intent_artifact_id',
    'plan_artifact_id',
    'accepted_shard_id',
    'context_pack_path',
    'write_scope',
    'sources_used',
  ];
  return keys
    .filter((key) => Object.prototype.hasOwnProperty.call(meta, key))
    .map((key) => [key, metadataValue(meta[key])]);
}

export function ExecDashboard({ slots, tasks }: { slots: SlotDef[]; tasks: Task[] }) {
  const [selectedTaskId, setSelectedTaskId] = useState<string | null>(null);
  const [selectedSlotId, setSelectedSlotId] = useState<string | null>(null);
  const [selectedTaskNotes, setSelectedTaskNotes] = useState<TaskNote[]>([]);

  const activeTasks = useMemo(() => {
    return tasks
      .filter((task) => ['running', 'blocked', 'open', 'verifying'].includes(task.status))
      .sort((a, b) => taskStatusRank(a) - taskStatusRank(b) || b.updatedAt.localeCompare(a.updatedAt));
  }, [tasks]);

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
    if (selectedTaskId && activeTasks.some((task) => task.id === selectedTaskId)) return;
    setSelectedTaskId(activeTasks[0]?.id ?? null);
  }, [activeTasks, selectedTaskId]);

  const selectedTask = activeTasks.find((task) => task.id === selectedTaskId) ?? activeTasks[0] ?? null;
  const taskSlot = slotForTask(selectedTask, sortedSlots);
  const selectedSlot = sortedSlots.find((slot) => slot.id === selectedSlotId) ?? taskSlot ?? sortedSlots[0] ?? null;
  const stepDigest = useMemo(
    () => extractExecutionStepDigest(selectedTask, selectedTaskNotes, selectedSlot),
    [selectedTask, selectedTaskNotes, selectedSlot],
  );
  const chainRows = useMemo(() => interactionChainRows(selectedTask), [selectedTask]);

  useEffect(() => {
    if (selectedSlotId && sortedSlots.some((slot) => slot.id === selectedSlotId)) return;
    if (taskSlot) {
      setSelectedSlotId(taskSlot.id);
      return;
    }
    setSelectedSlotId(sortedSlots[0]?.id ?? null);
  }, [selectedSlotId, sortedSlots, taskSlot]);

  useEffect(() => {
    let cancelled = false;
    setSelectedTaskNotes([]);
    if (!selectedTask?.id) return;
    fetchTaskWithNotes(selectedTask.id)
      .then((taskWithNotes) => {
        if (!cancelled) setSelectedTaskNotes(taskWithNotes.notes || []);
      })
      .catch(() => {
        if (!cancelled) setSelectedTaskNotes([]);
      });
    return () => {
      cancelled = true;
    };
  }, [selectedTask?.id]);

  return (
    <div className="flex-1 grid grid-cols-[320px_minmax(0,1fr)_430px] min-h-0 overflow-hidden bg-neutral-950">
      <aside className="min-h-0 border-r border-neutral-800 flex flex-col">
        <div className="px-3 py-3 border-b border-neutral-800">
          <div className="flex items-center gap-2 text-sm font-medium text-neutral-200">
            <ListChecks className="w-4 h-4 text-orange-400" />
            Execution Queue
          </div>
          <div className="mt-1 text-[11px] text-neutral-500">
            BoardTask first, PTY as evidence
          </div>
        </div>
        <div className="flex-1 min-h-0 overflow-y-auto p-2 space-y-1">
          {activeTasks.length === 0 ? (
            <div className="rounded-md border border-neutral-850 bg-neutral-900/40 p-3 text-xs text-neutral-600">
              No active BoardTasks.
            </div>
          ) : activeTasks.map((task) => {
            const slot = slotForTask(task, sortedSlots);
            const active = selectedTask?.id === task.id;
            return (
              <button
                key={task.id}
                onClick={() => {
                  setSelectedTaskId(task.id);
                  if (slot) setSelectedSlotId(slot.id);
                }}
                className={cn(
                  'w-full rounded-md border px-2.5 py-2 text-left transition-colors',
                  active
                    ? 'border-orange-500/40 bg-orange-500/10 text-orange-100'
                    : 'border-neutral-850 bg-neutral-900/35 text-neutral-400 hover:border-neutral-700 hover:bg-neutral-900',
                )}
              >
                <div className="flex min-w-0 items-center gap-2">
                  <Circle className={cn('w-2.5 h-2.5 fill-current shrink-0', task.status === 'running' ? 'text-emerald-400' : task.status === 'blocked' ? 'text-amber-400' : 'text-neutral-600')} />
                  <span className="truncate text-xs font-medium">{task.title}</span>
                </div>
                <div className="mt-1 flex items-center gap-2 text-[10px] text-neutral-600">
                  <span className="font-mono">#{task.id.slice(0, 8)}</span>
                  <span>{task.status}</span>
                  {slot ? <span className="truncate text-neutral-500">{slot.label || slot.id}</span> : null}
                </div>
              </button>
            );
          })}
        </div>
      </aside>

      <main className="min-h-0 min-w-0 flex flex-col border-r border-neutral-800">
        <div className="px-4 py-3 border-b border-neutral-800">
          <div className="flex items-center gap-2 text-sm font-medium text-neutral-200">
            <GitBranch className="w-4 h-4 text-cyan-400" />
            Execution Evidence
          </div>
          <div className="mt-1 text-[11px] text-neutral-500 truncate">
            {selectedTask ? `${selectedTask.project || 'missiond'} · ${selectedTask.status}` : 'No task selected'}
          </div>
        </div>
        <div className="flex-1 min-h-0 overflow-y-auto p-4 space-y-4">
          <DiagnosticBlock icon={<BrainCircuit className="w-4 h-4" />} title="BoardTask">
            <KeyValue label="id" value={selectedTask?.id || '-'} mono />
            <KeyValue label="title" value={selectedTask?.title || '-'} multiline />
            <KeyValue label="status" value={selectedTask?.status || '-'} />
            <KeyValue label="project" value={selectedTask?.project || '-'} />
            <KeyValue label="updated" value={formatTime(selectedTask?.updatedAt)} />
          </DiagnosticBlock>

          <DiagnosticBlock icon={<Radio className="w-4 h-4" />} title="EventBus Wait State">
            <KeyValue label="driver" value="BoardEvent / SlotEvent / Conversation final" />
            <KeyValue label="fallback" value="bounded HTTP refresh only" />
            <KeyValue label="slot" value={taskSlot?.id || '-'} mono />
            <KeyValue label="slot state" value={stateLabel(taskSlot)} tone={stateTone(taskSlot)} />
          </DiagnosticBlock>

          <DiagnosticBlock icon={<GitBranch className="w-4 h-4" />} title="Interaction Chain">
            {chainRows.length > 0 ? (
              chainRows.map(([label, value]) => (
                <KeyValue key={label} label={label} value={value} mono multiline />
              ))
            ) : (
              <KeyValue label="metadata" value="No runtime_metadata chain recorded." />
            )}
          </DiagnosticBlock>

          <DiagnosticBlock icon={<ListChecks className="w-4 h-4" />} title="Execution Step Digest">
            <div className="max-h-48 overflow-y-auto rounded border border-neutral-850 bg-neutral-950/50">
              {stepDigest.map((line, index) => (
                <div key={`${index}-${line}`} className="grid grid-cols-[34px_minmax(0,1fr)] gap-2 border-b border-neutral-900 px-2 py-1.5 last:border-b-0">
                  <span className="font-mono text-[10px] text-neutral-600">s{index + 1}</span>
                  <span className="text-xs text-neutral-300 break-words">{line}</span>
                </div>
              ))}
            </div>
          </DiagnosticBlock>

          <DiagnosticBlock icon={<CheckCircle2 className="w-4 h-4" />} title="Durable Conversation">
            <KeyValue label="id" value={selectedSlot?.latestConversation?.id || '-'} mono />
            <KeyValue label="source" value={selectedSlot?.latestConversation?.source || '-'} />
            <KeyValue label="status" value={selectedSlot?.latestConversation?.status || '-'} />
            <KeyValue label="messages" value={selectedSlot?.latestConversation?.messageCount == null ? '-' : String(selectedSlot.latestConversation.messageCount)} />
            <KeyValue label="updated" value={formatTime(selectedSlot?.latestConversation?.updatedAt)} />
          </DiagnosticBlock>
        </div>
      </main>

      <aside className="min-h-0 flex flex-col bg-neutral-950">
        <div className="px-4 py-3 border-b border-neutral-800">
          <div className="flex items-center gap-2 text-sm font-medium text-neutral-200">
            <TerminalSquare className="w-4 h-4 text-purple-400" />
            PTY Detail
          </div>
          <div className="mt-1 text-[11px] text-neutral-500">
            Diagnostic, not completion authority
          </div>
        </div>
        <div className="max-h-44 overflow-y-auto border-b border-neutral-800 p-2">
          <div className="grid grid-cols-1 gap-1">
            {sortedSlots.map((slot) => (
              <button
                key={slot.id}
                onClick={() => setSelectedSlotId(slot.id)}
                className={cn(
                  'rounded-md border px-2 py-1.5 text-left transition-colors',
                  selectedSlot?.id === slot.id
                    ? 'border-purple-500/40 bg-purple-500/10'
                    : 'border-neutral-850 bg-neutral-900/35 hover:border-neutral-700',
                )}
              >
                <div className="flex items-center gap-2 min-w-0">
                  <Circle className={cn('w-2.5 h-2.5 fill-current shrink-0', stateTone(slot))} />
                  <span className="truncate text-xs text-neutral-200">{slot.label || slot.id}</span>
                  <span className="ml-auto text-[10px] text-neutral-500">{PROVIDER_LABELS[providerKey(slot)] || providerKey(slot)}</span>
                </div>
                <div className="mt-0.5 text-[10px] text-neutral-600 truncate">{stateLabel(slot)}</div>
              </button>
            ))}
          </div>
        </div>
        <div className="min-h-0 flex-1">
          {selectedSlot ? (
            <Terminal
              slotId={selectedSlot.id}
              slot={selectedSlot}
              activeTask={selectedTask ? {
                id: selectedTask.id,
                title: selectedTask.title,
                status: selectedTask.status,
              } : null}
            />
          ) : (
            <div className="flex h-full items-center justify-center text-sm text-neutral-600">
              No workstation slots projected.
            </div>
          )}
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
    <div className={cn('grid grid-cols-[92px_minmax(0,1fr)] gap-2 text-xs', multiline && 'items-start')}>
      <span className="text-neutral-600">{label}</span>
      <span className={cn('text-neutral-300 break-words', tone, mono && 'font-mono text-[11px]', !multiline && 'truncate')} title={value}>
        {value}
      </span>
    </div>
  );
}
