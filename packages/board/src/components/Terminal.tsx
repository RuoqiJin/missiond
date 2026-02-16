import { useEffect, useRef, useState, useCallback } from 'react';
import { Terminal as XTerm } from 'xterm';
import { FitAddon } from '@xterm/addon-fit';
import 'xterm/css/xterm.css';

interface TerminalProps {
  slotId: string;
}

export function Terminal({ slotId }: TerminalProps) {
  const containerRef = useRef<HTMLDivElement>(null);
  const termRef = useRef<XTerm | null>(null);
  const wsRef = useRef<WebSocket | null>(null);
  const fitRef = useRef<FitAddon | null>(null);
  const [status, setStatus] = useState<'connecting' | 'connected' | 'disconnected'>('disconnected');

  const connect = useCallback(() => {
    if (wsRef.current?.readyState === WebSocket.OPEN) return;

    const term = termRef.current;
    if (!term) return;

    setStatus('connecting');
    // Use Vite proxy: /ws/pty/... → ws://localhost:9120/pty/...
    const protocol = location.protocol === 'https:' ? 'wss:' : 'ws:';
    const ws = new WebSocket(`${protocol}//${location.host}/ws/pty/${slotId}`);
    wsRef.current = ws;

    ws.onopen = () => {
      setStatus('connected');
      term.writeln(`\x1b[32m● Connected to ${slotId}\x1b[0m\r\n`);
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
          term.writeln(`\r\n\x1b[33m[state: ${msg.prevState} → ${msg.state}]\x1b[0m`);
        } else if (msg.type === 'exit') {
          term.writeln(`\r\n\x1b[31m[exited: code ${msg.code}]\x1b[0m`);
          setStatus('disconnected');
        }
      } catch {
        // Raw text data
        term.write(event.data);
      }
    };

    ws.onclose = () => {
      setStatus('disconnected');
      term.writeln(`\r\n\x1b[90m● Disconnected\x1b[0m`);
    };

    ws.onerror = () => {
      setStatus('disconnected');
    };
  }, [slotId]);

  useEffect(() => {
    if (!containerRef.current) return;

    const term = new XTerm({
      theme: {
        background: '#0a0a0a',
        foreground: '#d4d4d4',
        cursor: '#d4d4d4',
        selectionBackground: '#264f78',
        black: '#1e1e1e',
        red: '#f44747',
        green: '#6a9955',
        yellow: '#dcdcaa',
        blue: '#569cd6',
        magenta: '#c586c0',
        cyan: '#4ec9b0',
        white: '#d4d4d4',
        brightBlack: '#808080',
        brightRed: '#f44747',
        brightGreen: '#6a9955',
        brightYellow: '#dcdcaa',
        brightBlue: '#569cd6',
        brightMagenta: '#c586c0',
        brightCyan: '#4ec9b0',
        brightWhite: '#e5e5e5',
      },
      fontSize: 13,
      fontFamily: "'SF Mono', 'Fira Code', 'Cascadia Code', Menlo, monospace",
      cursorBlink: true,
      scrollback: 10000,
      convertEol: true,
    });

    const fit = new FitAddon();
    term.loadAddon(fit);
    term.open(containerRef.current);
    fit.fit();

    termRef.current = term;
    fitRef.current = fit;

    term.writeln('\x1b[90m● Ready. Waiting for connection...\x1b[0m');

    // Handle input (send to PTY)
    term.onData((data) => {
      if (wsRef.current?.readyState === WebSocket.OPEN) {
        wsRef.current.send(JSON.stringify({ type: 'input', data }));
      }
    });

    // Auto-fit on resize
    const observer = new ResizeObserver(() => {
      try { fit.fit(); } catch { /* ignore */ }
    });
    observer.observe(containerRef.current);

    return () => {
      observer.disconnect();
      wsRef.current?.close();
      term.dispose();
    };
  }, []);

  // Auto-connect when slotId changes
  useEffect(() => {
    // Close existing connection
    wsRef.current?.close();
    termRef.current?.clear();
    connect();
  }, [slotId, connect]);

  const statusColor = status === 'connected' ? 'bg-green-500' : status === 'connecting' ? 'bg-yellow-500' : 'bg-neutral-600';

  return (
    <div className="flex flex-col h-full">
      {/* Toolbar */}
      <div className="flex items-center justify-between px-3 py-2 border-b border-neutral-800 bg-neutral-900/50">
        <div className="flex items-center gap-2">
          <span className={`w-2 h-2 rounded-full ${statusColor}`} />
          <span className="text-xs text-neutral-400 font-mono">{slotId}</span>
          <span className="text-[10px] text-neutral-600">{status}</span>
        </div>
        <div className="flex gap-1.5">
          {status === 'disconnected' && (
            <button
              onClick={connect}
              className="text-[10px] px-2 py-0.5 rounded bg-neutral-800 text-neutral-400 hover:text-white hover:bg-neutral-700 transition-colors"
            >
              Reconnect
            </button>
          )}
        </div>
      </div>

      {/* Terminal */}
      <div ref={containerRef} className="flex-1 min-h-0" />
    </div>
  );
}
