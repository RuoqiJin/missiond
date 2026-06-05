'use client';

import { FormEvent, KeyboardEvent, useCallback, useEffect, useMemo, useRef, useState } from 'react';
import {
  Activity,
  AlertTriangle,
  Bot,
  ChevronDown,
  ChevronUp,
  Gauge,
  Loader2,
  Plus,
  Play,
  RefreshCw,
  Send,
  Square,
  Terminal,
  Trash2,
  User,
  Wifi,
  WifiOff,
  Wrench,
} from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
} from '@/components/ui/select';
import { Textarea } from '@/components/ui/textarea';
import { MarkdownContent } from '@/components/timeline/MarkdownContent';
import { cn } from '@/lib/utils';

type ChatRole = 'user' | 'assistant' | 'error';

type ChatMessage = {
  id: string;
  role: ChatRole;
  content: string;
  createdAt: string;
  durationMs?: number;
  streaming?: boolean;
};

type TurnEvent = {
  id: string;
  type: 'status' | 'context' | 'tool_call' | 'tool_result' | 'error' | 'done';
  label: string;
  content?: string;
  input?: unknown;
  pending?: boolean;
  error?: boolean;
  createdAt: string;
};

type ChatFrame =
  | { type: 'status'; message?: string }
  | { type: 'text'; content?: string }
  | { type: 'context'; label?: string; chars?: number }
  | { type: 'tool_call'; id?: string; name?: string; input?: unknown }
  | { type: 'tool_result'; id?: string; name?: string; result?: string }
  | { type: 'error'; message?: string }
  | { type: 'done' };

type StatusPayload = {
  ok?: boolean;
  baseUrl?: string;
  error?: string;
  health?: { ok?: boolean; body?: unknown };
  models?: { ok?: boolean; body?: unknown };
};

type ModelOption = {
  id: string;
  ownedBy?: string;
  root?: string;
  provider?: string;
  sourceId?: string;
  displayName?: string;
  providerModelId?: string;
  modelName?: string;
  modelProfile?: string;
  mode?: string;
  completionEndpoint?: string;
};

type LatencySample = {
  durationMs?: number;
  firstByteMs?: number | null;
  error?: string;
  finishedAt?: string;
};

type LatencyRow = {
  model: ModelOption;
  attempts: Array<LatencySample | null>;
  status: 'idle' | 'running' | 'done' | 'error' | 'stopped';
};

type LatencySortMode = 'source' | 'asc' | 'desc';

type LatencyResponse = {
  ok?: boolean;
  durationMs?: number;
  firstByteMs?: number | null;
  error?: string;
};

const DEFAULT_BASE_URL = 'http://127.0.0.1:4040';
const SESSION_STORAGE_KEY = 'board:xjpcode:session-id';
const BASE_URL_STORAGE_KEY = 'board:xjpcode:base-url';
const NON_CHAT_MODEL_IDS = new Set(['codex_image_generation', 'codex_research']);
const LATENCY_ATTEMPTS = 3;
const LATENCY_PROBE_TIMEOUT_MS = 60_000;

function newId(prefix: string): string {
  const random = typeof crypto !== 'undefined' && 'randomUUID' in crypto
    ? crypto.randomUUID()
    : `${Date.now()}-${Math.random().toString(16).slice(2)}`;
  return `${prefix}-${random}`;
}

function initialSessionId(): string {
  if (typeof window === 'undefined') return newId('xjpcode-session');
  return localStorage.getItem(SESSION_STORAGE_KEY) || newId('xjpcode-session');
}

function initialBaseUrl(): string {
  if (typeof window === 'undefined') return DEFAULT_BASE_URL;
  return localStorage.getItem(BASE_URL_STORAGE_KEY) || DEFAULT_BASE_URL;
}

function shortId(id: string): string {
  return id.replace(/^xjpcode-session-/, '').slice(0, 8);
}

function timeLabel(value: string): string {
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return '';
  return date.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit', second: '2-digit' });
}

function durationLabel(durationMs?: number): string {
  if (typeof durationMs !== 'number' || !Number.isFinite(durationMs) || durationMs < 0) return '';
  const seconds = durationMs / 1000;
  if (seconds < 10) return `${seconds.toFixed(1)}s`;
  if (seconds < 60) return `${Math.round(seconds)}s`;
  const minutes = Math.floor(seconds / 60);
  const rest = Math.round(seconds % 60);
  return `${minutes}m ${rest}s`;
}

function jsonPreview(value: unknown): string {
  if (value == null) return '';
  if (typeof value === 'string') return value;
  try {
    return JSON.stringify(value, null, 2);
  } catch {
    return String(value);
  }
}

function readStringField(value: unknown, key: string): string {
  return value && typeof value === 'object' && key in value && typeof value[key as keyof typeof value] === 'string'
    ? value[key as keyof typeof value] as string
    : '';
}

function modelProviderLabel(model: ModelOption): string {
  const ownedBy = model.ownedBy?.toLowerCase().replace(/[_\s-]/g, '') || '';
  const provider = model.provider?.toLowerCase().replace(/[_\s-]/g, '') || '';
  const id = model.id.toLowerCase();

  if (provider === 'codexagent') return 'MissionD-Codex Agent';
  if (provider === 'claudecodeagent') return 'MissionD-ClaudeCode Agent';
  if (ownedBy === 'missiondproviderbox') {
    if (id.startsWith('missiond-codex-agent-')) return 'MissionD-Codex Agent';
    if (id.startsWith('missiond-claude-code-agent-')) return 'MissionD-ClaudeCode Agent';
    return 'MissionD-Agent';
  }

  if (ownedBy === 'missiondagy') {
    if (id.startsWith('claude-code-')) return 'MissionD-ClaudeCode';
    if (id.startsWith('codex-')) return 'MissionD-Codex';
    return 'MissionD-AGY';
  }

  if (ownedBy === 'vertex') return 'Google-Vertex';
  if (ownedBy === 'geminisource') return 'Gemini';
  if (ownedBy === 'anthropicdirect') return 'Anthropic';
  if (ownedBy === 'ccsource') return 'ClaudeCode';
  if (ownedBy === 'openrouter') return 'OpenRouter';
  if (ownedBy === 'clewdr') return 'Clewdr';
  if (ownedBy === 'minimax') return 'MiniMax';
  if (ownedBy === 'zhipu') return 'Zhipu';
  if (ownedBy === 'jarvis') return 'Jarvis';
  if (ownedBy === 'meow61') return 'Meow61';

  if (id.startsWith('agy-')) return 'MissionD-AGY';
  if (id.startsWith('missiond-codex-agent-')) return 'MissionD-Codex Agent';
  if (id.startsWith('missiond-claude-code-agent-')) return 'MissionD-ClaudeCode Agent';
  if (id.startsWith('claude-code-')) return 'MissionD-ClaudeCode';
  if (id.startsWith('codex-')) return 'MissionD-Codex';
  if (id.startsWith('gemini-')) return 'Google-Vertex';
  if (id.startsWith('glm-')) return 'Zhipu';
  if (id.startsWith('minimax-')) return 'MiniMax';
  if (id.startsWith('claude-')) return 'Claude';
  return 'Router';
}

function modelDisplayLabel(model: ModelOption): string {
  return `${modelProviderLabel(model)} - ${model.displayName || model.id}`;
}

function modelDetailLabel(model: ModelOption): string {
  const details = [
    model.providerModelId || model.modelName,
    model.modelProfile,
    model.mode === 'interactive_agent_session' ? 'agent session' : undefined,
  ].filter(Boolean);
  return details.length > 0 ? details.join(' · ') : model.root || '';
}

function emptyLatencyAttempts(): Array<LatencySample | null> {
  return Array.from({ length: LATENCY_ATTEMPTS }, () => null);
}

function makeLatencyRow(model: ModelOption): LatencyRow {
  return {
    model,
    attempts: emptyLatencyAttempts(),
    status: 'idle',
  };
}

function averageLatencyMs(attempts: Array<LatencySample | null>): number | undefined {
  const values = attempts
    .map((sample) => sample?.durationMs)
    .filter((value): value is number => typeof value === 'number' && Number.isFinite(value));
  if (values.length === 0) return undefined;
  return values.reduce((sum, value) => sum + value, 0) / values.length;
}

function statusModels(payload: StatusPayload): { current: string; models: ModelOption[] } {
  const body = payload.models?.body as { current?: unknown; models?: unknown } | undefined;
  const models = Array.isArray(body?.models)
    ? body.models
        .map((item): ModelOption | null => {
          if (typeof item === 'string') return { id: item };
          const id = readStringField(item, 'id');
          if (!id) return null;
          return {
            id,
            ownedBy: readStringField(item, 'owned_by') || readStringField(item, 'provider') || undefined,
            root: readStringField(item, 'root') || undefined,
            provider: readStringField(item, 'provider') || undefined,
            sourceId: readStringField(item, 'source_id') || undefined,
            displayName: readStringField(item, 'display_name') || undefined,
            providerModelId: readStringField(item, 'provider_model_id') || undefined,
            modelName: readStringField(item, 'model') || undefined,
            modelProfile: readStringField(item, 'model_profile') || undefined,
            mode: readStringField(item, 'mode') || undefined,
            completionEndpoint: readStringField(item, 'completion_endpoint') || undefined,
          };
        })
        .filter((item): item is ModelOption => item !== null && !NON_CHAT_MODEL_IDS.has(item.id))
    : [];
  return {
    current: typeof body?.current === 'string' && !NON_CHAT_MODEL_IDS.has(body.current) ? body.current : '',
    models,
  };
}

export function XjpcodePanel() {
  const [baseUrl, setBaseUrl] = useState(initialBaseUrl);
  const [sessionId, setSessionId] = useState(initialSessionId);
  const [model, setModel] = useState('');
  const [models, setModels] = useState<ModelOption[]>([]);
  const [messages, setMessages] = useState<ChatMessage[]>([]);
  const [turnEvents, setTurnEvents] = useState<TurnEvent[]>([]);
  const [input, setInput] = useState('');
  const [status, setStatus] = useState<StatusPayload | null>(null);
  const [currentStatus, setCurrentStatus] = useState('');
  const [isRefreshing, setIsRefreshing] = useState(false);
  const [isStreaming, setIsStreaming] = useState(false);
  const [latencyRows, setLatencyRows] = useState<Record<string, LatencyRow>>({});
  const [latencySortMode, setLatencySortMode] = useState<LatencySortMode>('source');
  const [selectedLatencyModelIds, setSelectedLatencyModelIds] = useState<Set<string>>(() => new Set());
  const [isLatencyCollapsed, setIsLatencyCollapsed] = useState(false);
  const [isTestingLatency, setIsTestingLatency] = useState(false);
  const [testingModelId, setTestingModelId] = useState('');
  const [testingAttempt, setTestingAttempt] = useState<number | null>(null);
  const abortRef = useRef<AbortController | null>(null);
  const latencyAbortRef = useRef<AbortController | null>(null);
  const knownLatencyModelIdsRef = useRef<Set<string>>(new Set());
  const scrollRef = useRef<HTMLDivElement | null>(null);

  const online = status?.ok === true;
  const currentModel = useMemo(
    () => model || statusModels(status ?? {}).current || models[0]?.id || '',
    [model, models, status],
  );
  const currentModelOption = useMemo(
    () => models.find((item) => item.id === currentModel) || (currentModel ? { id: currentModel } : null),
    [currentModel, models],
  );
  const selectedLatencyModels = useMemo(
    () => models.filter((item) => selectedLatencyModelIds.has(item.id)),
    [models, selectedLatencyModelIds],
  );
  const latencyRowsForDisplay = useMemo(() => {
    const modelIds = new Set(models.map((item) => item.id));
    const modelOrder = new Map(models.map((item, index) => [item.id, index]));
    const liveRows = models.map((item) => (
      latencyRows[item.id] ? { ...latencyRows[item.id], model: item } : makeLatencyRow(item)
    ));
    const staleRows = Object.values(latencyRows).filter((row) => !modelIds.has(row.model.id));
    const rows = [...liveRows, ...staleRows];

    if (latencySortMode !== 'source') {
      return rows.sort((a, b) => {
        const delta = modelDisplayLabel(a.model).localeCompare(modelDisplayLabel(b.model), undefined, {
          numeric: true,
          sensitivity: 'base',
        });
        return latencySortMode === 'asc' ? delta : -delta;
      });
    }

    return rows.sort((a, b) => {
      const rank = (row: LatencyRow) => {
        if (row.model.id === testingModelId) return 0;
        if (row.attempts.some(Boolean)) return 1;
        return 2;
      };
      const rankDelta = rank(a) - rank(b);
      if (rankDelta !== 0) return rankDelta;
      return (modelOrder.get(a.model.id) ?? Number.MAX_SAFE_INTEGER)
        - (modelOrder.get(b.model.id) ?? Number.MAX_SAFE_INTEGER);
    });
  }, [latencyRows, latencySortMode, models, testingModelId]);

  useEffect(() => {
    localStorage.setItem(SESSION_STORAGE_KEY, sessionId);
  }, [sessionId]);

  useEffect(() => {
    localStorage.setItem(BASE_URL_STORAGE_KEY, baseUrl);
  }, [baseUrl]);

  useEffect(() => {
    scrollRef.current?.scrollTo({ top: scrollRef.current.scrollHeight });
  }, [messages, isStreaming]);

  useEffect(() => {
    const nextKnownIds = new Set(models.map((item) => item.id));
    const previousKnownIds = knownLatencyModelIdsRef.current;
    setSelectedLatencyModelIds((prev) => {
      const next = new Set([...prev].filter((id) => nextKnownIds.has(id)));
      for (const id of nextKnownIds) {
        if (!previousKnownIds.has(id)) next.add(id);
      }
      return next;
    });
    knownLatencyModelIdsRef.current = nextKnownIds;
  }, [models]);

  const refreshStatus = useCallback(async () => {
    setIsRefreshing(true);
    try {
      const params = new URLSearchParams({ baseUrl });
      const response = await fetch(`/api/xjpcode/status?${params}`, { cache: 'no-store' });
      const payload = await response.json() as StatusPayload;
      setStatus(payload);
      const nextModels = statusModels(payload);
      setModels(nextModels.models);
      if (!model && nextModels.current) setModel(nextModels.current);
    } catch (err) {
      setStatus({ ok: false, baseUrl, error: String(err) });
    } finally {
      setIsRefreshing(false);
    }
  }, [baseUrl, model]);

  useEffect(() => {
    refreshStatus();
  }, []); // eslint-disable-line react-hooks/exhaustive-deps

  function addTurnEvent(event: Omit<TurnEvent, 'id' | 'createdAt'> & { id?: string }) {
    setTurnEvents((prev) => [
      ...prev.slice(-59),
      {
        ...event,
        id: event.id || newId(`xjpcode-event-${event.type}`),
        createdAt: new Date().toISOString(),
      },
    ]);
  }

  function appendAssistantDelta(assistantId: string, content: string) {
    if (!content) return;
    setMessages((prev) => {
      const existing = prev.find((message) => message.id === assistantId);
      if (!existing) {
        return [
          ...prev,
          {
            id: assistantId,
            role: 'assistant',
            content,
            createdAt: new Date().toISOString(),
            streaming: true,
          },
        ];
      }
      return prev.map((message) => (
        message.id === assistantId
          ? { ...message, content: message.content + content, streaming: true }
          : message
      ));
    });
  }

  function finishAssistant(assistantId: string, durationMs?: number) {
    setMessages((prev) => prev.map((message) => (
      message.id === assistantId ? { ...message, streaming: false, durationMs: durationMs ?? message.durationMs } : message
    )));
  }

  const handleFrame = useCallback((frame: ChatFrame, assistantId: string, turnStartedAt: number) => {
    switch (frame.type) {
      case 'status': {
        const message = frame.message || '';
        setCurrentStatus(message);
        addTurnEvent({ type: 'status', label: 'Status', content: message });
        break;
      }
      case 'text':
        appendAssistantDelta(assistantId, frame.content || '');
        break;
      case 'context':
        addTurnEvent({
          type: 'context',
          label: frame.label || 'Context',
          content: typeof frame.chars === 'number' ? `${frame.chars} chars` : '',
        });
        break;
      case 'tool_call':
        addTurnEvent({
          id: frame.id ? `tool-${frame.id}` : undefined,
          type: 'tool_call',
          label: frame.name || 'Tool',
          input: frame.input,
          pending: true,
        });
        break;
      case 'tool_result':
        setTurnEvents((prev) => {
          const key = frame.id ? `tool-${frame.id}` : '';
          if (key && prev.some((event) => event.id === key)) {
            return prev.map((event) => event.id === key ? {
              ...event,
              type: 'tool_result',
              content: frame.result || '',
              pending: false,
              error: (frame.result || '').startsWith('Error:'),
            } : event);
          }
          return [
            ...prev.slice(-59),
            {
              id: newId('xjpcode-tool-result'),
              type: 'tool_result',
              label: frame.name || 'Tool result',
              content: frame.result || '',
              pending: false,
              error: (frame.result || '').startsWith('Error:'),
              createdAt: new Date().toISOString(),
            },
          ];
        });
        break;
      case 'error':
        {
          const duration = Date.now() - turnStartedAt;
          const label = durationLabel(duration);
          finishAssistant(assistantId, duration);
          setMessages((prev) => [
            ...prev,
            {
              id: newId('xjpcode-error'),
              role: 'error',
              content: frame.message || 'xjpcode returned an error',
              createdAt: new Date().toISOString(),
              durationMs: duration,
            },
          ]);
          addTurnEvent({
            type: 'error',
            label: 'Error',
            content: label ? `${frame.message || ''}\nResponse time: ${label}` : frame.message || '',
            error: true,
          });
        }
        break;
      case 'done': {
        const duration = Date.now() - turnStartedAt;
        const label = durationLabel(duration);
        finishAssistant(assistantId, duration);
        addTurnEvent({ type: 'done', label: 'Done', content: label ? `Response time: ${label}` : undefined });
        break;
      }
    }
  }, []);

  function toggleLatencySortMode() {
    setLatencySortMode((value) => (
      value === 'source' ? 'asc' : value === 'asc' ? 'desc' : 'source'
    ));
  }

  function toggleLatencyModel(modelId: string, selected: boolean) {
    setSelectedLatencyModelIds((prev) => {
      const next = new Set(prev);
      if (selected) {
        next.add(modelId);
      } else {
        next.delete(modelId);
      }
      return next;
    });
  }

  function selectAllLatencyModels() {
    setSelectedLatencyModelIds(new Set(models.map((item) => item.id)));
  }

  function clearLatencyModelSelection() {
    setSelectedLatencyModelIds(new Set());
  }

  function resetLatencyRows(targetModels: ModelOption[]) {
    setLatencyRows((prev) => {
      const next = { ...prev };
      for (const target of targetModels) {
        next[target.id] = makeLatencyRow(target);
      }
      return next;
    });
  }

  function updateLatencyRow(
    target: ModelOption,
    updater: (row: LatencyRow) => LatencyRow,
  ) {
    setLatencyRows((prev) => {
      const current = prev[target.id] || makeLatencyRow(target);
      return {
        ...prev,
        [target.id]: updater({ ...current, model: target, attempts: [...current.attempts] }),
      };
    });
  }

  async function runLatencyProbe(target: ModelOption, signal: AbortSignal): Promise<LatencySample> {
    const response = await fetch('/api/xjpcode/latency', {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({
        baseUrl,
        model: target.id,
        timeoutMs: LATENCY_PROBE_TIMEOUT_MS,
      }),
      signal,
    });
    const raw = await response.text();
    let payload: LatencyResponse = {};
    try {
      payload = raw ? JSON.parse(raw) as LatencyResponse : {};
    } catch {
      payload = { error: raw };
    }

    const sample: LatencySample = {
      durationMs: typeof payload.durationMs === 'number' ? payload.durationMs : undefined,
      firstByteMs: typeof payload.firstByteMs === 'number' ? payload.firstByteMs : null,
      finishedAt: new Date().toISOString(),
    };

    if (!response.ok || !payload.ok) {
      sample.error = payload.error || `HTTP ${response.status}`;
    }

    return sample;
  }

  async function startLatencyTests(targetModels: ModelOption[]) {
    if (isTestingLatency) {
      latencyAbortRef.current?.abort();
      return;
    }

    const uniqueTargets = targetModels.filter((target, index, list) => (
      target.id && list.findIndex((item) => item.id === target.id) === index
    ));
    if (uniqueTargets.length === 0) return;

    resetLatencyRows(uniqueTargets);
    setIsTestingLatency(true);
    const abort = new AbortController();
    latencyAbortRef.current = abort;

    let activeTarget: ModelOption | null = null;
    try {
      for (const target of uniqueTargets) {
        activeTarget = target;
        setTestingModelId(target.id);

        for (let attemptIndex = 0; attemptIndex < LATENCY_ATTEMPTS; attemptIndex += 1) {
          if (abort.signal.aborted) throw new DOMException('Latency probe stopped', 'AbortError');
          setTestingAttempt(attemptIndex + 1);
          updateLatencyRow(target, (row) => ({ ...row, status: 'running' }));

          const sample = await runLatencyProbe(target, abort.signal);
          updateLatencyRow(target, (row) => {
            const attempts = [...row.attempts];
            attempts[attemptIndex] = sample;
            return {
              ...row,
              attempts,
              status: attemptIndex + 1 === LATENCY_ATTEMPTS
                ? attempts.some((item) => item?.error) ? 'error' : 'done'
                : 'running',
            };
          });
        }
      }
    } catch (err) {
      if (activeTarget) {
        updateLatencyRow(activeTarget, (row) => ({
          ...row,
          status: err instanceof Error && err.name === 'AbortError' ? 'stopped' : 'error',
        }));
      }
    } finally {
      latencyAbortRef.current = null;
      setIsTestingLatency(false);
      setTestingModelId('');
      setTestingAttempt(null);
    }
  }

  async function sendMessage(event?: FormEvent) {
    event?.preventDefault();
    if (isStreaming) {
      abortRef.current?.abort();
      return;
    }

    const text = input.trim();
    if (!text) return;

    const turnStartedAt = Date.now();
    const userMessage: ChatMessage = {
      id: newId('xjpcode-user'),
      role: 'user',
      content: text,
      createdAt: new Date().toISOString(),
    };
    const assistantId = newId('xjpcode-assistant');
    setMessages((prev) => [...prev, userMessage]);
    setInput('');
    setCurrentStatus('sending');
    setIsStreaming(true);

    const abort = new AbortController();
    abortRef.current = abort;

    try {
      const response = await fetch('/api/xjpcode/chat', {
        method: 'POST',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify({
          baseUrl,
          session_id: sessionId,
          input: text,
          model: currentModel || null,
        }),
        signal: abort.signal,
      });

      if (!response.ok || !response.body) {
        const body = await response.json().catch(async () => ({ error: await response.text().catch(() => '') }));
        throw new Error(body.error || `xjpcode HTTP ${response.status}`);
      }

      const reader = response.body.getReader();
      const decoder = new TextDecoder();
      let buffer = '';

      while (true) {
        const { done, value } = await reader.read();
        if (done) break;
        buffer += decoder.decode(value, { stream: true });

        let index;
        while ((index = buffer.indexOf('\n')) !== -1) {
          const line = buffer.slice(0, index);
          buffer = buffer.slice(index + 1);
          const trimmed = line.trim();
          if (!trimmed.startsWith('data:')) continue;
          const json = trimmed.slice(5).trim();
          if (!json) continue;
          try {
            handleFrame(JSON.parse(json) as ChatFrame, assistantId, turnStartedAt);
          } catch {
            // Ignore malformed partial SSE lines.
          }
        }
      }
      finishAssistant(assistantId, Date.now() - turnStartedAt);
    } catch (err) {
      const duration = Date.now() - turnStartedAt;
      if ((err as Error).name !== 'AbortError') {
        const message = err instanceof Error ? err.message : String(err);
        setMessages((prev) => [
          ...prev,
          {
            id: newId('xjpcode-error'),
            role: 'error',
            content: message,
            createdAt: new Date().toISOString(),
            durationMs: duration,
          },
        ]);
        setCurrentStatus('error');
        addTurnEvent({
          type: 'error',
          label: 'Error',
          content: `Response time: ${durationLabel(duration)}\n${message}`,
          error: true,
        });
      } else {
        setCurrentStatus('stopped');
        finishAssistant(assistantId, duration);
        addTurnEvent({ type: 'done', label: 'Stopped', content: `Response time: ${durationLabel(duration)}` });
      }
    } finally {
      abortRef.current = null;
      setIsStreaming(false);
      setTimeout(() => setCurrentStatus((value) => (value === 'stopped' || value === 'error' ? value : '')), 800);
    }
  }

  async function resetSession() {
    abortRef.current?.abort();
    const previousSessionId = sessionId;
    const nextSessionId = newId('xjpcode-session');
    setSessionId(nextSessionId);
    setMessages([]);
    setTurnEvents([]);
    setCurrentStatus('');
    await fetch('/api/xjpcode/session', {
      method: 'DELETE',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ baseUrl, session_id: previousSessionId }),
    }).catch(() => null);
  }

  function handleKeyDown(event: KeyboardEvent<HTMLTextAreaElement>) {
    if (event.key === 'Enter' && !event.shiftKey) {
      event.preventDefault();
      sendMessage();
    }
  }

  return (
    <div className="mission-panel mx-4 mb-4 flex min-h-0 flex-1 flex-col overflow-hidden sm:mx-6">
      <header className="flex shrink-0 flex-col gap-3 border-b border-white/[0.07] px-3 py-3 lg:flex-row lg:items-center lg:justify-between">
        <div className="flex min-w-0 items-center gap-3">
          <div className="flex h-9 w-9 shrink-0 items-center justify-center rounded-md border border-teal-300/20 bg-teal-400/10">
            <Terminal className="h-4 w-4 text-teal-200" />
          </div>
          <div className="min-w-0">
            <div className="flex min-w-0 items-center gap-2">
              <h2 className="truncate text-sm font-semibold text-stone-100">XJPCode</h2>
              <span className={cn('inline-flex items-center gap-1 rounded-full border px-2 py-0.5 text-[10px]',
                online ? 'border-emerald-400/25 bg-emerald-400/10 text-emerald-200' : 'border-red-400/25 bg-red-400/10 text-red-200')}>
                {online ? <Wifi className="h-3 w-3" /> : <WifiOff className="h-3 w-3" />}
                {online ? 'online' : 'offline'}
              </span>
              {isStreaming ? (
                <span className="inline-flex items-center gap-1 rounded-full border border-amber-400/25 bg-amber-400/10 px-2 py-0.5 text-[10px] text-amber-200">
                  <Loader2 className="h-3 w-3 animate-spin" />
                  active
                </span>
              ) : null}
            </div>
            <div className="mt-0.5 flex min-w-0 items-center gap-2 text-[10px] text-stone-500">
              <span className="font-mono">{shortId(sessionId)}</span>
              <span className="truncate">{currentModelOption ? modelDisplayLabel(currentModelOption) : 'model pending'}</span>
              {currentStatus ? <span className="truncate text-stone-400">{currentStatus}</span> : null}
            </div>
          </div>
        </div>

        <div className="grid min-w-0 grid-cols-1 gap-2 sm:grid-cols-[minmax(180px,280px)_minmax(220px,340px)_auto_auto]">
          <Input
            value={baseUrl}
            onBlur={refreshStatus}
            onChange={(event) => setBaseUrl(event.target.value)}
            className="h-8 font-mono text-xs"
            aria-label="xjpcode endpoint"
          />
          <Select
            value={currentModel || undefined}
            onValueChange={setModel}
            disabled={!currentModel && models.length === 0}
          >
            <SelectTrigger className="h-8 font-mono text-xs text-stone-100" aria-label="xjpcode model">
              <span className="truncate">
                {currentModelOption ? modelDisplayLabel(currentModelOption) : 'Current model'}
              </span>
            </SelectTrigger>
            <SelectContent className="max-h-[360px] border-white/10 bg-stone-950 text-stone-100 shadow-2xl shadow-black/50">
              {currentModelOption && !models.some((item) => item.id === currentModelOption.id) ? (
                <SelectItem value={currentModel} className="font-mono text-xs text-stone-100 focus:bg-teal-400/15 focus:text-teal-100">
                  <span className="flex min-w-0 flex-col">
                    <span className="truncate">{modelDisplayLabel(currentModelOption)}</span>
                  </span>
                </SelectItem>
              ) : null}
              {models.map((item) => (
                <SelectItem key={item.id} value={item.id} className="font-mono text-xs text-stone-100 focus:bg-teal-400/15 focus:text-teal-100">
                  <span className="flex min-w-0 flex-col">
                    <span className="truncate">{modelDisplayLabel(item)}</span>
                    {modelDetailLabel(item) ? <span className="truncate text-[10px] text-stone-500">{modelDetailLabel(item)}</span> : null}
                  </span>
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
          <Button type="button" size="sm" variant="outline" onClick={refreshStatus} disabled={isRefreshing}>
            {isRefreshing ? <Loader2 className="h-4 w-4 animate-spin" /> : <RefreshCw className="h-4 w-4" />}
            Refresh
          </Button>
          <Button type="button" size="sm" variant="outline" onClick={resetSession}>
            <Plus className="h-4 w-4" />
            New
          </Button>
        </div>
      </header>

      <div className="flex min-h-0 flex-1 flex-col lg:flex-row">
        <main className="flex min-h-0 flex-1 flex-col border-b border-white/[0.07] lg:border-b-0 lg:border-r">
          <div ref={scrollRef} className="min-h-0 flex-1 overflow-auto px-3 py-4">
            {messages.length === 0 ? (
              <div className="flex h-full items-center justify-center">
                <div className="rounded-md border border-white/[0.07] bg-white/[0.025] px-4 py-3 text-center">
                  <div className="text-sm font-medium text-stone-200">xjpcode session ready</div>
                  <div className="mt-1 font-mono text-[10px] text-stone-500">{baseUrl}</div>
                </div>
              </div>
            ) : (
              <div className="space-y-4">
                {messages.map((message) => (
                  <MessageBubble key={message.id} message={message} />
                ))}
              </div>
            )}
          </div>

          <form onSubmit={sendMessage} className="shrink-0 border-t border-white/[0.07] p-3">
            <div className="flex items-end gap-2">
              <Textarea
                value={input}
                onChange={(event) => setInput(event.target.value)}
                onKeyDown={handleKeyDown}
                placeholder="Message xjpcode"
                className="max-h-44 min-h-[54px] resize-none text-sm"
                disabled={isStreaming && !abortRef.current}
              />
              <Button type="submit" className="h-[54px] w-[54px] shrink-0 px-0" aria-label={isStreaming ? 'Stop' : 'Send'}>
                {isStreaming ? <Square className="h-4 w-4" /> : <Send className="h-4 w-4" />}
              </Button>
            </div>
          </form>
        </main>

        <aside className="flex max-h-72 w-full shrink-0 flex-col bg-black/10 lg:max-h-none lg:w-[30rem]">
          <LatencyTestPanel
            rows={latencyRowsForDisplay}
            modelCount={models.length}
            selectedCount={selectedLatencyModels.length}
            selectedModelIds={selectedLatencyModelIds}
            sortMode={latencySortMode}
            collapsed={isLatencyCollapsed}
            isTesting={isTestingLatency}
            testingModelId={testingModelId}
            testingAttempt={testingAttempt}
            onTestAll={() => startLatencyTests(selectedLatencyModels)}
            onTestCurrent={() => {
              if (currentModelOption) startLatencyTests([currentModelOption]);
            }}
            onStop={() => latencyAbortRef.current?.abort()}
            onClear={() => setLatencyRows({})}
            onToggleSort={toggleLatencySortMode}
            onToggleCollapsed={() => setIsLatencyCollapsed((value) => !value)}
            onToggleModel={toggleLatencyModel}
            onSelectAll={selectAllLatencyModels}
            onSelectNone={clearLatencyModelSelection}
            canTestAll={selectedLatencyModels.length > 0}
            canTestCurrent={Boolean(currentModelOption)}
          />
          <div className="shrink-0 border-b border-white/[0.07] px-3 py-2.5">
            <div className="flex items-center justify-between gap-3">
              <div className="text-xs font-medium text-stone-200">Turn Events</div>
              <div className="font-mono text-[10px] text-stone-500">{turnEvents.length}</div>
            </div>
          </div>
          <div className="min-h-0 flex-1 overflow-auto p-3">
            {turnEvents.length === 0 ? (
              <div className="text-xs text-stone-600">No events yet.</div>
            ) : (
              <div className="space-y-2">
                {turnEvents.slice().reverse().map((event) => (
                  <TurnEventRow key={event.id} event={event} />
                ))}
              </div>
            )}
          </div>
        </aside>
      </div>
    </div>
  );
}

function LatencyTestPanel({
  rows,
  modelCount,
  selectedCount,
  selectedModelIds,
  sortMode,
  collapsed,
  isTesting,
  testingModelId,
  testingAttempt,
  onTestAll,
  onTestCurrent,
  onStop,
  onClear,
  onToggleSort,
  onToggleCollapsed,
  onToggleModel,
  onSelectAll,
  onSelectNone,
  canTestAll,
  canTestCurrent,
}: {
  rows: LatencyRow[];
  modelCount: number;
  selectedCount: number;
  selectedModelIds: Set<string>;
  sortMode: LatencySortMode;
  collapsed: boolean;
  isTesting: boolean;
  testingModelId: string;
  testingAttempt: number | null;
  onTestAll: () => void;
  onTestCurrent: () => void;
  onStop: () => void;
  onClear: () => void;
  onToggleSort: () => void;
  onToggleCollapsed: () => void;
  onToggleModel: (modelId: string, selected: boolean) => void;
  onSelectAll: () => void;
  onSelectNone: () => void;
  canTestAll: boolean;
  canTestCurrent: boolean;
}) {
  const testedCount = rows.filter((row) => row.attempts.some(Boolean)).length;
  const activeRow = rows.find((row) => row.model.id === testingModelId);
  const sortLabel = sortMode === 'source' ? 'Source order' : sortMode === 'asc' ? 'Name A-Z' : 'Name Z-A';

  return (
    <section className={cn('shrink-0 border-b border-white/[0.07] p-3', collapsed ? 'py-2.5' : '')}>
      <div className="flex items-center justify-between gap-3">
        <div className="flex min-w-0 items-center gap-2">
          <Gauge className="h-3.5 w-3.5 shrink-0 text-teal-300" />
          <div className="truncate text-xs font-medium text-stone-200">Model Latency</div>
        </div>
        <div className="flex shrink-0 items-center gap-2">
          {isTesting && collapsed ? <Loader2 className="h-3 w-3 animate-spin text-amber-200" /> : null}
          <div className="font-mono text-[10px] text-stone-500">{testedCount}/{selectedCount}/{modelCount}</div>
          <Button
            type="button"
            size="sm"
            variant="outline"
            onClick={onToggleCollapsed}
            className="h-7 px-2 text-xs"
            aria-label={collapsed ? 'Show latency test panel' : 'Hide latency test panel'}
          >
            {collapsed ? <ChevronDown className="h-3.5 w-3.5" /> : <ChevronUp className="h-3.5 w-3.5" />}
            {collapsed ? 'Show' : 'Hide'}
          </Button>
        </div>
      </div>

      {collapsed ? null : (
        <>
      <div className="mt-2 flex flex-wrap items-center gap-2">
        <Button
          type="button"
          size="sm"
          variant="outline"
          onClick={isTesting ? onStop : onTestAll}
          disabled={!isTesting && !canTestAll}
          className="h-7 px-2 text-xs"
        >
          {isTesting ? <Square className="h-3.5 w-3.5" /> : <Play className="h-3.5 w-3.5" />}
          {isTesting ? 'Stop' : selectedCount === modelCount ? 'Test All' : 'Test Selected'}
        </Button>
        <Button
          type="button"
          size="sm"
          variant="outline"
          onClick={onTestCurrent}
          disabled={isTesting || !canTestCurrent}
          className="h-7 px-2 text-xs"
        >
          <Gauge className="h-3.5 w-3.5" />
          Current
        </Button>
        <Button
          type="button"
          size="sm"
          variant="outline"
          onClick={onToggleSort}
          disabled={isTesting}
          className="h-7 px-2 text-xs"
        >
          {sortLabel}
        </Button>
        <Button
          type="button"
          size="sm"
          variant="outline"
          onClick={onSelectAll}
          disabled={isTesting || selectedCount === modelCount}
          className="h-7 px-2 text-xs"
        >
          All
        </Button>
        <Button
          type="button"
          size="sm"
          variant="outline"
          onClick={onSelectNone}
          disabled={isTesting || selectedCount === 0}
          className="h-7 px-2 text-xs"
        >
          None
        </Button>
        <Button
          type="button"
          size="sm"
          variant="outline"
          onClick={onClear}
          disabled={isTesting || testedCount === 0}
          className="h-7 px-2 text-xs"
        >
          <Trash2 className="h-3.5 w-3.5" />
          Clear
        </Button>
      </div>

      {isTesting && activeRow ? (
        <div className="mt-2 flex min-w-0 items-center gap-2 rounded border border-amber-400/20 bg-amber-400/[0.06] px-2 py-1.5 text-[10px] text-amber-100">
          <Loader2 className="h-3 w-3 shrink-0 animate-spin" />
          <span className="truncate font-mono">{modelDisplayLabel(activeRow.model)}</span>
          <span className="shrink-0 font-mono">#{testingAttempt || 1}</span>
        </div>
      ) : null}

      <div className="mt-2 max-h-56 overflow-auto rounded-md border border-white/[0.07] bg-black/10">
        {rows.length === 0 ? (
          <div className="p-3 text-xs text-stone-600">No models loaded.</div>
        ) : (
          <div className="min-w-[560px]">
            <div className="grid grid-cols-[24px_minmax(220px,1fr)_58px_58px_58px_72px] gap-2 border-b border-white/[0.07] px-2 py-1.5 font-mono text-[10px] uppercase text-stone-600">
              <div />
              <div>model</div>
              <div>1st</div>
              <div>2nd</div>
              <div>3rd</div>
              <div>avg</div>
            </div>
            {rows.map((row) => (
              <LatencyRowView
                key={row.model.id}
                row={row}
                selected={selectedModelIds.has(row.model.id)}
                activeAttempt={row.model.id === testingModelId ? testingAttempt : null}
                disabled={isTesting}
                onToggleModel={onToggleModel}
              />
            ))}
          </div>
        )}
      </div>
        </>
      )}
    </section>
  );
}

function LatencyRowView({
  row,
  selected,
  activeAttempt,
  disabled,
  onToggleModel,
}: {
  row: LatencyRow;
  selected: boolean;
  activeAttempt: number | null;
  disabled: boolean;
  onToggleModel: (modelId: string, selected: boolean) => void;
}) {
  const average = averageLatencyMs(row.attempts);
  const hasError = row.attempts.some((sample) => sample?.error);
  const averageText = typeof average === 'number' ? durationLabel(average) : '';

  return (
    <div className={cn('grid grid-cols-[24px_minmax(220px,1fr)_58px_58px_58px_72px] items-center gap-2 border-b border-white/[0.04] px-2 py-1.5 last:border-b-0',
      selected ? '' : 'opacity-50')}>
      <div className="flex items-center justify-center">
        <input
          type="checkbox"
          checked={selected}
          onChange={(event) => onToggleModel(row.model.id, event.target.checked)}
          disabled={disabled}
          aria-label={`Select ${modelDisplayLabel(row.model)}`}
          className="h-3.5 w-3.5 rounded border-stone-600 bg-black/20 accent-teal-300 disabled:cursor-not-allowed"
        />
      </div>
      <div className="min-w-0">
        <div className="truncate font-mono text-[10px] text-stone-300" title={modelDisplayLabel(row.model)}>
          {modelDisplayLabel(row.model)}
        </div>
        {row.model.root ? (
          <div className="truncate font-mono text-[9px] text-stone-600" title={row.model.root}>{row.model.root}</div>
        ) : null}
      </div>
      {row.attempts.map((sample, index) => (
        <LatencyCell
          key={`${row.model.id}-${index}`}
          sample={sample}
          active={activeAttempt === index + 1}
        />
      ))}
      <div className={cn('truncate font-mono text-[10px]',
        row.status === 'running' ? 'text-amber-200' : hasError ? 'text-red-300' : averageText ? 'text-teal-300' : 'text-stone-700')}>
        {row.status === 'running'
          ? 'running'
          : averageText || (row.status === 'stopped' ? 'stopped' : hasError ? 'error' : '--')}
      </div>
    </div>
  );
}

function LatencyCell({ sample, active }: { sample: LatencySample | null; active: boolean }) {
  if (active && !sample) {
    return (
      <div className="flex items-center gap-1 font-mono text-[10px] text-amber-200">
        <Loader2 className="h-3 w-3 animate-spin" />
        run
      </div>
    );
  }

  if (!sample) return <div className="font-mono text-[10px] text-stone-700">--</div>;
  if (sample.error) {
    return (
      <div className="truncate font-mono text-[10px] text-red-300" title={sample.error}>
        err
      </div>
    );
  }

  const firstByte = typeof sample.firstByteMs === 'number' ? `first byte ${durationLabel(sample.firstByteMs)}` : '';
  return (
    <div className="truncate font-mono text-[10px] text-stone-300" title={firstByte}>
      {durationLabel(sample.durationMs)}
    </div>
  );
}

function MessageBubble({ message }: { message: ChatMessage }) {
  const isUser = message.role === 'user';
  const isError = message.role === 'error';
  const duration = durationLabel(message.durationMs);
  return (
    <div className={cn('flex gap-2', isUser ? 'justify-end' : 'justify-start')}>
      {!isUser ? (
        <div className={cn('mt-1 flex h-7 w-7 shrink-0 items-center justify-center rounded-md border',
          isError ? 'border-red-400/25 bg-red-400/10 text-red-200' : 'border-teal-300/20 bg-teal-400/10 text-teal-200')}>
          {isError ? <AlertTriangle className="h-3.5 w-3.5" /> : <Bot className="h-3.5 w-3.5" />}
        </div>
      ) : null}
      <div className={cn('max-w-[min(780px,82%)] rounded-md border px-3 py-2',
        isUser
          ? 'border-amber-400/20 bg-amber-400/[0.09] text-amber-50'
          : isError
            ? 'border-red-400/20 bg-red-400/[0.08] text-red-100'
            : 'border-white/[0.07] bg-white/[0.035] text-stone-100')}>
        <div className="mb-1 flex items-center justify-between gap-3 text-[10px] text-stone-500">
          <span>{isUser ? 'You' : isError ? 'Error' : 'XJPCode'}</span>
          <span className="flex shrink-0 items-center gap-2 font-mono">
            {duration ? <span className="text-teal-300">{duration}</span> : null}
            <span>{timeLabel(message.createdAt)}</span>
          </span>
        </div>
        {isUser || isError ? (
          <div className="whitespace-pre-wrap break-words text-sm leading-relaxed">{message.content}</div>
        ) : (
          <MarkdownContent content={message.content} />
        )}
        {message.streaming ? (
          <div className="mt-2 inline-flex items-center gap-1 text-[10px] text-amber-200">
            <Loader2 className="h-3 w-3 animate-spin" />
            streaming
          </div>
        ) : null}
      </div>
      {isUser ? (
        <div className="mt-1 flex h-7 w-7 shrink-0 items-center justify-center rounded-md border border-amber-400/20 bg-amber-400/10 text-amber-200">
          <User className="h-3.5 w-3.5" />
        </div>
      ) : null}
    </div>
  );
}

function TurnEventRow({ event }: { event: TurnEvent }) {
  const Icon = event.type === 'tool_call' || event.type === 'tool_result'
    ? Wrench
    : event.type === 'error'
      ? AlertTriangle
      : event.type === 'done'
        ? Activity
        : Bot;
  const preview = event.content || jsonPreview(event.input);
  return (
    <div className={cn('rounded-md border bg-white/[0.025] p-2 text-xs',
      event.error ? 'border-red-400/20' : event.pending ? 'border-amber-400/20' : 'border-white/[0.07]')}>
      <div className="flex min-w-0 items-center gap-2">
        <Icon className={cn('h-3.5 w-3.5 shrink-0',
          event.error ? 'text-red-300' : event.pending ? 'text-amber-300' : 'text-teal-300')} />
        <div className="min-w-0 flex-1 truncate font-medium text-stone-200">{event.label}</div>
        <div className="font-mono text-[10px] text-stone-600">{timeLabel(event.createdAt)}</div>
      </div>
      {preview ? (
        <pre className="mt-2 max-h-40 overflow-auto whitespace-pre-wrap break-words rounded border border-white/[0.06] bg-black/20 p-2 font-mono text-[10px] leading-relaxed text-stone-400">
          {preview.length > 4000 ? `${preview.slice(0, 4000)}\n[...]` : preview}
        </pre>
      ) : null}
    </div>
  );
}
