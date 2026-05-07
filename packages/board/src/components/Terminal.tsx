'use client';

import { useEffect, useRef, useState, useCallback, Component, type ReactNode } from 'react';
import type { SlotDef } from '../types';

const WS_PORT = parseInt(process.env.NEXT_PUBLIC_WS_PORT || '9120', 10);

interface TerminalActiveTask {
  id: string;
  title: string;
  status?: string;
}

interface TerminalProps {
  slotId: string;
  slot?: SlotDef;
  activeTask?: TerminalActiveTask | null;
}

const STATUS_TEXT_MAX = 80;
function truncateStatusText(value: string | null | undefined): string | null {
  if (!value) return null;
  const flat = value.replace(/\s+/g, ' ').trim();
  if (!flat) return null;
  return flat.length > STATUS_TEXT_MAX ? `${flat.slice(0, STATUS_TEXT_MAX - 1)}…` : flat;
}

function durableConversationFallback(slot?: SlotDef): string[] {
  const conv = slot?.latestConversation;
  if (!conv?.id) return [];
  const lines = [
    `● No live PTY. Showing durable conversation for this slot.`,
    `  conversation: ${conv.id}`,
  ];
  if (conv.source) lines.push(`  source: ${conv.source}`);
  if (conv.status) lines.push(`  status: ${conv.status}`);
  if (conv.messageCount != null) lines.push(`  messages: ${conv.messageCount}`);
  if (conv.updatedAt) lines.push(`  updated: ${conv.updatedAt}`);
  if (conv.title) lines.push(`  title: ${conv.title}`);
  return lines;
}

type PTYState = 'unknown' | 'not_running' | 'starting' | 'idle' | 'slash_menu' | 'thinking' | 'responding' | 'tool_running' | 'confirming' | 'error' | 'exited';

// --- Error Boundary ---
class TerminalErrorBoundary extends Component<
  { children: ReactNode; onReset: () => void },
  { error: Error | null }
> {
  state = { error: null as Error | null };
  static getDerivedStateFromError(error: Error) { return { error }; }
  render() {
    if (this.state.error) {
      return (
        <div className="flex flex-col items-center justify-center h-full gap-3 text-neutral-400">
          <p className="text-red-400 text-sm font-mono">Terminal Error: {this.state.error.message}</p>
          <button
            onClick={() => { this.setState({ error: null }); this.props.onReset(); }}
            className="text-xs px-3 py-1 rounded bg-neutral-800 hover:bg-neutral-700 transition-colors"
          >
            Retry
          </button>
        </div>
      );
    }
    return this.props.children;
  }
}

// --- Terminal Inner ---
function TerminalInner({ slotId, slot, activeTask }: TerminalProps) {
  const containerRef = useRef<HTMLDivElement>(null);
  const termRef = useRef<import('@xterm/xterm').Terminal | null>(null);
  const wsRef = useRef<WebSocket | null>(null);
  const reconnectTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const reconnectAttemptRef = useRef(0);
  const [wsStatus, setWsStatus] = useState<'connecting' | 'connected' | 'disconnected'>('disconnected');
  const [ptyState, setPtyState] = useState<PTYState>('unknown');
  const [statusText, setStatusText] = useState<string | null>(null);
  const [spawning, setSpawning] = useState(false);
  const [ready, setReady] = useState(false); // xterm initialized
  const providerLabel = slotId.includes('gemini')
    ? 'Gemini CLI'
    : slotId.includes('codex')
      ? 'Codex CLI'
      : slotId.includes('claude')
        ? 'Claude CLI'
        : 'session';

  // --- Init xterm (once) ---
  useEffect(() => {
    const el = containerRef.current;
    if (!el) return;

    let disposed = false;

    (async () => {
      // Wait for container to have dimensions
      await new Promise<void>((resolve) => {
        const check = () => {
          if (disposed) return;
          if (el.clientWidth > 0 && el.clientHeight > 0) resolve();
          else requestAnimationFrame(check);
        };
        check();
      });
      if (disposed) return;

      const [{ Terminal }, { FitAddon }] = await Promise.all([
        import('@xterm/xterm'),
        import('@xterm/addon-fit'),
      ]);
      // CSS import - ignore TS error for CSS module
      try { await import('@xterm/xterm/css/xterm.css' as string); } catch { /* bundled separately */ }

      if (disposed) return;

      const term = new Terminal({
        theme: {
          background: '#0a0a0a', foreground: '#d4d4d4', cursor: '#d4d4d4',
          selectionBackground: '#264f78',
          black: '#1e1e1e', red: '#f44747', green: '#6a9955', yellow: '#dcdcaa',
          blue: '#569cd6', magenta: '#c586c0', cyan: '#4ec9b0', white: '#d4d4d4',
          brightBlack: '#808080', brightRed: '#f44747', brightGreen: '#6a9955',
          brightYellow: '#dcdcaa', brightBlue: '#569cd6', brightMagenta: '#c586c0',
          brightCyan: '#4ec9b0', brightWhite: '#e5e5e5',
        },
        fontSize: 13,
        fontFamily: "'SF Mono', 'Fira Code', 'Cascadia Code', Menlo, monospace",
        cursorBlink: true,
        scrollback: 10000,
        convertEol: true,
      });

      const fit = new FitAddon();
      term.loadAddon(fit);
      term.open(el);
      fit.fit();
      termRef.current = term;

      term.onData((data) => {
        if (wsRef.current?.readyState === WebSocket.OPEN) {
          wsRef.current.send(JSON.stringify({ type: 'input', data }));
        }
      });

      const observer = new ResizeObserver(() => {
        try { fit.fit(); } catch { /* ignore */ }
      });
      observer.observe(el);

      setReady(true);

      // Cleanup stored for unmount
      const cleanup = () => {
        observer.disconnect();
        term.dispose();
      };
      el.dataset.cleanup = 'true';
      (el as any).__cleanup = cleanup;
    })();

    return () => {
      disposed = true;
      if (reconnectTimerRef.current) {
        clearTimeout(reconnectTimerRef.current);
        reconnectTimerRef.current = null;
      }
      wsRef.current?.close();
      wsRef.current = null;
      if ((el as any).__cleanup) {
        (el as any).__cleanup();
        delete (el as any).__cleanup;
      }
      termRef.current = null;
      setReady(false);
    };
  }, []); // Only init once

  // --- Connect WS when slotId changes and xterm is ready ---
  useEffect(() => {
    if (!ready) return;
    const term = termRef.current;
    if (!term) return;

    // Close previous WS and cancel pending reconnects
    clearReconnectTimer();
    reconnectAttemptRef.current = 0;
    wsRef.current?.close();
    wsRef.current = null;
    safeClear(term);

    let cancelled = false;

    // Check status first, then connect WS only if running
    fetch(`/api/pty/status?slotId=${slotId}`)
      .then((r) => r.json())
      .then((data) => {
        if (cancelled) return;
        if (data.running) {
          setPtyState(data.state || 'idle');
          connectWs(term, slotId);
        } else {
          setPtyState(data.state === 'exited' ? 'exited' : 'not_running');
          const fallback = durableConversationFallback(slot);
          if (fallback.length > 0) {
            for (const line of fallback) safeWriteln(term, `\x1b[90m${line}\x1b[0m`);
            safeWriteln(term, `\x1b[90m  Open Logs/Exec for full durable transcript, or press Start to launch ${providerLabel}.\x1b[0m`);
          } else {
            safeWriteln(term, `\x1b[90m● No active session. Press Start to launch ${providerLabel}.\x1b[0m`);
          }
        }
      })
      .catch(() => {
        if (cancelled) return;
        setPtyState('not_running');
        safeWriteln(term, '\x1b[90m● Cannot reach missiond.\x1b[0m');
      });

    return () => { cancelled = true; };
  }, [
    slotId,
    ready,
    providerLabel,
    slot?.latestConversation?.id,
    slot?.latestConversation?.source,
    slot?.latestConversation?.status,
    slot?.latestConversation?.messageCount,
    slot?.latestConversation?.updatedAt,
    slot?.latestConversation?.title,
  ]);

  function clearReconnectTimer() {
    if (reconnectTimerRef.current) {
      clearTimeout(reconnectTimerRef.current);
      reconnectTimerRef.current = null;
    }
  }

  function scheduleReconnect(term: import('@xterm/xterm').Terminal, slot: string) {
    clearReconnectTimer();
    const attempt = reconnectAttemptRef.current;
    // Exponential backoff: 1s, 2s, 4s, 8s, max 15s
    const delay = Math.min(1000 * Math.pow(2, attempt), 15000);
    reconnectTimerRef.current = setTimeout(() => {
      reconnectAttemptRef.current = attempt + 1;
      connectWs(term, slot);
    }, delay);
  }

  /** Check if terminal container has dimensions (safe to write) */
  function isTermReady(term: import('@xterm/xterm').Terminal): boolean {
    const el = containerRef.current;
    return !!(el && el.clientWidth > 0 && el.clientHeight > 0 && (term as any)._core);
  }

  /** Safe write — skip if container has no dimensions to avoid xterm runtime error */
  function safeWrite(term: import('@xterm/xterm').Terminal, data: string) {
    try {
      if (isTermReady(term)) term.write(data);
    } catch { /* swallow dimensions error */ }
  }

  function safeWriteln(term: import('@xterm/xterm').Terminal, data: string) {
    try {
      if (isTermReady(term)) term.writeln(data);
    } catch { /* swallow dimensions error */ }
  }

  function safeClear(term: import('@xterm/xterm').Terminal) {
    try {
      if (isTermReady(term)) term.clear();
    } catch { /* swallow dimensions error */ }
  }

  function connectWs(term: import('@xterm/xterm').Terminal, slot: string) {
    if (wsRef.current?.readyState === WebSocket.OPEN ||
        wsRef.current?.readyState === WebSocket.CONNECTING) return;

    clearReconnectTimer();
    setWsStatus('connecting');
    const wsHost = process.env.NEXT_PUBLIC_WS_HOST || window.location.hostname;
    // When accessed via reverse proxy (Caddy), use same port as page (proxy routes /pty/* to MissionD).
    // Locally, NEXT_PUBLIC_WS_PORT=9120 connects directly to MissionD.
    const isLocal = wsHost === 'localhost' || wsHost === '127.0.0.1';
    const wsPort = isLocal ? WS_PORT : (parseInt(window.location.port, 10) || WS_PORT);
    const ws = new WebSocket(`ws://${wsHost}:${wsPort}/pty/${slot}`);
    wsRef.current = ws;

    ws.onopen = () => {
      setWsStatus('connected');
      reconnectAttemptRef.current = 0;
    };

    ws.onmessage = (event) => {
      try {
        const msg = JSON.parse(event.data);
        if (msg.type === 'data' && msg.data) {
          safeWrite(term, msg.data);
        } else if (msg.type === 'screen' && msg.data) {
          safeClear(term);
          safeWrite(term, msg.data);
        } else if (msg.type === 'state') {
          const newState = msg.state || 'unknown';
          setPtyState(newState);
          // Show statusText during processing; clear when idle/stopped
          const processing = ['thinking', 'responding', 'tool_running'].includes(newState);
          setStatusText(processing ? truncateStatusText(msg.statusText) : null);
        } else if (msg.type === 'exit') {
          safeWriteln(term, `\r\n\x1b[31m[exited: code ${msg.code}]\x1b[0m`);
          clearReconnectTimer();
          setWsStatus('disconnected');
          setPtyState('not_running');
          // Server will close with 4003 after this, onclose won't auto-reconnect
          return;
        } else if (msg.type === 'screenshot_request') {
          const requestId = msg.requestId;
          try {
            const el = containerRef.current;
            const screenEl = el?.querySelector('.xterm-screen');
            if (!screenEl || !term) {
              ws.send(JSON.stringify({ type: 'screenshot_response', requestId, error: 'No terminal' }));
              return;
            }
            const canvases = screenEl.querySelectorAll('canvas');
            if (!canvases.length) {
              ws.send(JSON.stringify({ type: 'screenshot_response', requestId, error: 'No canvas' }));
              return;
            }
            const w = canvases[0].width;
            const h = canvases[0].height;
            const composite = document.createElement('canvas');
            composite.width = w;
            composite.height = h;
            const ctx = composite.getContext('2d')!;
            canvases.forEach(c => ctx.drawImage(c, 0, 0));
            const base64 = composite.toDataURL('image/png').replace(/^data:image\/png;base64,/, '');
            ws.send(JSON.stringify({ type: 'screenshot_response', requestId, data: base64, width: w, height: h }));
          } catch (e) {
            ws.send(JSON.stringify({ type: 'screenshot_response', requestId, error: String(e) }));
          }
        }
      } catch {
        safeWrite(term, event.data);
      }
    };

    ws.onclose = (event) => {
      setWsStatus('disconnected');
      // 4001 = session not found, 4003 = PTY exited — don't auto-reconnect
      if (event.code === 4001 || event.code === 4003) {
        safeWriteln(term, `\r\n\x1b[90m● Disconnected\x1b[0m`);
        return;
      }
      // Auto-reconnect for unexpected disconnects
      safeWriteln(term, `\r\n\x1b[90m● Disconnected — reconnecting...\x1b[0m`);
      scheduleReconnect(term, slot);
    };

    ws.onerror = () => {
      // onclose will fire after onerror, reconnect is handled there
    };
  }

  const handleConnect = useCallback(() => {
    const term = termRef.current;
    if (term) {
      reconnectAttemptRef.current = 0;
      connectWs(term, slotId);
    }
  }, [slotId]);

  const handleSpawn = useCallback(async () => {
    setSpawning(true);
    const term = termRef.current;
    try {
      if (term) safeWriteln(term, `\x1b[33m● Starting session (${providerLabel})...\x1b[0m`);
      let res = await fetch(`/api/pty/spawn?slotId=${slotId}`, { method: 'POST' });
      let data = await res.json();

      // If stale session exists, kill it and retry
      if (data.error && /already running/i.test(String(data.error))) {
        if (term) safeWriteln(term, '\x1b[90m● Cleaning up stale session...\x1b[0m');
        await fetch(`/api/pty/kill?slotId=${slotId}`, { method: 'POST' });
        await new Promise((r) => setTimeout(r, 500));
        res = await fetch(`/api/pty/spawn?slotId=${slotId}`, { method: 'POST' });
        data = await res.json();
      }

      if (data.error) {
        if (term) safeWriteln(term, `\x1b[31m✗ ${data.error}\x1b[0m`);
        return;
      }
      if (term) safeWriteln(term, `\x1b[32m● Spawned (pid: ${data.pid || '?'})\x1b[0m\r\n`);
      setPtyState('starting');
      setTimeout(() => { if (term) connectWs(term, slotId); }, 500);
    } catch (err) {
      if (term) safeWriteln(term, `\x1b[31m✗ Failed: ${err}\x1b[0m`);
    } finally {
      setSpawning(false);
    }
  }, [slotId]);

  const handleKill = useCallback(async () => {
    try {
      clearReconnectTimer();
      await fetch(`/api/pty/kill?slotId=${slotId}`, { method: 'POST' });
      setPtyState('not_running');
      wsRef.current?.close();
      if (termRef.current) safeWriteln(termRef.current, '\r\n\x1b[31m● Session killed\x1b[0m');
    } catch { /* ignore */ }
  }, [slotId]);

  const wsColor = wsStatus === 'connected' ? 'bg-green-500' : wsStatus === 'connecting' ? 'bg-yellow-500' : 'bg-neutral-600';

  const stateLabel: Record<PTYState, { text: string; color: string }> = {
    unknown: { text: '...', color: 'text-neutral-500' },
    not_running: { text: 'Stopped', color: 'text-neutral-500' },
    starting: { text: 'Starting', color: 'text-yellow-400' },
    idle: { text: 'Idle', color: 'text-green-400' },
    slash_menu: { text: '/ Menu', color: 'text-green-300' },
    thinking: { text: 'Thinking', color: 'text-blue-400' },
    responding: { text: 'Responding', color: 'text-purple-400' },
    tool_running: { text: 'Tool Running', color: 'text-cyan-400' },
    confirming: { text: 'Confirming', color: 'text-orange-400' },
    error: { text: 'Error', color: 'text-red-400' },
    exited: { text: 'Exited', color: 'text-neutral-500' },
  };

  const { text: stateText, color: stateColor } = stateLabel[ptyState] ?? { text: ptyState, color: 'text-neutral-500' };
  const isRunning = ptyState !== 'not_running' && ptyState !== 'unknown' && ptyState !== 'exited';
  const headerStatus = statusText ?? null;
  const providerBits = [slot?.provider, slot?.engine, slot?.modelProfile, slot?.taskClass].filter(
    (v): v is string => !!v && v.length > 0,
  );
  const mcpLabel =
    slot?.mcpReady === undefined
      ? null
      : slot.mcpReady
        ? 'MCP ready'
        : slot.mcpEnabled === false
          ? 'MCP missing'
          : slot.mcpApprovalReady === false
            ? 'MCP approval'
            : 'MCP missing';
  const lastActivity = slot?.latestConversation?.updatedAt ?? null;
  const lastActivityLabel = lastActivity ? new Date(lastActivity).toLocaleTimeString() : null;
  const showInfoRow = !!(activeTask || providerBits.length > 0 || mcpLabel || slot?.activeTool || slot?.blockedKind || lastActivityLabel);

  return (
    <div className="flex flex-col h-full">
      <div className="flex flex-col gap-0.5 px-3 py-2 border-b border-neutral-800 bg-neutral-900/50">
        <div className="flex items-center justify-between gap-2">
        <div className="flex items-center gap-2 min-w-0 flex-1">
          <span className={`w-2 h-2 rounded-full shrink-0 ${wsColor}`} />
          <span className="text-xs text-neutral-400 font-mono truncate shrink-0" title={slotId}>{slotId}</span>
          {slot?.label && slot.label !== slotId && (
            <span className="text-[10px] text-neutral-500 truncate min-w-0" title={slot.label}>· {slot.label}</span>
          )}
          <span
            className={`text-[10px] font-medium shrink-0 ${stateColor}`}
            title={headerStatus ?? stateText}
          >
            {headerStatus ?? stateText}
          </span>
        </div>
        <div className="flex gap-1.5 shrink-0">
          {!isRunning && (
            <button onClick={handleSpawn} disabled={spawning}
              className="text-[10px] px-2 py-0.5 rounded bg-green-900/50 text-green-400 hover:bg-green-800/50 hover:text-green-300 transition-colors disabled:opacity-50">
              {spawning ? 'Starting...' : 'Start'}
            </button>
          )}
          {isRunning && wsStatus === 'disconnected' && (
            <button onClick={handleConnect}
              className="text-[10px] px-2 py-0.5 rounded bg-neutral-800 text-neutral-400 hover:text-white hover:bg-neutral-700 transition-colors">
              Reconnect
            </button>
          )}
          {isRunning && (
            <button onClick={handleKill}
              className="text-[10px] px-2 py-0.5 rounded bg-red-900/30 text-red-400 hover:bg-red-800/40 hover:text-red-300 transition-colors">
              Stop
            </button>
          )}
        </div>
        </div>
        {showInfoRow && (
          <div className="flex items-center gap-2 min-w-0 text-[10px] text-neutral-500">
            {activeTask && (
              <span className="flex items-center gap-1 min-w-0 shrink truncate" title={`${activeTask.id}\n${activeTask.title}`}>
                <span className="font-mono text-neutral-600 shrink-0">#{activeTask.id.slice(0, 8)}</span>
                <span className="truncate text-neutral-400">{activeTask.title}</span>
              </span>
            )}
            {providerBits.length > 0 && (
              <span className="text-neutral-600 truncate shrink-0" title={providerBits.join(' · ')}>
                {activeTask ? '· ' : ''}{providerBits.join(' · ')}
              </span>
            )}
            {mcpLabel && (
              <span
                className={`shrink-0 truncate ${slot?.mcpReady ? 'text-emerald-400/70' : 'text-amber-400/80'}`}
                title={[
                  slot?.mcpSource,
                  slot?.mcpApprovalMissingTools?.length
                    ? `missing approvals: ${slot.mcpApprovalMissingTools.join(', ')}`
                    : null,
                ].filter(Boolean).join('\n')}
              >
                · {mcpLabel}
              </span>
            )}
            {slot?.activeTool && (
              <span className="text-neutral-600 shrink-0 truncate" title={slot.activeTool}>· tool: {slot.activeTool}</span>
            )}
            {slot?.blockedKind && (
              <span className="text-amber-400/80 shrink-0 truncate" title={slot.blockedKind}>· blocked: {slot.blockedKind}</span>
            )}
            {lastActivityLabel && (
              <span className="ml-auto text-neutral-700 shrink-0" title={lastActivity ?? ''}>last: {lastActivityLabel}</span>
            )}
          </div>
        )}
      </div>
      <div ref={containerRef} className="flex-1 min-h-0" />
    </div>
  );
}

// --- Export with SSR guard + error boundary ---
export function Terminal({ slotId, slot, activeTask }: TerminalProps) {
  const [key, setKey] = useState(0);
  const [mounted, setMounted] = useState(false);
  useEffect(() => { setMounted(true); }, []);

  if (!mounted) {
    return (
      <div className="flex flex-col h-full">
        <div className="flex items-center px-3 py-2 border-b border-neutral-800 bg-neutral-900/50">
          <span className="w-2 h-2 rounded-full bg-neutral-600" />
          <span className="text-xs text-neutral-400 font-mono ml-2">{slotId}</span>
        </div>
        <div className="flex-1" />
      </div>
    );
  }

  return (
    <TerminalErrorBoundary onReset={() => setKey((k) => k + 1)}>
      <TerminalInner key={`${slotId}-${key}`} slotId={slotId} slot={slot} activeTask={activeTask} />
    </TerminalErrorBoundary>
  );
}
