'use client';

import { useState, useEffect, useRef, useCallback, useMemo } from 'react';
import { Radar, Play, Pause, Clock, CheckCircle2, AlertCircle, Loader2, ChevronDown, ChevronUp } from 'lucide-react';
import { cn } from '@/lib/utils';
import { useTaskCenterStore } from '../store';
import { useEventInvalidation } from '../hooks/useEventStream';
import { FLOW_PHASE_LABELS, FLOW_PHASES } from '../constants';
import type { Task, FlowPhase, TaskNote, SlotDef } from '../types';

// ─── PTY Screen Hook (polls only for focused slot) ───
function usePtyScreen(slotId: string | null, enabled: boolean) {
  const [screen, setScreen] = useState('');
  const [loading, setLoading] = useState(false);

  useEffect(() => {
    if (!slotId || !enabled) { setScreen(''); return; }

    let cancelled = false;
    const fetchScreen = () => {
      if (document.visibilityState === 'hidden') return;
      fetch(`/api/pty/screen?slotId=${encodeURIComponent(slotId)}&lines=50`)
        .then(r => r.json())
        .then(data => { if (!cancelled) { setScreen(data.screen || ''); setLoading(false); } })
        .catch(() => { if (!cancelled) setLoading(false); });
    };

    setLoading(true);
    fetchScreen();
    const id = setInterval(fetchScreen, 3000);
    return () => { cancelled = true; clearInterval(id); };
  }, [slotId, enabled]);

  return { screen, loading };
}

// ─── Slot helpers ───
const INACTIVE_SLOT_STATES = new Set(['complete', 'completed', 'exited', 'not_running', 'stopped', 'dead', 'missing', 'error']);
function normalizedSlotState(slot: SlotDef): string | null {
  return typeof slot.state === 'string' && slot.state.trim()
    ? slot.state.trim().toLowerCase()
    : null;
}
function isSlotActive(slot: SlotDef): boolean {
  const state = normalizedSlotState(slot);
  return !!slot.running || (!!state && !INACTIVE_SLOT_STATES.has(state));
}
function slotStateLabel(slot: SlotDef): string {
  const state = normalizedSlotState(slot);
  if (state) return state;
  return slot.running ? 'running' : 'no session';
}
function slotMetaLine(slot: SlotDef): string {
  return [slot.provider, slot.engine, slot.taskClass]
    .filter((v): v is string => !!v)
    .join(' · ');
}
function slotTooltip(slot: SlotDef, task?: Task): string {
  const parts: string[] = [slot.id];
  if (slot.role) parts.push(`role: ${slot.role}`);
  if (slot.provider) parts.push(`provider: ${slot.provider}`);
  if (slot.engine) parts.push(`engine: ${slot.engine}`);
  if (slot.modelProfile) parts.push(`model: ${slot.modelProfile}`);
  if (slot.taskClass) parts.push(`taskClass: ${slot.taskClass}`);
  if (slot.acceptsBoardTask !== undefined) parts.push(`acceptsBoardTask: ${slot.acceptsBoardTask}`);
  parts.push(`state: ${slotStateLabel(slot)}`);
  if (slot.confidence != null) parts.push(`confidence: ${Math.round(slot.confidence * 100)}%`);
  if (slot.reason) parts.push(`reason: ${slot.reason}`);
  if (slot.activeTool) parts.push(`tool: ${slot.activeTool}`);
  if (slot.blockedKind) parts.push(`blockedKind: ${slot.blockedKind}`);
  if (slot.latestConversation?.title) parts.push(`latest: ${slot.latestConversation.title}`);
  if (task) parts.push(`task: ${task.id} ${task.title}`);
  return parts.join('\n');
}

// ─── Slot Card ───
function SlotCard({ slot, task, isSelected, onClick }: {
  slot: SlotDef;
  task?: Task;
  isSelected: boolean;
  onClick: () => void;
}) {
  const active = isSlotActive(slot);
  const stateColor = slot.running
    ? task?.status === 'running' ? 'bg-orange-400' : 'bg-emerald-400'
    : active
    ? 'bg-emerald-400/40'
    : 'bg-neutral-600';
  const meta = slotMetaLine(slot);
  const stateLabel = slotStateLabel(slot);

  return (
    <button
      onClick={onClick}
      title={slotTooltip(slot, task)}
      className={cn(
        'flex flex-col gap-1 p-3 rounded-lg border text-left transition-all min-w-[160px] max-w-[240px]',
        isSelected
          ? 'border-orange-500/40 bg-orange-500/5'
          : 'border-neutral-800 bg-neutral-900/50 hover:border-neutral-700',
      )}
    >
      <div className="flex items-center gap-2 min-w-0">
        <div className={cn('w-2 h-2 rounded-full shrink-0', stateColor, slot.running && task?.status === 'running' && 'animate-pulse')} />
        <span className="text-xs font-mono text-neutral-300 truncate min-w-0 flex-1">{slot.label}</span>
        <span className="text-[10px] text-neutral-600 shrink-0">{slot.role}</span>
      </div>
      {meta && (
        <p className="text-[9px] text-neutral-600 truncate" title={meta}>
          {meta}
        </p>
      )}
      {task ? (
        <p className="text-[10px] text-neutral-500 truncate" title={task.title}>
          {task.title}
        </p>
      ) : (
        <p className="text-[10px] text-neutral-600 italic truncate">{stateLabel}</p>
      )}
    </button>
  );
}

// ─── Flow Stepper ───
function FlowStepper({ phase }: { phase?: FlowPhase }) {
  if (!phase) {
    return (
      <div className="flex items-center gap-1.5">
        <div className="w-2 h-2 rounded-full bg-orange-400 animate-pulse" />
        <span className="text-[10px] text-neutral-500">running...</span>
      </div>
    );
  }

  const currentIdx = FLOW_PHASES.indexOf(phase);

  return (
    <div className="flex items-center gap-0.5">
      {FLOW_PHASES.map((p, i) => {
        const isDone = i < currentIdx || phase === 'done';
        const isCurrent = i === currentIdx && phase !== 'done';
        return (
          <div key={p} className="flex items-center">
            <div className="flex flex-col items-center">
              <div
                className={cn(
                  'w-2.5 h-2.5 rounded-full transition-colors',
                  isDone ? 'bg-emerald-500' : isCurrent ? 'bg-orange-400 animate-pulse' : 'bg-neutral-700',
                )}
              />
              <span className={cn(
                'text-[8px] mt-0.5',
                isDone ? 'text-emerald-500' : isCurrent ? 'text-orange-400' : 'text-neutral-600',
              )}>
                {FLOW_PHASE_LABELS[p]}
              </span>
            </div>
            {i < FLOW_PHASES.length - 1 && (
              <div className={cn(
                'w-3 h-px mx-0.5 mt-[-8px]',
                isDone ? 'bg-emerald-500/50' : 'bg-neutral-700',
              )} />
            )}
          </div>
        );
      })}
    </div>
  );
}

// ─── PTY Preview (with smart auto-scroll) ───
function PtyPreview({ screen, loading }: { screen: string; loading: boolean }) {
  const preRef = useRef<HTMLPreElement>(null);
  const isAtBottom = useRef(true);

  const handleScroll = useCallback(() => {
    if (!preRef.current) return;
    const el = preRef.current;
    isAtBottom.current = el.scrollHeight - el.scrollTop - el.clientHeight < 50;
  }, []);

  useEffect(() => {
    if (preRef.current && isAtBottom.current) {
      preRef.current.scrollTop = preRef.current.scrollHeight;
    }
  }, [screen]);

  if (!screen && !loading) {
    return (
      <div className="flex items-center justify-center h-full text-neutral-600 text-xs">
        Select a running slot to preview
      </div>
    );
  }

  return (
    <div className="relative h-full">
      {loading && !screen && (
        <div className="absolute inset-0 flex items-center justify-center">
          <Loader2 className="w-4 h-4 animate-spin text-neutral-500" />
        </div>
      )}
      <pre
        ref={preRef}
        onScroll={handleScroll}
        className="h-full overflow-auto p-3 font-mono text-[11px] leading-relaxed text-green-400 bg-black/50 whitespace-pre-wrap break-all"
      >
        {screen}
      </pre>
    </div>
  );
}

// ─── Task Flow Card ───
function TaskFlowCard({ task, isSelected, onClick }: {
  task: Task;
  isSelected: boolean;
  onClick: () => void;
}) {
  const [notes, setNotes] = useState<TaskNote[]>([]);
  const [expanded, setExpanded] = useState(false);

  // Fetch notes on expand (notes come inline from board_get)
  useEffect(() => {
    if (!expanded) return;
    fetch(`/api/tasks?id=${task.id}`)
      .then(r => r.json())
      .then(data => { if (Array.isArray(data?.notes)) setNotes(data.notes); })
      .catch(() => {});
  }, [expanded, task.id]);

  const latestNote = notes[notes.length - 1];
  const statusIcon = task.status === 'running'
    ? <Play className="w-3 h-3 text-orange-400" />
    : task.status === 'done'
    ? <CheckCircle2 className="w-3 h-3 text-emerald-400" />
    : task.status === 'failed'
    ? <AlertCircle className="w-3 h-3 text-red-400" />
    : <Clock className="w-3 h-3 text-neutral-500" />;

  return (
    <div
      className={cn(
        'rounded-lg border p-3 transition-all cursor-pointer',
        isSelected
          ? 'border-orange-500/30 bg-orange-500/5'
          : 'border-neutral-800 bg-neutral-900/30 hover:border-neutral-700',
      )}
      onClick={onClick}
    >
      <div className="flex items-center gap-2 mb-2">
        {statusIcon}
        <span className="text-xs text-neutral-300 font-medium truncate flex-1">{task.title}</span>
        {task.assignee && (
          <span className="text-[10px] font-mono text-neutral-600">{task.assignee}</span>
        )}
        <span className={cn(
          'text-[10px] px-1.5 py-0.5 rounded',
          task.priority === 'high' ? 'bg-red-500/10 text-red-400'
            : task.priority === 'medium' ? 'bg-amber-500/10 text-amber-400'
            : 'bg-neutral-800 text-neutral-500',
        )}>
          {task.priority}
        </span>
      </div>

      <FlowStepper phase={task.flowPhase} />

      {/* Latest note preview */}
      {latestNote && !expanded && (
        <p className="text-[10px] text-neutral-500 mt-2 truncate">
          💬 {latestNote.content.slice(0, 120)}
        </p>
      )}

      {/* Expand/collapse notes */}
      <button
        onClick={(e) => { e.stopPropagation(); setExpanded(!expanded); }}
        className="flex items-center gap-1 mt-1.5 text-[10px] text-neutral-600 hover:text-neutral-400 transition-colors"
      >
        {expanded ? <ChevronUp className="w-3 h-3" /> : <ChevronDown className="w-3 h-3" />}
        {notes.length > 0 ? `${notes.length} notes` : 'notes'}
      </button>

      {expanded && notes.length > 0 && (
        <div className="mt-2 space-y-1.5 max-h-40 overflow-y-auto">
          {notes.map(n => (
            <div key={n.id} className="text-[10px] text-neutral-500 border-l-2 border-neutral-800 pl-2">
              <span className="text-neutral-600">{n.author || 'system'}</span>
              {' · '}
              <span className="text-neutral-700">{new Date(n.createdAt).toLocaleTimeString()}</span>
              <p className="mt-0.5 whitespace-pre-wrap">{n.content}</p>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}

// ─── Main Component ───
export function AutopilotMonitor({ slots }: { slots: SlotDef[] }) {
  const tasks = useTaskCenterStore(s => s.tasks);
  const fetchTasks = useTaskCenterStore(s => s.fetchTasks);
  const slotVersion = useEventInvalidation('slot');
  const taskVersion = useEventInvalidation('task');

  // Refetch tasks when WS events bump version
  useEffect(() => {
    if (taskVersion === 0) return;
    fetchTasks();
  }, [taskVersion, fetchTasks]);

  // Autopilot tasks: auto_execute OR has assignee, not done/skipped
  const autopilotTasks = useMemo(() =>
    tasks.filter(t =>
      (t.autoExecute || t.assignee) &&
      !['done', 'skipped'].includes(t.status)
    ).sort((a, b) => {
      // Running first, then by priority
      if (a.status === 'running' && b.status !== 'running') return -1;
      if (b.status === 'running' && a.status !== 'running') return 1;
      const prio = { high: 0, medium: 1, low: 2 };
      return (prio[a.priority] ?? 1) - (prio[b.priority] ?? 1);
    }),
    [tasks],
  );

  const runningTasks = useMemo(() =>
    autopilotTasks.filter(t => t.status === 'running'),
    [autopilotTasks],
  );

  // Active slot for PTY preview
  const [selectedSlot, setSelectedSlot] = useState<string | null>(null);

  // Map slot → running BoardTask (correlate via assignee or claimExecutorId).
  const slotTaskMap = useMemo(() => {
    const map = new Map<string, Task>();
    for (const t of runningTasks) {
      if (t.assignee || t.claimExecutorId) {
        map.set(t.assignee || t.claimExecutorId!, t);
      }
    }
    return map;
  }, [runningTasks]);

  // Auto-select preference: slot-with-running-BoardTask → active non-complete slot → any slot.
  useEffect(() => {
    if (selectedSlot) {
      const cur = slots.find(s => s.id === selectedSlot);
      if (cur && (cur.running || isSlotActive(cur) || slotTaskMap.has(cur.id))) return;
    }
    const slotWithTask = slots.find(s => slotTaskMap.has(s.id));
    if (slotWithTask) { setSelectedSlot(slotWithTask.id); return; }
    const active = slots.find(isSlotActive);
    if (active) { setSelectedSlot(active.id); return; }
    setSelectedSlot(slots[0]?.id ?? null);
  }, [slots, selectedSlot, slotTaskMap]);

  // PTY screen for selected slot
  const { screen, loading: screenLoading } = usePtyScreen(selectedSlot, true);

  const selectedSlotData = useMemo(
    () => (selectedSlot ? slots.find(s => s.id === selectedSlot) ?? null : null),
    [selectedSlot, slots],
  );

  // Resolve the slot's current task — prefer the BoardTask correlation, fall back to the
  // slot's own currentTaskId surfaced from the daemon (covers non-autopilot tasks).
  const selectedTask = useMemo(() => {
    if (!selectedSlot) return null;
    const fromBoard = slotTaskMap.get(selectedSlot);
    if (fromBoard) return fromBoard;
    const currentTaskId = selectedSlotData?.activeBoardTaskId ?? selectedSlotData?.currentTaskId;
    if (!currentTaskId) return null;
    return tasks.find(t => t.id === currentTaskId) ?? null;
  }, [selectedSlot, selectedSlotData, slotTaskMap, tasks]);

  const runningCount = slots.filter(s => s.running).length;

  return (
    <div className="flex-1 flex flex-col overflow-hidden px-4 sm:px-8 pb-4">
      {/* Header bar */}
      <div className="flex items-center gap-3 py-3">
        <Radar className="w-4 h-4 text-orange-400" />
        <span className="text-sm font-medium text-neutral-300">Autopilot Monitor</span>
        <span className="text-xs text-neutral-600">
          running: {runningCount}/{slots.length}
        </span>
        <span className="text-xs text-neutral-600">·</span>
        <span className="text-xs text-neutral-600">
          {autopilotTasks.length} tasks ({runningTasks.length} active)
        </span>
        {slotVersion > 0 && (
          <div className="ml-auto flex items-center gap-1 text-[10px] text-neutral-700">
            <div className="w-1.5 h-1.5 rounded-full bg-emerald-500/60" />
            live
          </div>
        )}
      </div>

      {/* Two-column layout */}
      <div className="flex-1 flex gap-4 min-h-0">
        {/* Left: Slot cards + Task list */}
        <div className="flex-1 flex flex-col gap-4 min-h-0 overflow-auto">
          {/* Slot cards row */}
          <div className="flex gap-2 flex-wrap">
            {slots.map(slot => (
              <SlotCard
                key={slot.id}
                slot={slot}
                task={slotTaskMap.get(slot.id)}
                isSelected={selectedSlot === slot.id}
                onClick={() => setSelectedSlot(slot.id)}
              />
            ))}
          </div>

          {/* Running + queued tasks */}
          <div className="flex-1 overflow-auto space-y-2">
            {autopilotTasks.length === 0 ? (
              <div className="flex items-center justify-center h-32 text-neutral-600 text-xs">
                <Pause className="w-4 h-4 mr-2" />
                No autopilot tasks
              </div>
            ) : (
              autopilotTasks.map(task => (
                <TaskFlowCard
                  key={task.id}
                  task={task}
                  isSelected={task.assignee === selectedSlot || task.claimExecutorId === selectedSlot}
                  onClick={() => {
                    if (task.assignee) setSelectedSlot(task.assignee);
                  }}
                />
              ))
            )}
          </div>
        </div>

        {/* Right: PTY screen preview */}
        <div className="w-[420px] flex-shrink-0 rounded-lg border border-neutral-800 overflow-hidden bg-black/30 flex flex-col">
          <div className="flex items-center gap-2 px-3 py-1.5 border-b border-neutral-800 bg-neutral-900/50 shrink-0">
            <div className={cn(
              'w-2 h-2 rounded-full shrink-0',
              selectedSlotData?.running
                ? 'bg-emerald-400'
                : selectedSlotData && isSlotActive(selectedSlotData)
                ? 'bg-emerald-400/40'
                : 'bg-neutral-600',
            )} />
            <span className="text-[10px] font-mono text-neutral-500 truncate" title={selectedSlot ?? ''}>
              {selectedSlot || 'no slot selected'}
            </span>
            {selectedSlotData?.state && (
              <span className="text-[10px] text-neutral-600 shrink-0">· {selectedSlotData.state}</span>
            )}
            {screenLoading && <Loader2 className="w-3 h-3 animate-spin text-neutral-600 ml-auto shrink-0" />}
          </div>
          {selectedSlotData && (
            <div className="px-3 py-1.5 border-b border-neutral-800 bg-neutral-950/50 space-y-0.5 shrink-0">
              {selectedTask && (
                <div className="text-[10px] text-neutral-300 truncate" title={`${selectedTask.id}\n${selectedTask.title}`}>
                  <span className="font-mono text-neutral-500">{selectedTask.id.slice(0, 8)}</span>
                  <span className="ml-1.5">{selectedTask.title}</span>
                </div>
              )}
              {(selectedSlotData.provider || selectedSlotData.engine || selectedSlotData.modelProfile || selectedSlotData.taskClass) && (
                <div className="flex items-center gap-1.5 text-[10px] text-neutral-500 flex-wrap">
                  {selectedSlotData.provider && <span>{selectedSlotData.provider}</span>}
                  {selectedSlotData.engine && <span className="text-neutral-600">· {selectedSlotData.engine}</span>}
                  {selectedSlotData.modelProfile && <span className="text-neutral-600">· {selectedSlotData.modelProfile}</span>}
                  {selectedSlotData.taskClass && <span className="text-neutral-600">· {selectedSlotData.taskClass}</span>}
                  {selectedSlotData.acceptsBoardTask && (
                    <span className="ml-auto text-emerald-500/70 text-[9px]">board ✓</span>
                  )}
                </div>
              )}
              {(selectedSlotData.confidence != null || selectedSlotData.reason) && (
                <div className="text-[10px] text-neutral-600 truncate" title={selectedSlotData.reason ?? ''}>
                  {selectedSlotData.confidence != null && (
                    <span className="font-mono">{Math.round(selectedSlotData.confidence * 100)}%</span>
                  )}
                  {selectedSlotData.reason && (
                    <span className={selectedSlotData.confidence != null ? 'ml-1.5' : ''}>{selectedSlotData.reason}</span>
                  )}
                </div>
              )}
              {selectedSlotData.activeTool && (
                <div className="text-[10px] text-neutral-600 truncate" title={selectedSlotData.activeTool}>
                  tool: <span className="font-mono">{selectedSlotData.activeTool}</span>
                </div>
              )}
              {selectedSlotData.latestConversation?.title && (
                <div className="text-[10px] text-neutral-600 truncate" title={selectedSlotData.latestConversation.title}>
                  conv: {selectedSlotData.latestConversation.title}
                </div>
              )}
            </div>
          )}
          <div className="flex-1 min-h-0">
            <PtyPreview screen={screen} loading={screenLoading} />
          </div>
        </div>
      </div>
    </div>
  );
}
