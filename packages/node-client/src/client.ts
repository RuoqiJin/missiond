/**
 * MissionControl Client
 *
 * Main client class for interacting with missiond daemon via IPC.
 * Provides task operations, process control, PTY management, and Claude Code tasks monitoring.
 *
 * Communication:
 * - IPC (Unix socket): Used for tool calls (spawn, kill, send, etc.)
 * - WebSocket: Used for PTY attach and CC tasks events (real-time streaming)
 */

import { EventEmitter } from 'events';
import * as fs from 'fs';
import * as net from 'net';
import * as path from 'path';
import * as os from 'os';
import WebSocket from 'ws';
import type {
  MissionControlOptions,
  MissionControlEvents,
  Task,
  TaskStatus,
  AgentInfo,
  SpawnOptions,
  PTYSpawnOptions,
  PTYSessionInfo,
  PTYOutMessage,
  CCSession,
  CCTask,
  CCTasksOverview,
  TasksEventMessage,
  CCTaskChangeEvent,
  TaskEventPayload,
  SessionEventPayload,
  SessionState,
  Message,
  ToolResult,
} from './types.js';

// ============ Type-safe EventEmitter ============

type EventMap = MissionControlEvents;
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
}

// ============ PTY Session ============

/**
 * PTY Session handle for interacting with an attached PTY
 */
export class PTYSession extends TypedEventEmitter {
  private ws: WebSocket | null = null;
  private _slotId: string;
  private _connected = false;

  constructor(
    slotId: string,
    private wsUrl: string
  ) {
    super();
    this._slotId = slotId;
  }

  get slotId(): string {
    return this._slotId;
  }

  get connected(): boolean {
    return this._connected;
  }

  /**
   * Connect to the PTY WebSocket endpoint
   */
  async connect(): Promise<void> {
    return new Promise((resolve, reject) => {
      const url = `${this.wsUrl}/pty/${this._slotId}`;
      this.ws = new WebSocket(url);

      this.ws.on('open', () => {
        this._connected = true;
        resolve();
      });

      this.ws.on('message', (data: WebSocket.Data) => {
        try {
          const msg = JSON.parse(data.toString()) as PTYOutMessage;
          this.handleMessage(msg);
        } catch {
          // Ignore parse errors
        }
      });

      this.ws.on('close', () => {
        this._connected = false;
        this.emit('disconnected');
      });

      this.ws.on('error', (err) => {
        if (!this._connected) {
          reject(err);
        } else {
          this.emit('error', err);
        }
      });
    });
  }

  private handleMessage(msg: PTYOutMessage): void {
    switch (msg.type) {
      case 'screen':
      case 'data':
        this.emit('pty:data', this._slotId, msg.data);
        break;
      case 'state':
        this.emit('pty:state', this._slotId, msg.state, msg.prevState);
        break;
      case 'exit':
        this.emit('pty:exit', this._slotId, msg.code);
        break;
    }
  }

  /**
   * Write data to the PTY
   */
  write(data: string): void {
    if (!this.ws || this.ws.readyState !== WebSocket.OPEN) {
      throw new Error('PTY session not connected');
    }
    this.ws.send(JSON.stringify({ type: 'input', data }));
  }

  /**
   * Close the PTY session
   */
  close(): void {
    if (this.ws) {
      this.ws.close();
      this.ws = null;
    }
    this._connected = false;
  }
}

// ============ PTY Manager ============

/**
 * PTY Manager for spawning and attaching to PTY sessions
 */
export class PTYManager {
  private toolCaller?: (name: string, args: Record<string, unknown>) => Promise<ToolResult>;

  constructor(
    private wsUrl: string,
    private httpUrl: string,
    toolCaller?: (name: string, args: Record<string, unknown>) => Promise<ToolResult>
  ) {
    this.toolCaller = toolCaller;
  }

  /**
   * Spawn a new PTY session
   */
  async spawn(options: PTYSpawnOptions): Promise<PTYSession> {
    if (this.toolCaller) {
      await this.toolCaller('mission_pty_spawn', {
        slotId: options.slotId,
        autoRestart: options.autoRestart ?? false,
      });
    } else {
      const response = await fetch(`${this.httpUrl}/pty/spawn`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(options),
      });

      if (!response.ok) {
        const error = await response.text();
        throw new Error(`Failed to spawn PTY: ${error}`);
      }
    }

    const session = new PTYSession(options.slotId, this.wsUrl);
    await session.connect();
    return session;
  }

  /**
   * Attach to an existing PTY session
   */
  async attach(slotId: string): Promise<PTYSession> {
    const session = new PTYSession(slotId, this.wsUrl);
    await session.connect();
    return session;
  }

  /**
   * List all PTY sessions
   */
  async list(): Promise<PTYSessionInfo[]> {
    if (this.toolCaller) {
      const result = await this.toolCaller('mission_pty_status', {});
      const text = result?.content?.[0]?.text ?? '';
      if (!text) return [];
      const parsed = JSON.parse(text) as PTYSessionInfo | PTYSessionInfo[];
      return Array.isArray(parsed) ? parsed : [parsed];
    }

    const response = await fetch(`${this.httpUrl}/pty/list`);
    if (!response.ok) {
      throw new Error('Failed to list PTY sessions');
    }
    return (await response.json()) as PTYSessionInfo[];
  }
}

// ============ CC Tasks Manager ============

/**
 * Claude Code Tasks Manager for monitoring task lists across sessions
 */
interface CCSessionSummary {
  sessionId: string;
  project?: string;
  summary?: string;
  modified?: string;
  isActive?: boolean;
}

export class CCTasksManager extends TypedEventEmitter {
  private ws: WebSocket | null = null;
  private _subscribed = false;

  constructor(
    private wsUrl: string,
    private toolCaller: (name: string, args: Record<string, unknown>) => Promise<ToolResult>
  ) {
    super();
  }

  get subscribed(): boolean {
    return this._subscribed;
  }

  /**
   * Get all Claude Code sessions
   */
  async sessions(): Promise<CCSession[]> {
    const result = await this.toolCaller('mission_cc_sessions', { activeOnly: false });
    const parsed = this.parseToolResult<CCSessionSummary[] | CCSession[]>(result) ?? [];
    if (!Array.isArray(parsed) || parsed.length === 0) {
      return [];
    }

    // If daemon already returns full sessions, pass through directly.
    if (Array.isArray((parsed as CCSession[])[0].tasks)) {
      return parsed as CCSession[];
    }

    const summaries = parsed as CCSessionSummary[];
    const sessions = await Promise.all(summaries.map(async (summary) => {
      let tasks: CCTask[] = [];
      try {
        tasks = await this.tasks(summary.sessionId);
      } catch {
        // Best-effort task hydration; keep summary even if one fetch fails.
      }
      const modified = summary.modified || new Date().toISOString();
      const projectPath = summary.project || '';
      return {
        sessionId: summary.sessionId,
        projectPath,
        projectName: projectPath.split('/').pop() || projectPath || 'Unknown',
        summary: summary.summary || '',
        modified,
        created: modified,
        gitBranch: undefined,
        tasks,
        messageCount: 0,
        isActive: !!summary.isActive,
        fullPath: projectPath,
      };
    }));

    return sessions;
  }

  /**
   * Get tasks for a specific session
   */
  async tasks(sessionId: string): Promise<CCTask[]> {
    const result = await this.toolCaller('mission_cc_tasks', { sessionId });
    return this.parseToolResult<CCTask[]>(result) ?? [];
  }

  /**
   * Get tasks overview
   */
  async overview(): Promise<CCTasksOverview> {
    const result = await this.toolCaller('mission_cc_overview', {});
    return this.parseToolResult<CCTasksOverview>(result);
  }

  /**
   * Subscribe to tasks events via WebSocket
   * Returns this manager for chaining event handlers
   */
  subscribe(): CCTasksManager {
    if (this._subscribed) {
      return this;
    }

    const url = `${this.wsUrl}/tasks`;
    this.ws = new WebSocket(url);

    this.ws.on('open', () => {
      this._subscribed = true;
    });

    this.ws.on('message', (data: WebSocket.Data) => {
      try {
        const msg = JSON.parse(data.toString()) as TasksEventMessage;
        this.handleMessage(msg);
      } catch {
        // Ignore parse errors
      }
    });

    this.ws.on('close', () => {
      this._subscribed = false;
    });

    this.ws.on('error', (err) => {
      this.emit('error', err);
    });

    return this;
  }

  private handleMessage(msg: TasksEventMessage): void {
    switch (msg.type) {
      case 'cc_tasks_overview':
        this.emit('tasks:overview', msg.payload);
        break;
      case 'cc_tasks_changed':
        this.emit('tasks:changed', msg.payload);
        break;
      case 'cc_task_started':
        this.emit('tasks:started', msg.payload);
        break;
      case 'cc_task_completed':
        this.emit('tasks:completed', msg.payload);
        break;
      case 'cc_session_active':
        this.emit('session:active', msg.payload);
        break;
      case 'cc_session_inactive':
        this.emit('session:inactive', msg.payload);
        break;
    }
  }

  /**
   * Unsubscribe from tasks events
   */
  unsubscribe(): void {
    if (this.ws) {
      this.ws.close();
      this.ws = null;
    }
    this._subscribed = false;
  }

  private parseToolResult<T>(result: ToolResult): T {
    const text = result?.content?.[0]?.text ?? '';
    if (!text) return null as T;
    try {
      return JSON.parse(text) as T;
    } catch {
      return text as T;
    }
  }
}

// ============ Default Configuration ============

const DEFAULT_WS_PORT = 9120;

function getDefaultSocketPath(): string {
  const missionHome = process.env.MISSIOND_HOME
    || process.env.XJP_MISSION_HOME
    || (fs.existsSync(path.join(os.homedir(), '.missiond')) ? path.join(os.homedir(), '.missiond')
      : fs.existsSync(path.join(os.homedir(), '.xjp-mission')) ? path.join(os.homedir(), '.xjp-mission')
      : path.join(os.homedir(), '.missiond'));
  return path.join(missionHome, 'missiond.sock');
}

function getDefaultWsUrl(): string {
  const port = process.env.MISSION_WS_PORT || DEFAULT_WS_PORT;
  return `ws://localhost:${port}`;
}

/** JSON-RPC request ID counter */
let rpcIdCounter = 0;

// ============ MissionControl ============

/**
 * MissionControl Client
 *
 * Main entry point for interacting with the missiond daemon.
 * Uses IPC (Unix socket) for tool calls and WebSocket for real-time events.
 *
 * Provides:
 * - Task operations (submit, ask, getTaskStatus, cancelTask)
 * - Process control (spawn, kill, agents)
 * - PTY operations via IPC (ptySpawn, ptySend, ptyKill, etc.)
 * - Claude Code tasks monitoring (ccTasks)
 *
 * For PTY session management with WebSocket streaming, use PTYManager from './pty.js'.
 *
 * @example
 * ```typescript
 * import { MissionControl, PTYManager } from '@missiond/core';
 *
 * const mc = new MissionControl();
 *
 * // Check if daemon is running
 * if (await mc.isRunning()) {
 *   // PTY operations via IPC
 *   await mc.ptySpawn('slot-1');
 *   const response = await mc.ptySend('slot-1', 'Hello Claude!');
 *
 *   // Or use PTYManager for WebSocket-based sessions
 *   const ptyManager = new PTYManager(mc);
 *   const session = await ptyManager.attach('slot-1');
 *   session.on('data', (data) => console.log(data));
 * }
 * ```
 */
export class MissionControl extends TypedEventEmitter {
  private socketPath: string;
  private _wsUrl: string;
  private _connected = false;

  /** Claude Code tasks monitoring (IPC for snapshots + WS for subscriptions) */
  readonly ccTasks: CCTasksManager;

  constructor(options: MissionControlOptions = {}) {
    super();

    this.socketPath = options.ipcPath ?? getDefaultSocketPath();
    this._wsUrl = options.wsUrl ?? getDefaultWsUrl();

    this.ccTasks = new CCTasksManager(this._wsUrl, (name, args) => this.callTool(name, args));

    if (options.autoConnect) {
      this.isRunning().then((running) => {
        if (running) {
          this._connected = true;
          this.emit('connected');
        }
      }).catch((err) => this.emit('error', err));
    }
  }

  /**
   * Get the WebSocket URL
   */
  getWsUrl(): string {
    return this._wsUrl;
  }

  /**
   * Get the IPC socket path
   */
  getSocketPath(): string {
    return this.socketPath;
  }

  /**
   * Check if connected to the daemon
   */
  isConnected(): boolean {
    return this._connected;
  }

  /**
   * Check if daemon is running by sending a ping
   */
  async isRunning(): Promise<boolean> {
    try {
      await this.call('ping', {});
      this._connected = true;
      return true;
    } catch {
      this._connected = false;
      return false;
    }
  }

  /**
   * Make an IPC call to the daemon
   */
  async call<T = unknown>(method: string, params: Record<string, unknown>): Promise<T> {
    return new Promise((resolve, reject) => {
      const socket = net.createConnection(this.socketPath);
      let responseData = '';

      const request = {
        jsonrpc: '2.0',
        id: ++rpcIdCounter,
        method,
        params,
      };

      socket.on('connect', () => {
        socket.write(JSON.stringify(request) + '\n');
      });

      socket.on('data', (data) => {
        responseData += data.toString();
        if (responseData.endsWith('\n')) {
          try {
            const response = JSON.parse(responseData.trim());
            if (response.error) {
              reject(new Error(response.error.message || 'RPC error'));
            } else {
              resolve(response.result as T);
            }
          } catch (e) {
            reject(new Error(`Failed to parse response: ${e}`));
          }
          socket.end();
        }
      });

      socket.on('error', (err) => {
        reject(new Error(`IPC connection failed: ${err.message}`));
      });

      socket.on('timeout', () => {
        socket.destroy();
        reject(new Error('IPC connection timeout'));
      });

      socket.setTimeout(30000);
    });
  }

  /**
   * Call a tool via IPC
   */
  async callTool(name: string, args: Record<string, unknown>): Promise<ToolResult> {
    return this.call('tools/call', { name, arguments: args });
  }

  /**
   * Disconnect from the daemon
   */
  async disconnect(): Promise<void> {
    this.ccTasks.unsubscribe();
    this._connected = false;
    this.emit('disconnected');
  }

  // ============ PTY Operations (via IPC) ============

  /**
   * Spawn a PTY session
   */
  async ptySpawn(slotId: string, options?: { autoRestart?: boolean }): Promise<PTYSessionInfo> {
    const result = await this.callTool('mission_pty_spawn', {
      slotId,
      autoRestart: options?.autoRestart ?? false,
    });
    return this.parseToolResult<PTYSessionInfo>(result);
  }

  /**
   * Send message and wait for response
   */
  async ptySend(slotId: string, message: string, timeoutMs = 300000): Promise<string> {
    const result = await this.callTool('mission_pty_send', {
      slotId,
      message,
      timeoutMs,
    });
    return this.extractText(result);
  }

  /**
   * Kill a PTY session
   */
  async ptyKill(slotId: string): Promise<void> {
    await this.callTool('mission_pty_kill', { slotId });
  }

  /**
   * Get screen content
   */
  async ptyScreen(slotId: string, lines?: number): Promise<string> {
    const result = await this.callTool('mission_pty_screen', { slotId, lines });
    return this.extractText(result);
  }

  /**
   * Get chat history
   */
  async ptyHistory(slotId: string): Promise<Message[]> {
    const result = await this.callTool('mission_pty_history', { slotId });
    return this.parseToolResult<Message[]>(result) ?? [];
  }

  /**
   * Get PTY session status
   */
  async ptyStatus(slotId?: string): Promise<PTYSessionInfo | PTYSessionInfo[]> {
    const result = await this.callTool('mission_pty_status', { slotId });
    return this.parseToolResult(result) ?? (slotId ? ({} as PTYSessionInfo) : []);
  }

  /**
   * Send confirmation response
   */
  async ptyConfirm(slotId: string, response: 'yes' | 'no' | number): Promise<void> {
    let value: boolean | number;
    if (response === 'yes') value = true;
    else if (response === 'no') value = false;
    else value = response;
    await this.callTool('mission_pty_confirm', { slotId, response: value });
  }

  /**
   * Send interrupt (Ctrl+C)
   */
  async ptyInterrupt(slotId: string): Promise<void> {
    await this.callTool('mission_pty_interrupt', { slotId });
  }

  // ============ Task Operations ============

  /**
   * Submit a task (async, returns immediately)
   */
  async submit(role: string, prompt: string): Promise<string> {
    const result = await this.callTool('mission_submit', { role, prompt });
    const data = this.parseToolResult<{ taskId: string }>(result);
    return data?.taskId ?? '';
  }

  /**
   * Ask an expert and wait for response
   */
  async ask(role: string, question: string, timeoutMs = 120000): Promise<string> {
    const result = await this.callTool('mission_ask', { role, question, timeoutMs });
    return this.extractText(result);
  }

  /**
   * Get task status
   */
  async getTaskStatus(taskId: string): Promise<Task | null> {
    const result = await this.callTool('mission_status', { taskId });
    return this.parseToolResult<Task>(result);
  }

  /**
   * Cancel a task
   */
  async cancelTask(taskId: string): Promise<boolean> {
    const result = await this.callTool('mission_cancel', { taskId });
    const data = this.parseToolResult<{ cancelled: boolean }>(result);
    return data?.cancelled ?? false;
  }

  // ============ Process Control ============

  /**
   * Spawn an agent
   */
  async spawn(slotId: string, options?: SpawnOptions): Promise<AgentInfo> {
    const result = await this.callTool('mission_spawn', {
      slotId,
      visible: false,
      autoRestart: options?.autoRestart ?? false,
    });
    return this.parseToolResult<AgentInfo>(result);
  }

  /**
   * Kill an agent
   */
  async kill(slotId: string): Promise<void> {
    await this.callTool('mission_kill', { slotId });
  }

  /**
   * Get all agents
   */
  async agents(): Promise<AgentInfo[]> {
    const result = await this.callTool('mission_agents', {});
    return this.parseToolResult<AgentInfo[]>(result) ?? [];
  }

  /**
   * List all slots
   */
  async slots(): Promise<unknown[]> {
    const result = await this.callTool('mission_slots', {});
    return this.parseToolResult<unknown[]>(result) ?? [];
  }

  // ============ Helper Methods ============

  private extractText(result: ToolResult): string {
    if (result?.content?.[0]?.text) {
      return result.content[0].text;
    }
    return '';
  }

  private parseToolResult<T>(result: ToolResult): T {
    const text = this.extractText(result);
    if (!text) return null as T;
    try {
      return JSON.parse(text) as T;
    } catch {
      return text as T;
    }
  }
}

// ============ Exports ============

export default MissionControl;
export type {
  MissionControlOptions,
  MissionControlEvents,
  Task,
  TaskStatus,
  AgentInfo,
  SpawnOptions,
  PTYSpawnOptions,
  PTYSessionInfo,
  PTYOutMessage,
  CCSession,
  CCTask,
  CCTasksOverview,
  CCTaskChangeEvent,
  TaskEventPayload,
  SessionEventPayload,
  SessionState,
};
