'use client';

import { useState, useRef, useEffect, useCallback } from 'react';
import ReactMarkdown from 'react-markdown';
import remarkGfm from 'remark-gfm';
import {
  Send, ImagePlus, Loader2, Brain, X, Plus,
  MessageSquare, ChevronLeft, ChevronDown, ChevronRight,
  Check, AlertTriangle, Clock, Activity,
} from 'lucide-react';
import { cn } from '@/lib/utils';

// ─── Types ───

interface ChatMessage {
  id: string;
  role: 'user' | 'assistant' | 'system';
  content: string;
  /** base64 image data URLs */
  images?: string[];
  timestamp: number;
}

interface Conversation {
  id: string;
  title: string;
  updatedAt: string;
  messageCount: number;
}

interface ToolActivity {
  id: string;
  tool: string;
  params?: Record<string, unknown>;
  status: 'running' | 'done';
  durationMs?: number;
  output?: string;
}

interface ConfirmOption {
  key: number | string;
  label: string;
  is_default?: boolean;
}

interface ConfirmAction {
  actionId: string;
  prompt: string;
  info?: { type?: string; tool?: string; options?: ConfirmOption[] };
}

interface SlotStatus {
  slotId: string;
  state: string;
  lastActivitySecsAgo?: number;
  sessionId?: string;
  progress?: {
    toolCounts: Record<string, number>;
    currentTool?: { name: string; startedAt: string };
    totalCalls: number;
    totalResults: number;
    errorCount: number;
  };
}

// ─── Content Cleaner ───

/**
 * Strip system prompts, boundary markers, and TUI noise from assistant content.
 * Handles both streamed (raw PTY) and stored (pre-cleaned) messages.
 */
function cleanAssistantContent(content: string): string {
  let text = content;

  // 1. Strip everything before and including <<<BOUNDARY_xxx>>>
  const boundaryMatch = text.match(/<<<BOUNDARY_[a-f0-9]+>>>/g);
  if (boundaryMatch) {
    const lastMarker = boundaryMatch[boundaryMatch.length - 1];
    const pos = text.lastIndexOf(lastMarker);
    text = text.slice(pos + lastMarker.length);
  }

  // 2. Strip <system_info>...</system_info> blocks
  text = text.replace(/<system_info>[\s\S]*?<\/system_info>/g, '');

  // 3. Strip <system-reminder>...</system-reminder> blocks
  text = text.replace(/<system-reminder>[\s\S]*?<\/system-reminder>/g, '');

  // 4. Strip Claude Code TUI markers and tool output remnants
  text = text
    .split('\n')
    .filter((line) => {
      const trimmed = line.trim();
      if (trimmed.startsWith('▸▸') || trimmed.startsWith('>>')) return false;
      if (trimmed.startsWith('✕ ') || trimmed.startsWith('✗ ')) return false;
      if (trimmed.includes('Auto-update failed')) return false;
      if (trimmed.includes('bypass permissions on')) return false;
      // Safety net: strip tool output border lines that leaked through
      if (trimmed.startsWith('⎿') || trimmed.startsWith('│')) return false;
      // Strip tool result file path references
      if (trimmed.includes('tool-results/')) return false;
      return true;
    })
    .map((line) => {
      const trimmed = line.trimStart();
      if (trimmed.startsWith('⏺ ') || trimmed === '⏺') return trimmed.slice(1).trimStart();
      if (trimmed.startsWith('● ') || trimmed === '●') return trimmed.slice(1).trimStart();
      return line;
    })
    .join('\n');

  return text.trim();
}

// ─── SSE Parser ───

function parseSSE(chunk: string): Array<{ event?: string; data: string }> {
  const events: Array<{ event?: string; data: string }> = [];
  let currentEvent: string | undefined;
  let dataLines: string[] = [];

  for (const line of chunk.split('\n')) {
    if (line.startsWith('event: ')) {
      currentEvent = line.slice(7).trim();
    } else if (line.startsWith('data: ')) {
      dataLines.push(line.slice(6));
    } else if (line === '' && dataLines.length > 0) {
      events.push({ event: currentEvent, data: dataLines.join('\n') });
      currentEvent = undefined;
      dataLines = [];
    }
  }
  // Handle trailing data without final newline
  if (dataLines.length > 0) {
    events.push({ event: currentEvent, data: dataLines.join('\n') });
  }
  return events;
}

// ─── Tool Card ───

function toolParamSummary(tool: ToolActivity): string {
  if (!tool.params) return '';
  const p = tool.params;
  if (tool.tool === 'Bash' && p.command) return String(p.command);
  if (tool.tool === 'Read' && p.file_path) return String(p.file_path);
  if (tool.tool === 'Write' && p.file_path) return String(p.file_path);
  if (tool.tool === 'Edit' && p.file_path) return String(p.file_path);
  if (tool.tool === 'Grep' && p.pattern) return String(p.pattern);
  if (tool.tool === 'Glob' && p.pattern) return String(p.pattern);
  if (tool.tool === 'Agent' && p.description) return String(p.description);
  const first = Object.values(p).find((v) => typeof v === 'string');
  return first ? String(first) : '';
}

function ToolCard({ tool }: { tool: ToolActivity }) {
  const [expanded, setExpanded] = useState(false);
  const summary = toolParamSummary(tool);
  const hasDetails = (tool.params && Object.keys(tool.params).length > 0) || tool.output;

  return (
    <div className="bg-neutral-900/60 border border-neutral-800/50 rounded-lg overflow-hidden">
      <button
        onClick={() => hasDetails && setExpanded(!expanded)}
        className={cn(
          'w-full flex items-center gap-2 px-3 py-1.5 text-[11px] text-left',
          hasDetails ? 'cursor-pointer hover:bg-neutral-800/30' : 'cursor-default',
        )}
      >
        {tool.status === 'running' ? (
          <Loader2 className="w-3 h-3 animate-spin text-amber-400 flex-shrink-0" />
        ) : (
          <Check className="w-3 h-3 text-green-400 flex-shrink-0" />
        )}
        <span className="text-neutral-300 font-mono font-medium">{tool.tool}</span>
        {summary && (
          <span className="text-neutral-500 truncate max-w-[300px] font-mono">{summary}</span>
        )}
        <span className="ml-auto flex items-center gap-1.5 flex-shrink-0">
          {tool.durationMs !== undefined && (
            <span className="text-neutral-600 flex items-center gap-0.5">
              <Clock className="w-2.5 h-2.5" />
              {tool.durationMs < 1000 ? `${Math.round(tool.durationMs)}ms` : `${(tool.durationMs / 1000).toFixed(1)}s`}
            </span>
          )}
          {hasDetails && (
            expanded ? <ChevronDown className="w-3 h-3 text-neutral-600" /> : <ChevronRight className="w-3 h-3 text-neutral-600" />
          )}
        </span>
      </button>

      {expanded && (
        <div className="border-t border-neutral-800/50 px-3 py-2 space-y-2">
          {tool.params && Object.keys(tool.params).length > 0 && (
            <div>
              <div className="text-[10px] text-neutral-600 mb-1 uppercase tracking-wide">Parameters</div>
              <pre className="text-[11px] text-neutral-400 bg-neutral-950 rounded px-2 py-1.5 overflow-x-auto max-h-40 font-mono whitespace-pre-wrap">
                {JSON.stringify(tool.params, null, 2)}
              </pre>
            </div>
          )}
          {tool.output && (
            <div>
              <div className="text-[10px] text-neutral-600 mb-1 uppercase tracking-wide">Output</div>
              <pre className="text-[11px] text-neutral-400 bg-neutral-950 rounded px-2 py-1.5 overflow-x-auto max-h-60 font-mono whitespace-pre-wrap">
                {tool.output}
              </pre>
            </div>
          )}
        </div>
      )}
    </div>
  );
}

// ─── Slot Activity Bar ───

function SlotActivityBar({ status, streamingConvTitle }: {
  status: SlotStatus | null;
  streamingConvTitle?: string;
}) {
  if (!status) return null;

  const state = status.state?.toLowerCase() || 'unknown';
  const progress = status.progress;

  // Don't show when idle
  if (state === 'idle') return null;

  const toolEntries = progress?.toolCounts ? Object.entries(progress.toolCounts) : [];
  const topTools = toolEntries.sort((a, b) => b[1] - a[1]).slice(0, 3);

  return (
    <div className="px-4 py-2 border-b border-amber-800/30 bg-amber-950/20">
      <div className="flex items-center gap-2 text-[11px]">
        <Activity className="w-3 h-3 text-amber-400 flex-shrink-0 animate-pulse" />
        <span className="text-amber-300 font-medium capitalize">{status.state}</span>
        {progress?.currentTool && (
          <>
            <ChevronRight className="w-2.5 h-2.5 text-neutral-600" />
            <span className="text-neutral-300 font-mono">{progress.currentTool.name}</span>
          </>
        )}
        {progress && progress.totalCalls > 0 && (
          <span className="text-neutral-600 font-mono">
            ({progress.totalCalls} calls)
          </span>
        )}
        {streamingConvTitle && (
          <span className="ml-auto text-neutral-600 truncate max-w-[200px]" title={streamingConvTitle}>
            {streamingConvTitle}
          </span>
        )}
      </div>
      {topTools.length > 0 && (
        <div className="flex gap-2 mt-1">
          {topTools.map(([name, count]) => (
            <span key={name} className="text-[10px] text-neutral-600 font-mono">
              {name}:{count}
            </span>
          ))}
        </div>
      )}
    </div>
  );
}

// ─── Main Component ───

export function JarvisChat() {
  const [messages, setMessages] = useState<ChatMessage[]>([]);
  const [input, setInput] = useState('');
  const [images, setImages] = useState<string[]>([]);
  const [isStreaming, setIsStreaming] = useState(false);
  const [statusText, setStatusText] = useState('');
  const [tools, setTools] = useState<ToolActivity[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [confirmAction, setConfirmAction] = useState<ConfirmAction | null>(null);
  const [slotReady, setSlotReady] = useState<boolean | null>(null); // null = checking

  // Conversation history sidebar
  const [conversations, setConversations] = useState<Conversation[]>([]);
  const [activeConvId, setActiveConvId] = useState<string | null>(null);
  const [sidebarOpen, setSidebarOpen] = useState(false);

  // Slot status polling
  const [slotStatus, setSlotStatus] = useState<SlotStatus | null>(null);

  // Streaming conversation tracking — allows switching away during streaming
  const streamingConvIdRef = useRef<string | null>(null);
  const activeConvIdRef = useRef<string | null>(null);
  const messagesCacheRef = useRef<Map<string, ChatMessage[]>>(new Map());
  const streamingToolsRef = useRef<ToolActivity[]>([]);
  const streamingStatusRef = useRef<string>('');

  // Async dispatch tracking
  const pendingAsyncRef = useRef<Set<string>>(new Set());

  const messagesEndRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLTextAreaElement>(null);
  const fileInputRef = useRef<HTMLInputElement>(null);
  const abortRef = useRef<AbortController | null>(null);

  // Keep activeConvIdRef in sync
  useEffect(() => {
    activeConvIdRef.current = activeConvId;
  }, [activeConvId]);

  // Listen for Jarvis task completion via WebSocket — refetch conversation messages
  const loadConversationsRef = useRef<() => void>(() => {});
  useEffect(() => {
    const handler = (e: Event) => {
      const { conversation_id: convId, task_id: taskId } = (e as CustomEvent).detail || {};
      if (!convId) return;

      pendingAsyncRef.current.delete(taskId);

      // Refetch messages if viewing this conversation
      if (activeConvIdRef.current === convId) {
        fetch(`/api/jarvis/conversations?id=${convId}`)
          .then((r) => r.json())
          .then((data) => {
            const msgs: ChatMessage[] = (data.messages || [])
              .filter((m: { role: string }) => m.role === 'user' || m.role === 'assistant')
              .map((m: { id: number; role: string; content: string; timestamp: string }) => ({
                id: String(m.id),
                role: m.role as 'user' | 'assistant',
                content: m.content,
                timestamp: new Date(m.timestamp).getTime(),
              }));
            setMessages(msgs);
            setTools([]);
            setStatusText('');
          })
          .catch(() => { /* ignore */ });
      }

      // Refresh conversation list
      loadConversationsRef.current();
    };

    window.addEventListener('jarvis-task-completed', handler);
    return () => window.removeEventListener('jarvis-task-completed', handler);
  }, []);

  // Auto-spawn slot-jarvis on mount
  useEffect(() => {
    let cancelled = false;
    async function ensureSlot() {
      try {
        const res = await fetch('/api/pty/status?slotId=slot-jarvis');
        const data = await res.json();
        if (data?.state && data.state !== 'exited') {
          if (!cancelled) setSlotReady(true);
          return;
        }
      } catch { /* slot not running */ }

      if (!cancelled) setSlotReady(false);
      try {
        await fetch('/api/pty/spawn?slotId=slot-jarvis', { method: 'POST' });
      } catch { /* ignore spawn errors */ }

      for (let i = 0; i < 30 && !cancelled; i++) {
        await new Promise((r) => setTimeout(r, 1000));
        try {
          const res = await fetch('/api/pty/status?slotId=slot-jarvis');
          const data = await res.json();
          if (data?.state === 'idle') {
            if (!cancelled) setSlotReady(true);
            return;
          }
        } catch { /* keep polling */ }
      }
      if (!cancelled) setSlotReady(true);
    }
    ensureSlot();
    return () => { cancelled = true; };
  }, []);

  // Slot status polling (3s interval)
  useEffect(() => {
    const poll = async () => {
      try {
        const res = await fetch('/api/pty/status?slotId=slot-jarvis');
        if (res.ok) {
          const data = await res.json();
          setSlotStatus(data);
        }
      } catch { /* ignore */ }
    };
    poll();
    const interval = setInterval(poll, 3000);
    return () => clearInterval(interval);
  }, []);

  // Auto-scroll to bottom on new messages
  useEffect(() => {
    requestAnimationFrame(() => {
      messagesEndRef.current?.scrollIntoView({ behavior: 'smooth' });
    });
  }, [messages, statusText, tools, confirmAction]);

  // Load conversation list
  const loadConversations = useCallback(async () => {
    try {
      const res = await fetch('/api/jarvis/conversations');
      if (res.ok) {
        const data = await res.json();
        setConversations(data);
        return data as Conversation[];
      }
    } catch { /* ignore */ }
    return [] as Conversation[];
  }, []);
  // Keep ref in sync for WS event handler
  loadConversationsRef.current = loadConversations;

  // Load conversation list on mount and auto-restore the most recent one
  const initialLoadDone = useRef(false);
  useEffect(() => {
    if (initialLoadDone.current) return;
    initialLoadDone.current = true;
    loadConversations().then((convs) => {
      if (convs.length > 0 && !activeConvId) {
        const latest = convs[0];
        fetch(`/api/jarvis/conversations?id=${latest.id}`)
          .then((r) => r.json())
          .then((data) => {
            const msgs: ChatMessage[] = (data.messages || [])
              .filter((m: { role: string }) => m.role === 'user' || m.role === 'assistant')
              .map((m: { id: number; role: string; content: string; timestamp: string }) => ({
                id: String(m.id),
                role: m.role as 'user' | 'assistant',
                content: m.content,
                timestamp: new Date(m.timestamp).getTime(),
              }));
            if (msgs.length > 0) {
              setMessages(msgs);
              setActiveConvId(latest.id);
            }
          })
          .catch(() => { /* ignore */ });
      }
    });
  }, []); // eslint-disable-line react-hooks/exhaustive-deps

  // Load a specific conversation's messages
  const loadConversation = useCallback(async (convId: string) => {
    // If switching away from a streaming conversation, cache its messages
    if (isStreaming && activeConvIdRef.current) {
      // Don't abort streaming — let it continue in background
      // Cache the current messages state
      messagesCacheRef.current.set(activeConvIdRef.current, [...messages]);
    }

    // Check if target conversation has cached messages
    const cached = messagesCacheRef.current.get(convId);
    if (cached) {
      setMessages(cached);
      setActiveConvId(convId);
      setSidebarOpen(false);
      // Show streaming UI only if this is the streaming conversation
      if (convId !== streamingConvIdRef.current) {
        setTools([]);
        setStatusText('');
      }
      return;
    }

    try {
      const res = await fetch(`/api/jarvis/conversations?id=${convId}`);
      if (res.ok) {
        const data = await res.json();
        const msgs: ChatMessage[] = (data.messages || [])
          .filter((m: { role: string }) => m.role === 'user' || m.role === 'assistant')
          .map((m: { id: number; role: string; content: string; timestamp: string }) => ({
            id: String(m.id),
            role: m.role as 'user' | 'assistant',
            content: m.content,
            timestamp: new Date(m.timestamp).getTime(),
          }));
        setMessages(msgs);
        setActiveConvId(convId);
        setSidebarOpen(false);
        // Clear tool/status if not viewing the streaming conversation
        if (convId !== streamingConvIdRef.current) {
          setTools([]);
          setStatusText('');
        }
      }
    } catch { /* ignore */ }
  }, [isStreaming, messages]);

  // New conversation
  const startNewChat = useCallback(() => {
    // Cache current messages if streaming
    if (isStreaming && activeConvIdRef.current) {
      messagesCacheRef.current.set(activeConvIdRef.current, [...messages]);
    }
    setMessages([]);
    setActiveConvId(null);
    setInput('');
    setImages([]);
    setError(null);
    setSidebarOpen(false);
    // Clear tool/status display (streaming continues in background)
    if (!isStreaming || activeConvIdRef.current !== streamingConvIdRef.current) {
      setTools([]);
      setStatusText('');
    }
    inputRef.current?.focus();
  }, [isStreaming, messages]);

  // Image handling
  const handleImageUpload = useCallback((files: FileList | null) => {
    if (!files) return;
    Array.from(files).forEach((file) => {
      if (!file.type.startsWith('image/')) return;
      if (file.size > 10 * 1024 * 1024) return;
      const reader = new FileReader();
      reader.onload = (e) => {
        const dataUrl = e.target?.result as string;
        setImages((prev) => [...prev, dataUrl]);
      };
      reader.readAsDataURL(file);
    });
  }, []);

  const removeImage = useCallback((index: number) => {
    setImages((prev) => prev.filter((_, i) => i !== index));
  }, []);

  const handlePaste = useCallback((e: React.ClipboardEvent) => {
    const items = e.clipboardData?.items;
    if (!items) return;
    const imageFiles: File[] = [];
    for (const item of Array.from(items)) {
      if (item.type.startsWith('image/')) {
        const file = item.getAsFile();
        if (file) imageFiles.push(file);
      }
    }
    if (imageFiles.length > 0) {
      e.preventDefault();
      const dt = new DataTransfer();
      imageFiles.forEach((f) => dt.items.add(f));
      handleImageUpload(dt.files);
    }
  }, [handleImageUpload]);

  const handleDrop = useCallback((e: React.DragEvent) => {
    e.preventDefault();
    handleImageUpload(e.dataTransfer.files);
  }, [handleImageUpload]);

  // Check if slot is busy (another conversation is streaming)
  const isSlotBusy = isStreaming && streamingConvIdRef.current !== activeConvId;
  // Check if THIS conversation is the one streaming
  const isThisConvStreaming = isStreaming && streamingConvIdRef.current === activeConvId;

  // Send message
  const sendMessage = useCallback(async () => {
    const text = input.trim();
    if (!text && images.length === 0) return;
    if (isStreaming) return; // Only one stream at a time

    // Build user message
    const userMsg: ChatMessage = {
      id: `user-${Date.now()}`,
      role: 'user',
      content: text,
      images: images.length > 0 ? [...images] : undefined,
      timestamp: Date.now(),
    };
    setMessages((prev) => [...prev, userMsg]);
    setInput('');
    setImages([]);
    setIsStreaming(true);
    setStatusText('');
    setTools([]);
    setError(null);

    // Track which conversation is streaming
    const currentConvId = activeConvId;
    streamingConvIdRef.current = currentConvId;

    // Build OpenAI-compatible messages
    const allMsgs = messages
      .filter((m) => m.role !== 'system')
      .concat(userMsg);
    const apiMessages = allMsgs.slice(-40).map((m) => {
        if (m.images && m.images.length > 0) {
          return {
            role: m.role,
            content: [
              { type: 'text' as const, text: m.content },
              ...m.images.map((img) => ({
                type: 'image_url' as const,
                image_url: { url: img },
              })),
            ],
          };
        }
        return { role: m.role, content: m.content };
      });

    const controller = new AbortController();
    abortRef.current = controller;

    try {
      // Retry loop for 503 (slot busy) — wait and retry up to 60s
      const maxRetries = 12;
      let res: Response | null = null;
      for (let attempt = 0; attempt <= maxRetries; attempt++) {
        if (controller.signal.aborted) throw new DOMException('Aborted', 'AbortError');

        res = await fetch('/missiond/v1/chat/completions', {
          method: 'POST',
          headers: {
            'Content-Type': 'application/json',
            'Authorization': 'Bearer jarvis-ui',
            'X-Slot-Id': 'slot-jarvis',
          },
          body: JSON.stringify({ messages: apiMessages, conversation_id: activeConvId }),
          signal: controller.signal,
        });

        if (res.status === 503) {
          const errBody = await res.text();
          let retryAfter = 5;
          try {
            const parsed = JSON.parse(errBody);
            retryAfter = parsed.retry_after || 5;
            const slotState = parsed.error?.message?.match(/state: (\w+)/)?.[1] || 'busy';
            setStatusText(`Jarvis is ${slotState.toLowerCase()}, waiting... (${attempt + 1}/${maxRetries})`);
          } catch {
            setStatusText(`Jarvis is busy, waiting... (${attempt + 1}/${maxRetries})`);
          }
          if (attempt < maxRetries) {
            await new Promise((r) => setTimeout(r, retryAfter * 1000));
            continue;
          }
          // Max retries exceeded
          throw new Error('Jarvis is still busy after waiting 60s. Please try again later.');
        }

        // Non-503 error
        if (!res.ok) {
          const errBody = await res.text();
          let errMsg = `HTTP ${res.status}`;
          try {
            const parsed = JSON.parse(errBody);
            errMsg = parsed.error?.message || errMsg;
          } catch { /* use default */ }
          throw new Error(errMsg);
        }

        // Success — clear waiting status
        setStatusText('');
        break;
      }

      if (!res) throw new Error('No response');
      if (!res.ok) throw new Error(`HTTP ${res.status}`);

      const reader = res.body?.getReader();
      if (!reader) throw new Error('No response body');

      const decoder = new TextDecoder();
      let buffer = '';
      let assistantContent = '';

      while (true) {
        const { done, value } = await reader.read();
        if (done) break;

        buffer += decoder.decode(value, { stream: true });
        const events = parseSSE(buffer);
        buffer = '';

        for (const evt of events) {
          if (evt.data === '[DONE]') continue;

          // Check if user is still viewing this conversation
          const isViewing = activeConvIdRef.current === currentConvId ||
                           (currentConvId === null && activeConvIdRef.current === null);

          if (evt.event === 'meta') {
            try {
              const meta = JSON.parse(evt.data);
              if (meta.conversation_id) {
                streamingConvIdRef.current = meta.conversation_id;
                if (isViewing || currentConvId === null) {
                  setActiveConvId(meta.conversation_id);
                }
              }
            } catch { /* ignore */ }
            continue;
          }

          if (evt.event === 'status') {
            try {
              const status = JSON.parse(evt.data);
              streamingStatusRef.current = status.text || status.phase || '';
              if (isViewing) {
                setStatusText(streamingStatusRef.current);
              }
            } catch { /* ignore */ }
            continue;
          }

          if (evt.event === 'tool_start') {
            try {
              const tool = JSON.parse(evt.data);
              const newTool: ToolActivity = {
                id: tool.id, tool: tool.tool,
                params: typeof tool.params === 'object' ? tool.params : undefined,
                status: 'running',
              };
              streamingToolsRef.current = [...streamingToolsRef.current, newTool];
              if (isViewing) {
                setTools((prev) => [...prev, newTool]);
              }
            } catch { /* ignore */ }
            continue;
          }

          if (evt.event === 'tool_end') {
            try {
              const tool = JSON.parse(evt.data);
              streamingToolsRef.current = streamingToolsRef.current.map((t) =>
                t.id === tool.id ? {
                  ...t,
                  status: 'done' as const,
                  durationMs: tool.duration_ms,
                  output: tool.output || undefined,
                } : t
              );
              if (isViewing) {
                setTools((prev) => prev.map((t) =>
                  t.id === tool.id ? {
                    ...t,
                    status: 'done' as const,
                    durationMs: tool.duration_ms,
                    output: tool.output || undefined,
                  } : t
                ));
              }
            } catch { /* ignore */ }
            continue;
          }

          if (evt.event === 'async_dispatch') {
            try {
              const dispatch = JSON.parse(evt.data);
              const taskId = dispatch.task_id || '';
              pendingAsyncRef.current.add(taskId);
            } catch { /* ignore */ }
            continue;
          }

          if (evt.event === 'confirm_required') {
            try {
              const confirm = JSON.parse(evt.data);
              setConfirmAction({
                actionId: confirm.action_id,
                prompt: confirm.prompt,
                info: confirm.info,
              });
            } catch { /* ignore */ }
            continue;
          }

          // Default: chat completion chunk
          if (!evt.event) {
            try {
              const chunk = JSON.parse(evt.data);
              if (chunk.error) {
                setError(chunk.error.message);
                continue;
              }
              const delta = chunk.choices?.[0]?.delta;
              if (delta?.content) {
                assistantContent += delta.content;
                if (isViewing) {
                  setMessages((prev) => {
                    const existing = prev.find((m) => m.id === 'streaming');
                    if (existing) {
                      return prev.map((m) =>
                        m.id === 'streaming' ? { ...m, content: assistantContent } : m
                      );
                    }
                    return [...prev, {
                      id: 'streaming',
                      role: 'assistant' as const,
                      content: assistantContent,
                      timestamp: Date.now(),
                    }];
                  });
                }
                // Always update the cache
                const cacheId = streamingConvIdRef.current || currentConvId || '__new';
                const cached = messagesCacheRef.current.get(cacheId);
                if (cached) {
                  const existing = cached.find((m) => m.id === 'streaming');
                  if (existing) {
                    messagesCacheRef.current.set(cacheId, cached.map((m) =>
                      m.id === 'streaming' ? { ...m, content: assistantContent } : m
                    ));
                  } else {
                    messagesCacheRef.current.set(cacheId, [...cached, {
                      id: 'streaming',
                      role: 'assistant' as const,
                      content: assistantContent,
                      timestamp: Date.now(),
                    }]);
                  }
                }
              }
              if (chunk.choices?.[0]?.finish_reason === 'stop') {
                if (isViewing) {
                  setMessages((prev) =>
                    prev.map((m) =>
                      m.id === 'streaming' ? { ...m, id: `asst-${Date.now()}` } : m
                    )
                  );
                }
              }
            } catch { /* ignore malformed JSON */ }
          }
        }
      }

      // Refresh conversation list
      if (assistantContent) {
        loadConversations();
      }
    } catch (err) {
      if ((err as Error).name === 'AbortError') {
        // User cancelled
      } else {
        setError((err as Error).message);
      }
    } finally {
      setIsStreaming(false);
      setStatusText('');
      streamingConvIdRef.current = null;
      streamingToolsRef.current = [];
      streamingStatusRef.current = '';
      abortRef.current = null;
      // Clear message cache for this conversation (now persisted in DB)
      if (currentConvId) {
        messagesCacheRef.current.delete(currentConvId);
      }
    }
  }, [input, images, messages, isStreaming, activeConvId, loadConversations]);

  // Stop streaming
  const handleStop = useCallback(() => {
    abortRef.current?.abort();
    setIsStreaming(false);
    setStatusText('');
    streamingConvIdRef.current = null;
    streamingToolsRef.current = [];
  }, []);

  // Keyboard handling
  const handleKeyDown = useCallback((e: React.KeyboardEvent<HTMLTextAreaElement>) => {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault();
      sendMessage();
    }
  }, [sendMessage]);

  // Auto-resize textarea
  const adjustTextareaHeight = useCallback(() => {
    const el = inputRef.current;
    if (el) {
      el.style.height = 'auto';
      el.style.height = Math.min(el.scrollHeight, 200) + 'px';
    }
  }, []);

  // Find the title of the streaming conversation for the activity bar
  const streamingConvTitle = isStreaming && streamingConvIdRef.current
    ? conversations.find((c) => c.id === streamingConvIdRef.current)?.title
    : undefined;

  return (
    <div className="flex-1 flex min-h-0 mx-4 sm:mx-8 mb-4"
      onDragOver={(e) => e.preventDefault()}
      onDrop={handleDrop}
    >
      {/* Sidebar: conversation history */}
      {sidebarOpen && (
        <div className="w-64 flex-shrink-0 border-r border-neutral-800 flex flex-col bg-neutral-950 rounded-l-lg">
          <div className="flex items-center justify-between p-3 border-b border-neutral-800">
            <span className="text-xs font-medium text-neutral-400">History</span>
            <div className="flex gap-1">
              <button onClick={startNewChat} className="p-1 text-neutral-500 hover:text-white transition-colors" title="New chat">
                <Plus className="w-3.5 h-3.5" />
              </button>
              <button onClick={() => setSidebarOpen(false)} className="p-1 text-neutral-500 hover:text-white transition-colors">
                <ChevronLeft className="w-3.5 h-3.5" />
              </button>
            </div>
          </div>
          <div className="flex-1 overflow-y-auto">
            {conversations.map((conv) => (
              <button
                key={conv.id}
                onClick={() => loadConversation(conv.id)}
                className={cn(
                  'w-full text-left px-3 py-2 text-xs border-b border-neutral-900 hover:bg-neutral-900 transition-colors',
                  activeConvId === conv.id ? 'bg-neutral-800/50 text-white' : 'text-neutral-400',
                )}
              >
                <div className="flex items-center gap-1.5">
                  <span className="truncate flex-1">{conv.title}</span>
                  {/* Show streaming indicator on the active streaming conversation */}
                  {isStreaming && streamingConvIdRef.current === conv.id && (
                    <Loader2 className="w-3 h-3 animate-spin text-amber-400 flex-shrink-0" />
                  )}
                </div>
                <div className="text-[10px] text-neutral-600 mt-0.5">
                  {conv.messageCount} messages
                </div>
              </button>
            ))}
            {conversations.length === 0 && (
              <div className="p-4 text-center text-neutral-600 text-xs">No conversations yet</div>
            )}
          </div>
        </div>
      )}

      {/* Main chat area */}
      <div className="flex-1 flex flex-col min-w-0 border border-neutral-800 rounded-lg overflow-hidden"
        style={sidebarOpen ? { borderTopLeftRadius: 0, borderBottomLeftRadius: 0 } : undefined}
      >
        {/* Chat header */}
        <div className="flex items-center justify-between px-4 py-2 border-b border-neutral-800 bg-neutral-950">
          <div className="flex items-center gap-2">
            {!sidebarOpen && (
              <button onClick={() => { loadConversations(); setSidebarOpen(true); }} className="p-1 text-neutral-500 hover:text-white transition-colors" title="History">
                <MessageSquare className="w-4 h-4" />
              </button>
            )}
            <span className="text-sm font-medium text-neutral-300">Jarvis</span>
            {/* Slot status dot */}
            {slotStatus && (
              <span className={cn(
                'w-2 h-2 rounded-full',
                slotStatus.state?.toLowerCase() === 'idle' ? 'bg-green-500' : 'bg-amber-400 animate-pulse',
              )} title={slotStatus.state} />
            )}
            {isThisConvStreaming && (
              <span className="flex items-center gap-1 text-[10px] text-amber-400">
                <Loader2 className="w-3 h-3 animate-spin" />
                {statusText || 'Processing...'}
              </span>
            )}
          </div>
          <div className="flex items-center gap-1">
            <button onClick={startNewChat} className="p-1.5 text-neutral-500 hover:text-white transition-colors" title="New chat">
              <Plus className="w-3.5 h-3.5" />
            </button>
          </div>
        </div>

        {/* Activity bar — shows when slot is busy (visible from any conversation) */}
        <SlotActivityBar status={slotStatus} streamingConvTitle={streamingConvTitle} />

        {/* Messages */}
        <div className="flex-1 overflow-y-auto px-4 py-4 space-y-4">
          {messages.length === 0 && !isThisConvStreaming && (
            <div className="flex flex-col items-center justify-center h-full text-neutral-600">
              {slotReady === false ? (
                <>
                  <Loader2 className="w-8 h-8 mb-3 text-amber-500 animate-spin" />
                  <p className="text-sm text-amber-400">Jarvis is waking up...</p>
                  <p className="text-xs mt-1 text-neutral-700">Starting slot-jarvis, this may take a moment</p>
                </>
              ) : slotReady === null ? (
                <>
                  <Loader2 className="w-8 h-8 mb-3 text-neutral-600 animate-spin" />
                  <p className="text-sm">Checking Jarvis status...</p>
                </>
              ) : (
                <>
                  <Brain className="w-10 h-10 mb-3 text-neutral-700" />
                  <p className="text-sm">Ask Jarvis anything</p>
                  <p className="text-xs mt-1 text-neutral-700">
                    {isSlotBusy
                      ? 'Jarvis is busy with another conversation. Your message will be queued.'
                      : 'Supports text, images, and multi-turn conversation'
                    }
                  </p>
                </>
              )}
            </div>
          )}

          {messages.map((msg) => (
            <div key={msg.id} className={cn(
              'flex',
              msg.role === 'user' ? 'justify-end' : 'justify-start',
            )}>
              <div className={cn(
                'max-w-[85%] rounded-xl px-4 py-3',
                msg.role === 'user'
                  ? 'bg-blue-600/20 text-blue-100 rounded-br-sm'
                  : 'bg-neutral-800/60 text-neutral-200 rounded-bl-sm',
              )}>
                {/* User images */}
                {msg.images && msg.images.length > 0 && (
                  <div className="flex flex-wrap gap-2 mb-2">
                    {msg.images.map((img, i) => (
                      <img key={i} src={img} alt="Uploaded" className="max-h-40 rounded-lg object-cover" />
                    ))}
                  </div>
                )}
                {/* Content */}
                {msg.role === 'assistant' ? (
                  <div className="prose prose-sm prose-invert max-w-none
                    prose-pre:bg-neutral-900 prose-pre:border prose-pre:border-neutral-700 prose-pre:rounded-lg
                    prose-code:text-amber-300 prose-code:bg-neutral-900/50 prose-code:px-1 prose-code:py-0.5 prose-code:rounded
                    prose-p:my-1.5 prose-headings:my-2 prose-ul:my-1 prose-li:my-0.5
                    prose-a:text-blue-400 prose-a:no-underline hover:prose-a:underline
                    prose-table:text-xs">
                    <ReactMarkdown remarkPlugins={[remarkGfm]}>
                      {cleanAssistantContent(msg.content)}
                    </ReactMarkdown>
                  </div>
                ) : (
                  <div className="text-sm whitespace-pre-wrap">{msg.content}</div>
                )}
              </div>
            </div>
          ))}

          {/* Tool activity — collapsible cards */}
          {tools.length > 0 && (
            <div className="flex justify-start">
              <div className="max-w-[85%] space-y-1">
                {tools.map((tool) => (
                  <ToolCard key={tool.id} tool={tool} />
                ))}
              </div>
            </div>
          )}

          {/* Confirm dialog / AskUserQuestion */}
          {confirmAction && (
            <div className="flex justify-start">
              <div className="max-w-[85%] bg-amber-900/20 border border-amber-700/40 rounded-lg px-4 py-3">
                {confirmAction.info?.options && confirmAction.info.options.length > 0 &&
                 !confirmAction.info.options.some((o) => {
                   const l = o.label.toLowerCase();
                   return l.startsWith('yes') || l.startsWith('allow');
                 }) ? (
                  <>
                    <p className="text-sm text-neutral-200 mb-3">{confirmAction.prompt}</p>
                    <div className="flex flex-col gap-1.5">
                      {confirmAction.info.options.map((opt, i) => (
                        <button
                          key={i}
                          onClick={async () => {
                            try {
                              const optNum = typeof opt.key === 'number' ? opt.key : i + 1;
                              await fetch(`/api/pty/confirm?slotId=slot-jarvis&option=${optNum}`, { method: 'POST' });
                            } catch { /* ignore */ }
                            setConfirmAction(null);
                          }}
                          className="w-full text-left px-3 py-2 text-xs bg-neutral-800/80 hover:bg-neutral-700 text-neutral-200 rounded-lg transition-colors border border-neutral-700/50 hover:border-neutral-600"
                        >
                          <span className="text-neutral-500 mr-2">{typeof opt.key === 'number' ? opt.key : i + 1}.</span>
                          {opt.label}
                        </button>
                      ))}
                    </div>
                  </>
                ) : (
                  <>
                    <div className="flex items-center gap-2 mb-2">
                      <AlertTriangle className="w-4 h-4 text-amber-400 flex-shrink-0" />
                      <span className="text-sm text-amber-300 font-medium">Permission Required</span>
                    </div>
                    <p className="text-xs text-neutral-300 mb-3">{confirmAction.prompt}</p>
                    {confirmAction.info?.tool && (
                      <p className="text-[11px] text-neutral-500 mb-2 font-mono">Tool: {confirmAction.info.tool}</p>
                    )}
                    <div className="flex gap-2">
                      <button
                        onClick={async () => {
                          try {
                            await fetch(`/api/pty/confirm?slotId=slot-jarvis&decision=allow`, { method: 'POST' });
                          } catch { /* ignore */ }
                          setConfirmAction(null);
                        }}
                        className="px-3 py-1 text-xs bg-emerald-600/80 hover:bg-emerald-600 text-white rounded transition-colors"
                      >
                        Allow
                      </button>
                      <button
                        onClick={async () => {
                          try {
                            await fetch(`/api/pty/confirm?slotId=slot-jarvis&decision=deny`, { method: 'POST' });
                          } catch { /* ignore */ }
                          setConfirmAction(null);
                        }}
                        className="px-3 py-1 text-xs bg-neutral-700/80 hover:bg-neutral-600 text-neutral-300 rounded transition-colors"
                      >
                        Deny
                      </button>
                    </div>
                  </>
                )}
              </div>
            </div>
          )}

          {/* Error display */}
          {error && (
            <div className="flex justify-center">
              <div className="bg-red-900/20 border border-red-800/30 text-red-400 text-xs px-3 py-2 rounded-lg max-w-md">
                {error}
              </div>
            </div>
          )}

          <div ref={messagesEndRef} />
        </div>

        {/* Image previews */}
        {images.length > 0 && (
          <div className="px-4 pb-2 flex gap-2 flex-wrap">
            {images.map((img, i) => (
              <div key={i} className="relative group">
                <img src={img} alt="Preview" className="h-16 rounded-lg object-cover border border-neutral-700" />
                <button
                  onClick={() => removeImage(i)}
                  className="absolute -top-1.5 -right-1.5 w-4 h-4 bg-neutral-700 rounded-full flex items-center justify-center
                    opacity-0 group-hover:opacity-100 transition-opacity"
                >
                  <X className="w-2.5 h-2.5 text-white" />
                </button>
              </div>
            ))}
          </div>
        )}

        {/* Input area */}
        <div className="border-t border-neutral-800 p-3 bg-neutral-950">
          <div className="flex items-end gap-2">
            <button
              onClick={() => fileInputRef.current?.click()}
              className="p-2 text-neutral-500 hover:text-white transition-colors flex-shrink-0"
              title="Upload image"
            >
              <ImagePlus className="w-4 h-4" />
            </button>
            <input
              ref={fileInputRef}
              type="file"
              accept="image/*"
              multiple
              className="hidden"
              onChange={(e) => handleImageUpload(e.target.files)}
            />

            <textarea
              ref={inputRef}
              value={input}
              onChange={(e) => { setInput(e.target.value); adjustTextareaHeight(); }}
              onKeyDown={handleKeyDown}
              onPaste={handlePaste}
              placeholder={
                isSlotBusy
                  ? 'Jarvis is busy — message will be queued...'
                  : isThisConvStreaming
                    ? 'Jarvis is responding...'
                    : 'Message Jarvis...'
              }
              rows={1}
              className={cn(
                'flex-1 bg-neutral-900 border rounded-lg px-3 py-2 text-sm text-white',
                'placeholder-neutral-600 resize-none focus:outline-none transition-colors',
                'min-h-[36px] max-h-[200px]',
                isThisConvStreaming
                  ? 'border-amber-800/40 cursor-not-allowed'
                  : 'border-neutral-800 focus:border-neutral-600',
              )}
              disabled={isThisConvStreaming}
            />

            {isThisConvStreaming ? (
              <button
                onClick={handleStop}
                className="p-2 bg-neutral-700 hover:bg-neutral-600 rounded-lg transition-colors flex-shrink-0"
                title="Stop"
              >
                <div className="w-3.5 h-3.5 bg-white rounded-sm" />
              </button>
            ) : (
              <button
                onClick={sendMessage}
                disabled={(!input.trim() && images.length === 0) || isStreaming}
                className={cn(
                  'p-2 rounded-lg transition-colors flex-shrink-0',
                  (input.trim() || images.length > 0) && !isStreaming
                    ? 'bg-blue-600 hover:bg-blue-500 text-white'
                    : 'bg-neutral-800 text-neutral-600 cursor-not-allowed',
                )}
                title={isSlotBusy ? 'Jarvis is busy with another conversation' : 'Send (Enter)'}
              >
                <Send className="w-4 h-4" />
              </button>
            )}
          </div>
          <p className="text-[10px] text-neutral-700 mt-1.5 text-center">
            Shift+Enter for newline · Paste or drag images · Context auto-injected
          </p>
        </div>
      </div>
    </div>
  );
}
