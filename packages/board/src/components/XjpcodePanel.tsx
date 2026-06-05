'use client';

import { FormEvent, KeyboardEvent, useCallback, useEffect, useMemo, useRef, useState } from 'react';
import {
  Activity,
  AlertTriangle,
  Bot,
  Loader2,
  Plus,
  RefreshCw,
  Send,
  Square,
  Terminal,
  User,
  Wifi,
  WifiOff,
  Wrench,
} from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
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

const DEFAULT_BASE_URL = 'http://127.0.0.1:4040';
const SESSION_STORAGE_KEY = 'board:xjpcode:session-id';
const BASE_URL_STORAGE_KEY = 'board:xjpcode:base-url';
const NON_CHAT_MODEL_IDS = new Set(['codex_image_generation', 'codex_research']);

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

function statusModels(payload: StatusPayload): { current: string; models: string[] } {
  const body = payload.models?.body as { current?: unknown; models?: unknown } | undefined;
  const models = Array.isArray(body?.models)
    ? body.models
        .map((item) => {
          if (typeof item === 'string') return item;
          if (item && typeof item === 'object' && 'id' in item && typeof item.id === 'string') return item.id;
          return '';
        })
        .filter(Boolean)
        .filter((id) => !NON_CHAT_MODEL_IDS.has(id))
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
  const [models, setModels] = useState<string[]>([]);
  const [messages, setMessages] = useState<ChatMessage[]>([]);
  const [turnEvents, setTurnEvents] = useState<TurnEvent[]>([]);
  const [input, setInput] = useState('');
  const [status, setStatus] = useState<StatusPayload | null>(null);
  const [currentStatus, setCurrentStatus] = useState('');
  const [isRefreshing, setIsRefreshing] = useState(false);
  const [isStreaming, setIsStreaming] = useState(false);
  const abortRef = useRef<AbortController | null>(null);
  const scrollRef = useRef<HTMLDivElement | null>(null);

  const online = status?.ok === true;
  const currentModel = useMemo(
    () => model || statusModels(status ?? {}).current || models[0] || '',
    [model, models, status],
  );

  useEffect(() => {
    localStorage.setItem(SESSION_STORAGE_KEY, sessionId);
  }, [sessionId]);

  useEffect(() => {
    localStorage.setItem(BASE_URL_STORAGE_KEY, baseUrl);
  }, [baseUrl]);

  useEffect(() => {
    scrollRef.current?.scrollTo({ top: scrollRef.current.scrollHeight });
  }, [messages, isStreaming]);

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
              <span className="truncate">{currentModel || 'model pending'}</span>
              {currentStatus ? <span className="truncate text-stone-400">{currentStatus}</span> : null}
            </div>
          </div>
        </div>

        <div className="grid min-w-0 grid-cols-1 gap-2 sm:grid-cols-[minmax(180px,280px)_minmax(150px,220px)_auto_auto]">
          <Input
            value={baseUrl}
            onBlur={refreshStatus}
            onChange={(event) => setBaseUrl(event.target.value)}
            className="h-8 font-mono text-xs"
            aria-label="xjpcode endpoint"
          />
          <select
            value={currentModel}
            onChange={(event) => setModel(event.target.value)}
            className="h-8 rounded-md border border-input bg-white/[0.035] px-2 text-xs text-foreground shadow-inner shadow-black/10 focus:outline-none focus:ring-2 focus:ring-ring/35"
            aria-label="xjpcode model"
          >
            {currentModel && !models.includes(currentModel) ? <option value={currentModel}>{currentModel}</option> : null}
            {models.length === 0 ? <option value="">Current model</option> : null}
            {models.map((item) => <option key={item} value={item}>{item}</option>)}
          </select>
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

        <aside className="flex max-h-72 w-full shrink-0 flex-col bg-black/10 lg:max-h-none lg:w-96">
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
