'use client';

import { useEffect, useRef, useState, useCallback, Component, type ReactNode } from 'react';

const WS_PORT = 9120;

interface TerminalProps {
  slotId: string;
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
function TerminalInner({ slotId }: TerminalProps) {
  const containerRef = useRef<HTMLDivElement>(null);
  const termRef = useRef<import('xterm').Terminal | null>(null);
  const wsRef = useRef<WebSocket | null>(null);
  const [wsStatus, setWsStatus] = useState<'connecting' | 'connected' | 'disconnected'>('disconnected');
  const [ptyState, setPtyState] = useState<PTYState>('unknown');
  const [statusText, setStatusText] = useState<string | null>(null);
  const [spawning, setSpawning] = useState(false);
  const [ready, setReady] = useState(false); // xterm initialized

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
        import('xterm'),
        import('@xterm/addon-fit'),
      ]);
      // CSS import - ignore TS error for CSS module
      try { await import('xterm/css/xterm.css' as string); } catch { /* bundled separately */ }

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

    // Close previous WS
    wsRef.current?.close();
    wsRef.current = null;
    term.clear();

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
          setPtyState('not_running');
          term.writeln('\x1b[90m● No active session. Press Start to launch Claude Code.\x1b[0m');
        }
      })
      .catch(() => {
        if (cancelled) return;
        setPtyState('not_running');
        term.writeln('\x1b[90m● Cannot reach missiond.\x1b[0m');
      });

    return () => { cancelled = true; };
  }, [slotId, ready]);

  function connectWs(term: import('xterm').Terminal, slot: string) {
    if (wsRef.current?.readyState === WebSocket.OPEN) return;

    setWsStatus('connecting');
    const ws = new WebSocket(`ws://localhost:${WS_PORT}/pty/${slot}`);
    wsRef.current = ws;

    ws.onopen = () => {
      setWsStatus('connected');
    };

    ws.onmessage = (event) => {
      try {
        const msg = JSON.parse(event.data);
        if (msg.type === 'data' && msg.data) {
          term.write(msg.data);
        } else if (msg.type === 'screen' && msg.data) {
          term.clear();
          term.write(msg.data);
        } else if (msg.type === 'state') {
          const newState = msg.state || 'unknown';
          setPtyState(newState);
          // Show statusText during processing; clear when idle/stopped
          const processing = ['thinking', 'responding', 'tool_running'].includes(newState);
          setStatusText(processing ? (msg.statusText || null) : null);
        } else if (msg.type === 'exit') {
          term.writeln(`\r\n\x1b[31m[exited: code ${msg.code}]\x1b[0m`);
          setWsStatus('disconnected');
          setPtyState('not_running');
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
        term.write(event.data);
      }
    };

    ws.onclose = () => {
      setWsStatus('disconnected');
      term.writeln(`\r\n\x1b[90m● Disconnected\x1b[0m`);
    };

    ws.onerror = () => {
      setWsStatus('disconnected');
    };
  }

  const handleConnect = useCallback(() => {
    const term = termRef.current;
    if (term) connectWs(term, slotId);
  }, [slotId]);

  const handleSpawn = useCallback(async () => {
    setSpawning(true);
    const term = termRef.current;
    try {
      term?.writeln('\x1b[33m● Starting Claude Code...\x1b[0m');
      let res = await fetch(`/api/pty/spawn?slotId=${slotId}`, { method: 'POST' });
      let data = await res.json();

      // If stale session exists, kill it and retry
      if (data.error && /already running/i.test(String(data.error))) {
        term?.writeln('\x1b[90m● Cleaning up stale session...\x1b[0m');
        await fetch(`/api/pty/kill?slotId=${slotId}`, { method: 'POST' });
        await new Promise((r) => setTimeout(r, 500));
        res = await fetch(`/api/pty/spawn?slotId=${slotId}`, { method: 'POST' });
        data = await res.json();
      }

      if (data.error) {
        term?.writeln(`\x1b[31m✗ ${data.error}\x1b[0m`);
        return;
      }
      term?.writeln(`\x1b[32m● Spawned (pid: ${data.pid || '?'})\x1b[0m\r\n`);
      setPtyState('starting');
      setTimeout(() => { if (term) connectWs(term, slotId); }, 500);
    } catch (err) {
      term?.writeln(`\x1b[31m✗ Failed: ${err}\x1b[0m`);
    } finally {
      setSpawning(false);
    }
  }, [slotId]);

  const handleKill = useCallback(async () => {
    try {
      await fetch(`/api/pty/kill?slotId=${slotId}`, { method: 'POST' });
      setPtyState('not_running');
      wsRef.current?.close();
      termRef.current?.writeln('\r\n\x1b[31m● Session killed\x1b[0m');
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

  return (
    <div className="flex flex-col h-full">
      <div className="flex items-center justify-between px-3 py-2 border-b border-neutral-800 bg-neutral-900/50">
        <div className="flex items-center gap-2">
          <span className={`w-2 h-2 rounded-full ${wsColor}`} />
          <span className="text-xs text-neutral-400 font-mono">{slotId}</span>
          <span className={`text-[10px] font-medium ${stateColor}`}>{statusText || stateText}</span>
        </div>
        <div className="flex gap-1.5">
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
      <div ref={containerRef} className="flex-1 min-h-0" />
    </div>
  );
}

// --- Export with SSR guard + error boundary ---
export function Terminal({ slotId }: TerminalProps) {
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
      <TerminalInner key={`${slotId}-${key}`} slotId={slotId} />
    </TerminalErrorBoundary>
  );
}
