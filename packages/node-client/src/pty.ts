/**
 * PTY Session and Manager
 *
 * Provides WebSocket-based PTY session management.
 * PTYSession connects directly to the WebSocket server for real-time interaction.
 * PTYManager uses the MissionControl client for lifecycle operations.
 */

import { EventEmitter } from 'events';
import WebSocket from 'ws';
import type {
  SessionState,
  PTYSessionInfo,
  PTYSpawnOptions,
  PTYOutMessage,
  PTYSessionEvents,
  TextOutputEvent,
  ConfirmInfo,
  ClaudeCodeStatus,
  ClaudeCodeToolOutput,
} from './types.js';
import type { MissionControl } from './client.js';

// ============ Type-safe EventEmitter ============

type EventMap = PTYSessionEvents;
type EventKey = keyof EventMap;

class TypedEventEmitter extends EventEmitter {
  override emit<K extends EventKey>(event: K, ...args: EventMap[K]): boolean {
    return super.emit(event, ...args);
  }

  override on<K extends EventKey>(event: K, listener: (...args: EventMap[K]) => void): this {
    return super.on(event, listener as (...args: unknown[]) => void);
  }

  override once<K extends EventKey>(event: K, listener: (...args: EventMap[K]) => void): this {
    return super.once(event, listener as (...args: unknown[]) => void);
  }

  override off<K extends EventKey>(event: K, listener: (...args: EventMap[K]) => void): this {
    return super.off(event, listener as (...args: unknown[]) => void);
  }

  override removeListener<K extends EventKey>(
    event: K,
    listener: (...args: EventMap[K]) => void
  ): this {
    return super.removeListener(event, listener as (...args: unknown[]) => void);
  }
}

// ============ PTYSession ============

/**
 * PTY Session - Interactive terminal session connected via WebSocket
 *
 * Events:
 * - 'data' (data: string) - Raw data from PTY
 * - 'screen' (screen: string) - Full screen content received
 * - 'state' (state: SessionState, prevState: SessionState) - State changed
 * - 'text' (event: TextOutputEvent) - Text output (streaming or complete)
 * - 'confirm' (info: ConfirmInfo) - Confirmation required
 * - 'status' (status: ClaudeCodeStatus) - Status bar update
 * - 'tool' (output: ClaudeCodeToolOutput) - Tool output parsed
 * - 'exit' (code: number) - Session exited
 * - 'error' (error: Error) - Error occurred
 *
 * @example
 * ```typescript
 * const session = new PTYSession(ws, 'slot-1');
 *
 * session.on('data', (data) => process.stdout.write(data));
 * session.on('state', (state, prev) => console.log(`State: ${prev} -> ${state}`));
 * session.on('exit', (code) => console.log(`Exited with code ${code}`));
 *
 * // Send a message and wait for response
 * const response = await session.send('Hello, Claude!');
 *
 * // Or write directly without waiting
 * session.write('Some input\n');
 *
 * // Handle confirmation dialogs
 * session.on('confirm', (info) => {
 *   console.log('Confirm:', info);
 *   session.confirm('yes');
 * });
 *
 * // Close when done
 * session.close();
 * ```
 */
export class PTYSession extends TypedEventEmitter {
  private ws: WebSocket;
  private _slotId: string;
  private _state: SessionState = 'starting';
  private _screen = '';
  private _connected = false;
  private pendingPromise: {
    resolve: (value: string) => void;
    reject: (error: Error) => void;
    timeout: NodeJS.Timeout;
  } | null = null;

  /**
   * Create a PTYSession from an existing WebSocket connection
   *
   * @param ws - WebSocket connection to the PTY endpoint
   * @param slotId - The slot ID this session is attached to
   */
  constructor(ws: WebSocket, slotId: string) {
    super();
    this.ws = ws;
    this._slotId = slotId;

    this.setupWebSocket();
  }

  /**
   * Get the slot ID
   */
  get slotId(): string {
    return this._slotId;
  }

  /**
   * Get the current session state
   */
  get state(): SessionState {
    return this._state;
  }

  /**
   * Check if the session is connected
   */
  get connected(): boolean {
    return this._connected && this.ws.readyState === WebSocket.OPEN;
  }

  private setupWebSocket(): void {
    this.ws.on('open', () => {
      this._connected = true;
    });

    this.ws.on('message', (data: WebSocket.Data) => {
      try {
        const msg = JSON.parse(data.toString()) as PTYOutMessage;
        this.handleMessage(msg);
      } catch (err) {
        // Try to handle as raw text
        this.emit('data', data.toString());
      }
    });

    this.ws.on('close', (code, reason) => {
      this._connected = false;
      this._state = 'exited';
      this.emit('exit', code || 0);

      // Reject any pending promise
      if (this.pendingPromise) {
        clearTimeout(this.pendingPromise.timeout);
        this.pendingPromise.reject(new Error('WebSocket closed'));
        this.pendingPromise = null;
      }
    });

    this.ws.on('error', (err) => {
      this.emit('error', err instanceof Error ? err : new Error(String(err)));
    });

    // If already open, mark as connected
    if (this.ws.readyState === WebSocket.OPEN) {
      this._connected = true;
    }
  }

  private handleMessage(msg: PTYOutMessage): void {
    switch (msg.type) {
      case 'screen':
        this._screen = msg.data;
        this.emit('screen', msg.data);
        this.emit('data', msg.data);
        break;

      case 'data':
        this._screen += msg.data;
        this.emit('data', msg.data);
        break;

      case 'state':
        const prevState = this._state;
        this._state = msg.state;
        this.emit('state', msg.state, msg.prevState);

        // If we're waiting for a response and state becomes idle, resolve
        if (this.pendingPromise && msg.state === 'idle' && prevState !== 'idle') {
          clearTimeout(this.pendingPromise.timeout);
          this.pendingPromise.resolve(this._screen);
          this.pendingPromise = null;
        }
        break;

      case 'exit':
        this._state = 'exited';
        this.emit('exit', msg.code);

        // Reject any pending promise
        if (this.pendingPromise) {
          clearTimeout(this.pendingPromise.timeout);
          this.pendingPromise.reject(new Error(`Session exited with code ${msg.code}`));
          this.pendingPromise = null;
        }
        break;
    }
  }

  /**
   * Send a message and wait for Claude's response
   *
   * @param message - The message to send
   * @param timeoutMs - Timeout in milliseconds (default: 300000 = 5 minutes)
   * @returns Promise that resolves with the screen content when idle
   */
  async send(message: string, timeoutMs = 300000): Promise<string> {
    if (!this.connected) {
      throw new Error('PTY session not connected');
    }

    if (this._state !== 'idle') {
      throw new Error(`Cannot send message in state: ${this._state}`);
    }

    return new Promise((resolve, reject) => {
      // Clear any existing pending promise
      if (this.pendingPromise) {
        clearTimeout(this.pendingPromise.timeout);
        this.pendingPromise.reject(new Error('Superseded by new send'));
      }

      const timeout = setTimeout(() => {
        if (this.pendingPromise) {
          this.pendingPromise = null;
          reject(new Error('Timeout waiting for response'));
        }
      }, timeoutMs);

      this.pendingPromise = { resolve, reject, timeout };

      // Clear screen buffer and send message
      this._screen = '';
      this.write(message + '\n');
    });
  }

  /**
   * Write data directly to the PTY (no state checks, no waiting)
   *
   * @param data - The data to write
   */
  write(data: string): void {
    if (!this.connected) {
      throw new Error('PTY session not connected');
    }
    this.ws.send(JSON.stringify({ type: 'input', data }));
  }

  /**
   * Send a confirmation response
   *
   * @param response - 'yes', 'no', or option number
   */
  confirm(response: 'yes' | 'no' | number): void {
    if (!this.connected) {
      throw new Error('PTY session not connected');
    }

    let input: string;
    if (response === 'yes') {
      // First option, just Enter
      input = '\r';
    } else if (response === 'no') {
      // Third option (No): Down, Down, Enter
      input = '\x1b[B\x1b[B\r';
    } else if (typeof response === 'number') {
      // Navigate to option N (1-indexed)
      const downs = '\x1b[B'.repeat(Math.max(0, response - 1));
      input = downs + '\r';
    } else {
      input = '\r';
    }

    this.ws.send(JSON.stringify({ type: 'input', data: input }));
  }

  /**
   * Send interrupt signal (Ctrl+C)
   */
  interrupt(): void {
    if (!this.connected) {
      throw new Error('PTY session not connected');
    }
    this.ws.send(JSON.stringify({ type: 'input', data: '\x03' }));
  }

  /**
   * Get the current screen content
   *
   * @returns The screen content
   */
  async getScreen(): Promise<string> {
    return this._screen;
  }

  /**
   * Close the PTY session
   */
  close(): void {
    if (this.pendingPromise) {
      clearTimeout(this.pendingPromise.timeout);
      this.pendingPromise.reject(new Error('Session closed'));
      this.pendingPromise = null;
    }

    if (this.ws.readyState === WebSocket.OPEN) {
      this.ws.close();
    }
    this._connected = false;
  }
}

// ============ PTYManager ============

/**
 * PTY Manager - Manages PTY session lifecycle
 *
 * Uses MissionControl client for IPC-based operations (spawn, kill, list)
 * and creates WebSocket connections for PTY sessions.
 *
 * @example
 * ```typescript
 * import { MissionControl, PTYManager } from '@missiond/core';
 *
 * const client = new MissionControl();
 * const manager = new PTYManager(client);
 *
 * // Spawn a new PTY session
 * const session = await manager.spawn({ slotId: 'worker-1' });
 *
 * // Or attach to an existing session
 * const attached = await manager.attach('worker-1');
 *
 * // List all sessions
 * const sessions = await manager.list();
 *
 * // Kill a session
 * await manager.kill('worker-1');
 * ```
 */
export class PTYManager {
  private client: MissionControl;
  private wsUrl: string;
  private sessions: Map<string, PTYSession> = new Map();

  /**
   * Create a PTYManager
   *
   * @param client - MissionControl client for IPC operations
   */
  constructor(client: MissionControl) {
    this.client = client;
    this.wsUrl = (client as unknown as { getWsUrl: () => string }).getWsUrl?.() || 'ws://localhost:9120';
  }

  /**
   * Spawn a new PTY session
   *
   * @param options - Spawn options
   * @returns The PTY session
   */
  async spawn(options: PTYSpawnOptions): Promise<PTYSession> {
    // First, spawn via IPC
    await this.client.ptySpawn(options.slotId, {
      autoRestart: options.autoRestart,
    });

    // Then connect via WebSocket
    return this.attach(options.slotId);
  }

  /**
   * Attach to an existing PTY session
   *
   * @param slotId - The slot ID to attach to
   * @returns The PTY session
   */
  async attach(slotId: string): Promise<PTYSession> {
    // Check if already attached
    const existing = this.sessions.get(slotId);
    if (existing?.connected) {
      return existing;
    }

    return new Promise((resolve, reject) => {
      const url = `${this.wsUrl}/pty/${slotId}`;
      const ws = new WebSocket(url);

      ws.on('open', () => {
        const session = new PTYSession(ws, slotId);
        this.sessions.set(slotId, session);

        // Remove from map when session closes
        session.on('exit', () => {
          this.sessions.delete(slotId);
        });

        resolve(session);
      });

      ws.on('error', (err) => {
        reject(new Error(`Failed to attach to PTY: ${err.message}`));
      });
    });
  }

  /**
   * List all PTY sessions
   *
   * @returns Array of session info
   */
  async list(): Promise<PTYSessionInfo[]> {
    const result = await this.client.ptyStatus();
    return Array.isArray(result) ? result : [result];
  }

  /**
   * Kill a PTY session
   *
   * @param slotId - The slot ID to kill
   */
  async kill(slotId: string): Promise<void> {
    // Close local session if exists
    const session = this.sessions.get(slotId);
    if (session) {
      session.close();
      this.sessions.delete(slotId);
    }

    // Kill via IPC
    await this.client.ptyKill(slotId);
  }

  /**
   * Get a connected session
   *
   * @param slotId - The slot ID
   * @returns The session or undefined
   */
  getSession(slotId: string): PTYSession | undefined {
    const session = this.sessions.get(slotId);
    return session?.connected ? session : undefined;
  }
}

// ============ Convenience function ============

/**
 * Connect to a PTY session directly via WebSocket
 *
 * @param wsUrl - WebSocket URL (e.g., 'ws://localhost:9120')
 * @param slotId - The slot ID to connect to
 * @returns Promise resolving to the connected PTYSession
 */
export async function connectPTY(wsUrl: string, slotId: string): Promise<PTYSession> {
  return new Promise((resolve, reject) => {
    const url = `${wsUrl}/pty/${slotId}`;
    const ws = new WebSocket(url);

    ws.on('open', () => {
      resolve(new PTYSession(ws, slotId));
    });

    ws.on('error', (err) => {
      reject(new Error(`Failed to connect to PTY: ${err.message}`));
    });

    // Handle connection timeout
    const timeout = setTimeout(() => {
      ws.close();
      reject(new Error('Connection timeout'));
    }, 10000);

    ws.on('open', () => clearTimeout(timeout));
  });
}

export default PTYSession;
