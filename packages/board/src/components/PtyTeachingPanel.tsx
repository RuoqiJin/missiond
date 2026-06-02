'use client';

import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import {
  ArrowDown,
  ArrowUp,
  CornerDownLeft,
  Eraser,
  Gauge,
  Keyboard,
  Play,
  Power,
  RefreshCw,
  RotateCcw,
  Send,
  Slash,
  Square,
  X,
} from 'lucide-react';
import { Terminal } from './Terminal';
import { cn } from '@/lib/utils';
import type { SlotDef } from '../types';

interface PtyTeachingPanelProps {
  slots: SlotDef[];
  refreshSlots?: () => void;
}

interface PtyStatus {
  running?: boolean;
  state?: string;
  reason?: string;
  recognition?: {
    state?: string;
    reason?: string;
    confidence?: number;
  };
}

interface TeachingStep {
  id: number;
  at: string;
  action: string;
  result: 'sent' | 'failed' | 'observed';
  state?: string;
  reason?: string;
  screenTail?: string;
}

const SAFE_KEY_ACTIONS = [
  { key: 'enter', label: 'Enter', icon: CornerDownLeft },
  { key: 'escape', label: 'Esc', icon: X },
  { key: 'up', label: 'Up', icon: ArrowUp },
  { key: 'down', label: 'Down', icon: ArrowDown },
] as const;

const SAFE_TEXT_ACTIONS = [
  { text: '/model', label: '/model', icon: Slash },
  { text: '/usage', label: '/usage', icon: Gauge },
  { text: '/clear', label: '/clear', icon: Eraser },
] as const;

const FALLBACK_AGY_SLOT: SlotDef = {
  id: 'slot-agy-research',
  label: 'slot-agy-research',
  role: 'researcher',
  provider: 'agy',
  engine: 'agy',
};

const FALLBACK_CLAUDE_CODE_SLOT: SlotDef = {
  id: 'slot-claude-code-default',
  label: 'slot-claude-code-default',
  role: 'coder',
  provider: 'claude_code',
  engine: 'claude_code',
};

const FALLBACK_CODEX_SLOT: SlotDef = {
  id: 'slot-codex-code-worker',
  label: 'slot-codex-code-worker',
  role: 'coder',
  provider: 'codex',
  engine: 'codex',
};

const DEFAULT_TEACH_SLOT_ID = FALLBACK_CLAUDE_CODE_SLOT.id;
const TEACHABLE_ENGINES = new Set(['agy', 'codex', 'claude_code', 'claude-code', 'gemini']);

function normalizedSlotText(slot: SlotDef) {
  return `${slot.id} ${slot.provider || ''} ${slot.engine || ''} ${slot.label || ''}`.toLowerCase();
}

function isTeachableCliSlot(slot: SlotDef) {
  const engine = `${slot.engine || slot.provider || ''}`.toLowerCase();
  const text = normalizedSlotText(slot);
  return TEACHABLE_ENGINES.has(engine) || [...TEACHABLE_ENGINES].some((value) => text.includes(value));
}

function sortTeachSlots(a: SlotDef, b: SlotDef) {
  const order = (slot: SlotDef) => {
    const text = normalizedSlotText(slot);
    if (slot.id === DEFAULT_TEACH_SLOT_ID) return 0;
    if (text.includes('claude')) return 1;
    if (text.includes('codex')) return 2;
    if (text.includes('agy')) return 3;
    if (text.includes('gemini')) return 4;
    return 5;
  };
  return order(a) - order(b) || a.id.localeCompare(b.id);
}

function statusState(status: PtyStatus | null, slot?: SlotDef | null) {
  return status?.state || status?.recognition?.state || slot?.state || (slot?.running ? 'running' : 'stopped');
}

function statusReason(status: PtyStatus | null, slot?: SlotDef | null) {
  return status?.reason || status?.recognition?.reason || slot?.reason || null;
}

function compactScreenTail(screen: string) {
  const lines = screen
    .replace(/\r/g, '')
    .split('\n')
    .map((line) => line.trim())
    .filter(Boolean);
  return lines.slice(-5).join(' / ').slice(0, 360);
}

function stateTone(state: string | null | undefined) {
  const normalized = (state || '').toLowerCase();
  if (['idle', 'slash_menu', 'running'].includes(normalized)) return 'text-emerald-300';
  if (['starting', 'thinking', 'responding', 'tool_running'].includes(normalized)) return 'text-sky-300';
  if (['confirming', 'blocked'].includes(normalized)) return 'text-amber-300';
  if (['complete', 'completed', 'exited', 'not_running', 'stopped', 'dead', 'missing', 'error'].includes(normalized)) return 'text-red-300';
  return 'text-neutral-400';
}

function isDangerousText(value: string) {
  return ['/exit', '/quit', 'exit', 'quit'].includes(value.trim().toLowerCase());
}

export function PtyTeachingPanel({ slots, refreshSlots }: PtyTeachingPanelProps) {
  const teachSlots = useMemo(() => {
    const projected = slots.filter(isTeachableCliSlot).sort(sortTeachSlots);
    return projected.length > 0 ? projected : [FALLBACK_CLAUDE_CODE_SLOT, FALLBACK_CODEX_SLOT, FALLBACK_AGY_SLOT];
  }, [slots]);
  const [slotId, setSlotId] = useState(() => {
    if (typeof window === 'undefined') return DEFAULT_TEACH_SLOT_ID;
    return localStorage.getItem('board:teachSlot') || DEFAULT_TEACH_SLOT_ID;
  });
  const [status, setStatus] = useState<PtyStatus | null>(null);
  const [text, setText] = useState('');
  const [busy, setBusy] = useState(false);
  const [dangerArmed, setDangerArmed] = useState<string | null>(null);
  const [steps, setSteps] = useState<TeachingStep[]>([]);
  const [screenText, setScreenText] = useState('');
  const [controlsCollapsed, setControlsCollapsed] = useState(() => {
    if (typeof window === 'undefined') return true;
    return localStorage.getItem('board:teachControlsCollapsed') !== 'false';
  });
  const [keepAlive, setKeepAlive] = useState(() => {
    if (typeof window === 'undefined') return true;
    return localStorage.getItem('board:teachKeepAlive') !== 'false';
  });
  const keepAliveInflightRef = useRef(false);
  const lastKeepAliveSpawnAtRef = useRef(0);

  const selectedSlot = useMemo(
    () => teachSlots.find((slot) => slot.id === slotId)
      ?? teachSlots.find((slot) => slot.id === DEFAULT_TEACH_SLOT_ID)
      ?? teachSlots[0]
      ?? null,
    [teachSlots, slotId],
  );
  const selectedSlotId = selectedSlot?.id ?? '';
  const currentState = statusState(status, selectedSlot);
  const currentReason = statusReason(status, selectedSlot);
  const isRunning = !!status?.running || ['idle', 'slash_menu', 'running', 'starting', 'thinking', 'responding', 'tool_running', 'confirming', 'blocked'].includes((currentState || '').toLowerCase());
  const canSendInput = ['idle', 'slash_menu', 'confirming', 'blocked'].includes((currentState || '').toLowerCase());

  useEffect(() => {
    if (!selectedSlotId) return;
    setSlotId(selectedSlotId);
    localStorage.setItem('board:teachSlot', selectedSlotId);
  }, [selectedSlotId]);

  useEffect(() => {
    localStorage.setItem('board:teachControlsCollapsed', controlsCollapsed ? 'true' : 'false');
  }, [controlsCollapsed]);

  useEffect(() => {
    localStorage.setItem('board:teachKeepAlive', keepAlive ? 'true' : 'false');
  }, [keepAlive]);

  const recordStep = useCallback((step: Omit<TeachingStep, 'id' | 'at'>) => {
    setSteps((prev) => [
      { id: Date.now(), at: new Date().toLocaleTimeString(), ...step },
      ...prev,
    ].slice(0, 16));
  }, []);

  const observe = useCallback(async (action: string, result: TeachingStep['result'], record = true) => {
    if (!selectedSlotId) return;
    let nextStatus: PtyStatus | null = null;
    let screenTail: string | undefined;
    try {
      const statusRes = await fetch(`/api/pty/status?slotId=${encodeURIComponent(selectedSlotId)}`);
      nextStatus = await statusRes.json();
      setStatus(nextStatus);
    } catch {
      nextStatus = null;
    }
    try {
      const screenRes = await fetch(`/api/pty/screen?slotId=${encodeURIComponent(selectedSlotId)}&lines=40`);
      const screenData = await screenRes.json();
      if (typeof screenData?.screen === 'string' && !screenData.screen.includes('"error"')) {
        setScreenText(screenData.screen);
        screenTail = compactScreenTail(screenData.screen);
      }
    } catch {
      screenTail = undefined;
    }
    if (record) {
      recordStep({
        action,
        result,
        state: statusState(nextStatus, selectedSlot),
        reason: statusReason(nextStatus, selectedSlot) ?? undefined,
        screenTail,
      });
    }
    refreshSlots?.();
  }, [recordStep, refreshSlots, selectedSlot, selectedSlotId]);

  const refreshStatus = useCallback(async () => {
    if (!selectedSlotId) return;
    await observe('refresh status', 'observed', false);
  }, [observe, selectedSlotId]);

  useEffect(() => {
    void refreshStatus();
    const id = setInterval(() => void refreshStatus(), controlsCollapsed ? 1000 : 5000);
    return () => clearInterval(id);
  }, [controlsCollapsed, refreshStatus]);

  async function sendInput(input: { text?: string; key?: string; label: string }) {
    if (!selectedSlotId || busy) return;
    setBusy(true);
    try {
      const res = await fetch('/api/pty/raw', {
        method: 'POST',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify({ slotId: selectedSlotId, text: input.text, key: input.key }),
      });
      const data = await res.json().catch(() => ({}));
      if (!res.ok || data?.error) throw new Error(String(data?.error || res.statusText));
      await new Promise((resolve) => setTimeout(resolve, 350));
      await observe(input.label, 'sent');
    } catch (err) {
      recordStep({ action: input.label, result: 'failed', reason: String(err) });
    } finally {
      setBusy(false);
    }
  }

  async function startSlot(operatorShell = false, options: { autoRestart?: boolean; action?: string } = {}) {
    if (!selectedSlotId || busy) return;
    setBusy(true);
    try {
      const res = await fetch(`/api/pty/spawn?slotId=${encodeURIComponent(selectedSlotId)}`, {
        method: 'POST',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify({
          ...(operatorShell ? { operatorShell: true } : {}),
          ...(options.autoRestart ? { autoRestart: true } : {}),
        }),
      });
      const data = await res.json().catch(() => ({}));
      if (!res.ok || data?.error) throw new Error(String(data?.error || res.statusText));
      await new Promise((resolve) => setTimeout(resolve, 800));
      if (!operatorShell) setKeepAlive(true);
      await observe(options.action || (operatorShell ? 'start operator shell' : 'start PTY'), 'observed');
    } catch (err) {
      recordStep({ action: options.action || 'start PTY', result: 'failed', reason: String(err) });
    } finally {
      setBusy(false);
    }
  }

  const keepAliveSpawn = useCallback(async () => {
    if (!selectedSlotId || keepAliveInflightRef.current) return;
    const now = Date.now();
    if (now - lastKeepAliveSpawnAtRef.current < 5000) return;
    keepAliveInflightRef.current = true;
    lastKeepAliveSpawnAtRef.current = now;
    try {
      const res = await fetch(`/api/pty/spawn?slotId=${encodeURIComponent(selectedSlotId)}`, {
        method: 'POST',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify({ autoRestart: true }),
      });
      const data = await res.json().catch(() => ({}));
      if (!res.ok || data?.error) throw new Error(String(data?.error || res.statusText));
      await new Promise((resolve) => setTimeout(resolve, 800));
      await observe('keep alive restart', 'observed');
    } catch (err) {
      recordStep({ action: 'keep alive restart', result: 'failed', reason: String(err) });
    } finally {
      keepAliveInflightRef.current = false;
    }
  }, [observe, recordStep, selectedSlotId]);

  useEffect(() => {
    if (!keepAlive || busy || !selectedSlotId || status === null || isRunning) return;
    void keepAliveSpawn();
  }, [busy, isRunning, keepAlive, keepAliveSpawn, selectedSlotId, status]);

  async function stopSlot() {
    if (!selectedSlotId || busy) return;
    setKeepAlive(false);
    setBusy(true);
    try {
      const res = await fetch(`/api/pty/kill?slotId=${encodeURIComponent(selectedSlotId)}`, { method: 'POST' });
      const data = await res.json().catch(() => ({}));
      if (!res.ok || data?.error) throw new Error(String(data?.error || res.statusText));
      await observe('stop PTY', 'observed');
    } catch (err) {
      recordStep({ action: 'stop PTY', result: 'failed', reason: String(err) });
    } finally {
      setBusy(false);
    }
  }

  async function guarded(actionId: string, run: () => Promise<void>) {
    if (dangerArmed !== actionId) {
      setDangerArmed(actionId);
      window.setTimeout(() => setDangerArmed((value) => (value === actionId ? null : value)), 4000);
      return;
    }
    setDangerArmed(null);
    await run();
  }

  async function submitTypedText() {
    const value = text;
    if (!value || !canSendInput) return;
    if (isDangerousText(value)) {
      await guarded(`text:${value.trim().toLowerCase()}`, async () => {
        setText('');
        await sendInput({ text: value, label: `type ${JSON.stringify(value)}` });
      });
      return;
    }
    setText('');
    await sendInput({ text: value, label: `type ${JSON.stringify(value)}` });
  }

  if (teachSlots.length === 0) {
    return (
      <div className="mx-4 mb-4 flex min-h-0 flex-1 items-center justify-center rounded-lg border border-neutral-800 bg-neutral-950/60 text-sm text-neutral-500 sm:mx-8">
        No teachable CLI slots projected.
      </div>
    );
  }

  if (controlsCollapsed) {
    return (
      <div className="mx-4 mb-4 flex min-h-0 flex-1 flex-col overflow-hidden rounded-lg border border-neutral-800 bg-neutral-950 sm:mx-8">
        <div className="flex shrink-0 items-center justify-between gap-3 border-b border-neutral-800 bg-neutral-950/95 px-3 py-2">
          <div className="flex min-w-0 items-center gap-2">
            <span className={cn('h-2 w-2 shrink-0 rounded-full', isRunning ? 'bg-emerald-400' : 'bg-neutral-600')} />
            <span className="truncate font-mono text-xs text-neutral-400" title={selectedSlotId}>{selectedSlotId || 'no-slot'}</span>
            <span className={cn('shrink-0 font-mono text-[10px]', stateTone(currentState))}>{currentState || 'unknown'}</span>
            {keepAlive ? <span className="shrink-0 rounded border border-emerald-500/20 px-1.5 py-0.5 text-[10px] text-emerald-300">keep</span> : null}
            {currentReason ? (
              <span className="hidden truncate text-[10px] text-neutral-600 sm:block" title={currentReason}>{currentReason}</span>
            ) : null}
          </div>
          <button
            onClick={() => setControlsCollapsed(false)}
            className="shrink-0 rounded-md border border-neutral-800 bg-neutral-900 px-2 py-1 text-[10px] text-neutral-500 hover:text-neutral-200"
          >
            Controls
          </button>
        </div>
        <div className="min-h-0 flex-1 overflow-hidden bg-black">
          {screenText ? (
            <pre className="h-full overflow-hidden whitespace-pre-wrap break-words p-3 font-mono text-[13px] leading-5 text-neutral-200">
              {screenText}
            </pre>
          ) : selectedSlot ? (
            <div className="flex h-full items-center justify-center text-sm text-neutral-600">Waiting for PTY screen...</div>
          ) : (
            <div className="flex h-full items-center justify-center text-sm text-neutral-500">No slot selected.</div>
          )}
        </div>
      </div>
    );
  }

  return (
    <div className="mx-4 mb-4 grid min-h-0 flex-1 grid-cols-1 gap-3 overflow-hidden sm:mx-8 xl:grid-cols-[320px_minmax(0,1fr)]">
      <aside className="flex min-h-0 flex-col rounded-lg border border-neutral-800 bg-neutral-950/70">
        <div className="border-b border-neutral-800 px-3 py-3">
          <div className="flex items-center justify-between gap-3">
            <div>
              <div className="text-sm font-semibold text-neutral-100">PTY Teach</div>
              <div className="mt-1 font-mono text-[10px] text-neutral-600">{selectedSlotId || 'no-slot'}</div>
            </div>
            <button
              onClick={() => void observe('refresh status', 'observed')}
              disabled={busy}
              title="Refresh"
              className="rounded-md border border-neutral-800 bg-neutral-900 p-2 text-neutral-400 hover:text-white disabled:opacity-50"
            >
              <RefreshCw className="h-4 w-4" />
            </button>
            <button
              onClick={() => setControlsCollapsed(true)}
              title="Show live only"
              className="rounded-md border border-neutral-800 bg-neutral-900 px-2 py-2 text-[10px] text-neutral-500 hover:text-neutral-200"
            >
              Live
            </button>
          </div>
          <div className="mt-3">
            <label className="text-[10px] uppercase tracking-wide text-neutral-600">CLI slot</label>
            <select
              value={selectedSlotId}
              onChange={(event) => setSlotId(event.target.value)}
              className="mt-1 w-full rounded-md border border-neutral-800 bg-neutral-900 px-2 py-1.5 text-xs text-neutral-200 outline-none"
            >
              {teachSlots.map((slot) => (
                <option key={slot.id} value={slot.id}>{slot.label || slot.id}</option>
              ))}
            </select>
          </div>
          <dl className="mt-3 space-y-1.5 text-xs">
            <div className="flex justify-between gap-3">
              <dt className="text-neutral-600">State</dt>
              <dd className={cn('font-mono', stateTone(currentState))}>{currentState || 'unknown'}</dd>
            </div>
            <div className="flex justify-between gap-3">
              <dt className="text-neutral-600">Running</dt>
              <dd className={isRunning ? 'text-emerald-300' : 'text-neutral-500'}>{isRunning ? 'yes' : 'no'}</dd>
            </div>
            <div className="flex justify-between gap-3">
              <dt className="text-neutral-600">Reason</dt>
              <dd className="max-w-44 truncate text-neutral-400" title={currentReason || undefined}>{currentReason || '-'}</dd>
            </div>
          </dl>
        </div>

        <div className="space-y-3 border-b border-neutral-800 p-3">
          <div className="flex gap-2">
            <button
              onClick={() => void startSlot(false, { autoRestart: true })}
              disabled={busy || isRunning}
              className="inline-flex flex-1 items-center justify-center gap-1.5 rounded-md border border-emerald-500/20 bg-emerald-500/10 px-2 py-1.5 text-xs text-emerald-300 hover:bg-emerald-500/15 disabled:opacity-40"
            >
              <Play className="h-3.5 w-3.5" /> Start
            </button>
            <button
              onClick={() => void startSlot(true)}
              disabled={busy || isRunning}
              className="inline-flex flex-1 items-center justify-center gap-1.5 rounded-md border border-sky-500/20 bg-sky-500/10 px-2 py-1.5 text-xs text-sky-300 hover:bg-sky-500/15 disabled:opacity-40"
            >
              <Keyboard className="h-3.5 w-3.5" /> Shell
            </button>
            <button
              onClick={() => void guarded('restart', async () => {
                await stopSlot();
                await new Promise((resolve) => setTimeout(resolve, 600));
                setKeepAlive(true);
                await startSlot(false, { autoRestart: true, action: 'restart PTY' });
              })}
              disabled={busy}
              className={cn(
                'inline-flex flex-1 items-center justify-center gap-1.5 rounded-md border px-2 py-1.5 text-xs disabled:opacity-40',
                dangerArmed === 'restart'
                  ? 'border-amber-500/40 bg-amber-500/15 text-amber-200'
                  : 'border-neutral-800 bg-neutral-900 text-neutral-400 hover:text-white',
              )}
            >
              <RotateCcw className="h-3.5 w-3.5" /> {dangerArmed === 'restart' ? 'Confirm' : 'Restart'}
            </button>
          </div>
          <button
            onClick={() => setKeepAlive((value) => !value)}
            className={cn(
              'mt-2 inline-flex w-full items-center justify-center gap-1.5 rounded-md border px-2 py-1.5 text-xs',
              keepAlive
                ? 'border-emerald-500/20 bg-emerald-500/10 text-emerald-300 hover:bg-emerald-500/15'
                : 'border-neutral-800 bg-neutral-900 text-neutral-500 hover:text-neutral-200',
            )}
          >
            <RefreshCw className="h-3.5 w-3.5" /> Keep Alive {keepAlive ? 'On' : 'Off'}
          </button>
        </div>

        <div className="space-y-3 border-b border-neutral-800 p-3">
          <div className="text-[10px] uppercase tracking-wide text-neutral-600">Safe input</div>
          <div className="flex min-w-0 items-center gap-1.5 rounded-md border border-neutral-800 bg-neutral-900/70 px-2 py-1.5">
            <Keyboard className="h-3.5 w-3.5 shrink-0 text-neutral-500" />
            <input
              value={text}
              onChange={(event) => setText(event.target.value)}
              onKeyDown={(event) => {
                if (event.key === 'Enter') {
                  event.preventDefault();
                  void submitTypedText();
                }
              }}
              placeholder="type text"
              className="min-w-0 flex-1 bg-transparent text-xs text-neutral-200 outline-none placeholder:text-neutral-600"
            />
            <button
              onClick={() => {
                void submitTypedText();
              }}
              disabled={busy || !text || !canSendInput}
              title="Send typed text"
              className="rounded p-1 text-neutral-500 hover:bg-neutral-800 hover:text-neutral-200 disabled:opacity-40"
            >
              <Send className="h-3.5 w-3.5" />
            </button>
          </div>
          <div className="grid grid-cols-4 gap-1.5">
            {SAFE_KEY_ACTIONS.map(({ key, label, icon: Icon }) => (
              <button
                key={key}
                onClick={() => void sendInput({ key, label: `press ${label}` })}
                disabled={busy || !canSendInput}
                title={label}
                className="inline-flex items-center justify-center gap-1 rounded-md border border-neutral-800 bg-neutral-900 px-2 py-1.5 text-xs text-neutral-400 hover:text-white disabled:opacity-40"
              >
                <Icon className="h-3.5 w-3.5" />
                {label}
              </button>
            ))}
          </div>
          <div className="grid grid-cols-3 gap-1.5">
            {SAFE_TEXT_ACTIONS.map(({ text: command, label, icon: Icon }) => (
              <button
                key={command}
                onClick={() => void sendInput({ text: command, label: `type ${command}` })}
                disabled={busy || !canSendInput}
                title={label}
                className="inline-flex items-center justify-center gap-1 rounded-md border border-neutral-800 bg-neutral-900 px-2 py-1.5 text-xs text-neutral-400 hover:text-white disabled:opacity-40"
              >
                <Icon className="h-3.5 w-3.5" />
                {label.replace('/', '')}
              </button>
            ))}
          </div>
        </div>

        <div className="space-y-2 border-b border-neutral-800 p-3">
          <div className="text-[10px] uppercase tracking-wide text-neutral-600">Guarded</div>
          <div className="grid grid-cols-2 gap-1.5">
            <button
              onClick={() => void guarded('ctrl-d', () => sendInput({ key: 'ctrl-d', label: 'press Ctrl-D' }))}
              disabled={busy || !canSendInput}
              className={cn(
                'inline-flex items-center justify-center gap-1 rounded-md border px-2 py-1.5 text-xs disabled:opacity-40',
                dangerArmed === 'ctrl-d'
                  ? 'border-amber-500/40 bg-amber-500/15 text-amber-200'
                  : 'border-neutral-800 bg-neutral-900 text-neutral-400 hover:text-white',
              )}
            >
              <Power className="h-3.5 w-3.5" /> {dangerArmed === 'ctrl-d' ? 'Confirm' : 'Ctrl-D'}
            </button>
            <button
              onClick={() => void guarded('stop', stopSlot)}
              disabled={busy || !canSendInput}
              className={cn(
                'inline-flex items-center justify-center gap-1 rounded-md border px-2 py-1.5 text-xs disabled:opacity-40',
                dangerArmed === 'stop'
                  ? 'border-red-500/40 bg-red-500/15 text-red-200'
                  : 'border-neutral-800 bg-neutral-900 text-neutral-400 hover:text-white',
              )}
            >
              <Square className="h-3.5 w-3.5" /> {dangerArmed === 'stop' ? 'Confirm' : 'Stop'}
            </button>
          </div>
        </div>

        <div className="min-h-0 flex-1 overflow-auto p-3">
          <div className="mb-2 text-[10px] uppercase tracking-wide text-neutral-600">Step log</div>
          {steps.length > 0 ? (
            <div className="space-y-2">
              {steps.map((step) => (
                <div key={step.id} className="rounded-md border border-neutral-800 bg-neutral-900/50 p-2">
                  <div className="flex items-center justify-between gap-2">
                    <span className={cn(
                      'truncate text-xs',
                      step.result === 'failed' ? 'text-red-300' : step.result === 'observed' ? 'text-neutral-400' : 'text-emerald-300',
                    )}>
                      {step.action}
                    </span>
                    <span className="shrink-0 font-mono text-[10px] text-neutral-600">{step.at}</span>
                  </div>
                  <div className="mt-1 flex items-center gap-2 text-[10px]">
                    {step.state ? <span className={stateTone(step.state)}>{step.state}</span> : null}
                    {step.reason ? <span className="truncate text-neutral-600" title={step.reason}>{step.reason}</span> : null}
                  </div>
                  {step.screenTail ? (
                    <div className="mt-1 line-clamp-2 text-[10px] text-neutral-500" title={step.screenTail}>{step.screenTail}</div>
                  ) : null}
                </div>
              ))}
            </div>
          ) : (
            <div className="text-xs text-neutral-600">No steps yet.</div>
          )}
        </div>
      </aside>

      <main className="min-h-0 overflow-hidden rounded-lg border border-neutral-800 bg-neutral-950">
        {selectedSlot ? (
          <Terminal
            key={`teach-${selectedSlot.id}`}
            slotId={selectedSlot.id}
            slot={selectedSlot}
            showHeaderActions={false}
            enableDirectInput={false}
          />
        ) : (
          <div className="flex h-full items-center justify-center text-sm text-neutral-500">No slot selected.</div>
        )}
      </main>
    </div>
  );
}
