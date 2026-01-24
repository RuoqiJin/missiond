/**
 * Type definitions for missiond Node.js client
 *
 * Mirrors Rust implementations:
 * - missiond-core/src/pty/session.rs (SessionState, SessionEvent, ConfirmInfo)
 * - missiond-mcp/src/tools.rs (ToolDefinition, ToolResult)
 * - missiond-core/src/semantic/types.rs (ClaudeCodeStatus, ClaudeCodeToolOutput, ClaudeCodeTitle)
 * - missiond-core/src/cc_tasks/types.rs (CCTask, CCTaskStatus)
 * - missiond-core/src/ws/server.rs (PtyOutMessage, PtyInMessage, TasksEventMessage)
 */

// ============ Task Types ============

export type TaskStatus = 'queued' | 'running' | 'done' | 'failed' | 'cancelled';

export interface Task {
  id: string;
  role: string;
  prompt: string;
  status: TaskStatus;
  slotId?: string;
  sessionId?: string;
  result?: string;
  error?: string;
  createdAt: number;
  startedAt?: number;
  finishedAt?: number;
}

// ============ Agent Types ============

export type SessionState =
  | 'starting'
  | 'idle'
  | 'thinking'
  | 'responding'
  | 'tool_running'
  | 'confirming'
  | 'exited'
  | 'error';

export interface AgentInfo {
  slotId: string;
  role: string;
  pid?: number;
  state: SessionState;
  startedAt?: number;
  currentTaskId?: string;
  logFile: string;
}

export interface SpawnOptions {
  autoRestart?: boolean;
}

// ============ PTY Types ============

export interface PTYSpawnOptions {
  slotId: string;
  role?: string;
  cwd?: string;
  cols?: number;
  rows?: number;
  autoRestart?: boolean;
}

export interface PTYSessionInfo {
  slotId: string;
  role: string;
  pid?: number;
  state: SessionState;
  startedAt?: number;
}

/** Messages from PTY to client */
export type PTYOutMessage =
  | { type: 'screen'; data: string }
  | { type: 'data'; data: string }
  | { type: 'state'; state: SessionState; prevState: SessionState }
  | { type: 'exit'; code: number };

/** Messages from client to PTY */
export type PTYInMessage = { type: 'input'; data: string };

// ============ Claude Code Tasks Types ============

export type CCTaskStatus = 'pending' | 'in_progress' | 'completed';

export interface CCTask {
  content: string;
  status: CCTaskStatus;
  activeForm?: string;
}

export interface CCSession {
  sessionId: string;
  projectPath: string;
  projectName: string;
  summary: string;
  modified: string; // ISO date
  created: string; // ISO date
  gitBranch?: string;
  tasks: CCTask[];
  messageCount: number;
  isActive: boolean;
  fullPath: string;
}

export interface TasksByStatus {
  pending: number;
  in_progress: number;
  completed: number;
}

export interface CCTasksOverview {
  totalSessions: number;
  activeSessions: number;
  tasksByStatus: TasksByStatus;
  sessionsWithTasks: number;
  recentChanges: CCTaskChangeEvent[];
}

export interface TaskStatusChange {
  task: CCTask;
  previousStatus: CCTaskStatus;
}

export interface CCTaskChangeEvent {
  sessionId: string;
  projectPath: string;
  projectName: string;
  previousTasks: CCTask[];
  currentTasks: CCTask[];
  added: CCTask[];
  removed: CCTask[];
  statusChanged: TaskStatusChange[];
  timestamp: string; // ISO date
}

/** WebSocket event messages for /tasks endpoint */
export type TasksEventMessage =
  | { type: 'cc_tasks_overview'; payload: CCTasksOverview }
  | { type: 'cc_tasks_changed'; payload: CCTaskChangeEvent }
  | { type: 'cc_task_started'; payload: TaskEventPayload }
  | { type: 'cc_task_completed'; payload: TaskEventPayload }
  | { type: 'cc_session_active'; payload: SessionEventPayload }
  | { type: 'cc_session_inactive'; payload: SessionEventPayload };

export interface TaskEventPayload {
  sessionId: string;
  projectName: string;
  task: CCTask;
}

export interface SessionEventPayload {
  sessionId: string;
  projectName: string;
  summary?: string;
}

// ============ Slot Types ============

export interface SlotConfig {
  id: string;
  role: string;
  description: string;
  cwd?: string;
  mcpConfig?: string;
  autoStart?: boolean;
}

export interface Slot extends SlotConfig {
  sessionId?: string;
}

// ============ Stats Types ============

export interface TaskStats {
  queued: number;
  running: number;
  done: number;
  failed: number;
}

export interface AgentStats {
  total: number;
  stopped: number;
  idle: number;
  busy: number;
}

export interface SlotStats {
  total: number;
  byRole: Record<string, number>;
}

export interface InboxStats {
  unread: number;
}

export interface MissionStats {
  tasks: TaskStats;
  agents: AgentStats;
  slots: SlotStats;
  inbox: InboxStats;
}

// ============ Client Options ============

export interface MissionControlOptions {
  /** WebSocket URL for PTY and Tasks subscriptions (default: ws://localhost:9527) */
  wsUrl?: string;
  /** IPC socket path (default: /tmp/missiond.sock) */
  ipcPath?: string;
  /** HTTP API URL (default: http://localhost:9528) */
  httpUrl?: string;
  /** Auto-connect on instantiation */
  autoConnect?: boolean;
  /** Reconnect on disconnect */
  reconnect?: boolean;
  /** Reconnect interval in ms */
  reconnectInterval?: number;
}

// ============ Event Types ============

export interface MissionControlEvents {
  connected: [];
  disconnected: [];
  error: [error: Error];
  // PTY events (when attached)
  'pty:data': [slotId: string, data: string];
  'pty:state': [slotId: string, state: SessionState, prevState: SessionState];
  'pty:exit': [slotId: string, code: number];
  // CC Tasks events
  'tasks:overview': [overview: CCTasksOverview];
  'tasks:changed': [event: CCTaskChangeEvent];
  'tasks:started': [payload: TaskEventPayload];
  'tasks:completed': [payload: TaskEventPayload];
  'session:active': [payload: SessionEventPayload];
  'session:inactive': [payload: SessionEventPayload];
}

// ============ Semantic Types (Claude Code Status/Title/ToolOutput) ============

/** Phase of Claude Code status */
export type StatusPhase = 'thinking' | 'tool_running' | 'unknown';

/** Claude Code status bar information */
export interface ClaudeCodeStatus {
  /** Spinner character */
  spinner: string;
  /** Status text (e.g., "Precipitating...") */
  status_text: string;
  /** Current phase */
  phase: StatusPhase;
  /** Whether the operation can be interrupted */
  interruptible: boolean;
}

/** Status of a tool execution */
export type ToolExecutionStatus = 'running' | 'completed';

/** Parsed tool output from Claude Code */
export interface ClaudeCodeToolOutput {
  /** Tool name (e.g., "Bash", "Read", "Edit") */
  tool_name: string;
  /** Tool parameters */
  params: Record<string, unknown>;
  /** Tool output content */
  output?: string;
  /** Duration in milliseconds (if completed) */
  duration_ms?: number;
  /** Tool execution status */
  status: ToolExecutionStatus;
}

/** Information parsed from Claude Code terminal title */
export interface ClaudeCodeTitle {
  /** Current task name (if any) */
  task_name?: string;
  /** Spinner character state */
  spinner_state: string;
  /** Whether Claude is currently processing */
  is_processing: boolean;
}

// ============ Confirmation Types ============

/** Type of confirmation dialog */
export type ConfirmType = 'options' | 'yes_no';

/** Key type for confirm options */
export type ConfirmKey = number | string;

/** A single confirm option */
export interface ConfirmOption {
  /** Option key (number or character) */
  key: ConfirmKey;
  /** Option label */
  label: string;
  /** Whether this is the default option */
  is_default?: boolean;
}

/** Tool information from confirmation dialog */
export interface ToolInfo {
  /** Tool name */
  name: string;
  /** MCP server name */
  mcp_server?: string;
  /** Tool parameters */
  params: Record<string, unknown>;
}

/** Confirmation dialog information */
export interface ConfirmInfo {
  /** Confirmation type */
  type: string;
  /** Tool information (if confirming tool usage) */
  tool?: ToolInfo;
  /** Available options */
  options: string[];
  /** Currently selected option index */
  selected: number;
}

/** Confirmation action */
export type ConfirmAction = 'confirm' | 'deny' | 'select' | 'input';

/** Response to a confirmation dialog */
export interface ConfirmResponse {
  /** Action to take */
  action: ConfirmAction;
  /** Option number (for Select action) */
  option?: number;
  /** Custom value (for Input action) */
  value?: string;
}

/** Permission decision for tool execution */
export type PermissionDecision = 'allow' | 'deny' | 'confirm';

// ============ Session Events (Full SessionEvent union) ============

/** Source of screen text */
export type ScreenTextSource = 'assistant' | 'user' | 'tool' | 'ui' | 'unknown';

/** Text output event - streaming */
export interface TextOutputStreamEvent {
  type: 'stream';
  turn_id: number;
  seq: number;
  content: string;
  timestamp: number;
}

/** Text output event - complete */
export interface TextOutputCompleteEvent {
  type: 'complete';
  turn_id: number;
  content: string;
  timestamp: number;
}

/** Text output event (stream or complete) */
export type TextOutputEvent = TextOutputStreamEvent | TextOutputCompleteEvent;

/** Screen text event for non-assistant content */
export interface ScreenTextEvent {
  source: ScreenTextSource;
  kind: string;
  y: number;
  content: string;
  timestamp: number;
  turn_id?: number;
}

/** Session event - raw data from PTY */
export interface SessionEventData {
  type: 'data';
  data: Uint8Array;
}

/** Session event - state changed */
export interface SessionEventStateChange {
  type: 'state_change';
  new_state: SessionState;
  prev_state: SessionState;
}

/** Session event - text output */
export interface SessionEventTextOutput {
  type: 'text_output';
  event: TextOutputEvent;
}

/** Session event - screen text */
export interface SessionEventScreenText {
  type: 'screen_text';
  event: ScreenTextEvent;
}

/** Session event - confirmation required */
export interface SessionEventConfirmRequired {
  type: 'confirm_required';
  prompt: string;
  info?: ConfirmInfo;
}

/** Session event - status update */
export interface SessionEventStatusUpdate {
  type: 'status_update';
  status: ClaudeCodeStatus;
}

/** Session event - tool output */
export interface SessionEventToolOutput {
  type: 'tool_output';
  output: ClaudeCodeToolOutput;
}

/** Session event - title change */
export interface SessionEventTitleChange {
  type: 'title_change';
  title: ClaudeCodeTitle;
}

/** Session event - exit */
export interface SessionEventExit {
  type: 'exit';
  code: number;
}

/** All session event types */
export type SessionEvent =
  | SessionEventData
  | SessionEventStateChange
  | SessionEventTextOutput
  | SessionEventScreenText
  | SessionEventConfirmRequired
  | SessionEventStatusUpdate
  | SessionEventToolOutput
  | SessionEventTitleChange
  | SessionEventExit;

// ============ MCP Tool Types ============

/** Tool definition following MCP schema */
export interface ToolDefinition {
  /** Tool name (e.g., "mission_submit") */
  name: string;
  /** Human-readable description */
  description: string;
  /** JSON Schema for input parameters */
  inputSchema: Record<string, unknown>;
}

/** Tool content type - text */
export interface ToolContentText {
  type: 'text';
  text: string;
}

export type ToolContent = ToolContentText;

/** Tool call result */
export interface ToolResult {
  content: ToolContent[];
  isError?: boolean;
}

/** Permission rule for role/slot */
export interface PermissionRule {
  auto_allow?: string[];
  require_confirm?: string[];
  deny?: string[];
}

// ============ PTY Session Status ============

/** PTY session status (detailed) */
export interface PTYSessionStatus {
  /** Session ID */
  id: string;
  /** Slot ID */
  slot_id: string;
  /** Current state */
  state: SessionState;
  /** Working directory */
  cwd: string;
  /** Process ID */
  pid?: number;
  /** Terminal columns */
  cols: number;
  /** Terminal rows */
  rows: number;
  /** Whether session is running */
  running: boolean;
  /** Pending confirmation info */
  pending_confirm?: ConfirmInfo;
  /** Terminal title */
  terminal_title?: string;
}

// ============ Chat Message Types ============

/** Chat message role */
export type MessageRole = 'user' | 'assistant';

/** Chat message */
export interface Message {
  role: MessageRole;
  content: string;
  timestamp: number;
}

// ============ Inbox Types ============

/** Inbox message */
export interface InboxMessage {
  id: string;
  from_slot: string;
  from_role: string;
  content: string;
  timestamp: number;
  read: boolean;
}

// ============ PTY Session Events ============

/** Events emitted by PTYSession */
export interface PTYSessionEvents {
  /** Raw data from PTY */
  data: [data: string];
  /** Full screen content received */
  screen: [screen: string];
  /** State changed */
  state: [state: SessionState, prevState: SessionState];
  /** Text output (streaming or complete) */
  text: [event: TextOutputEvent];
  /** Confirmation required */
  confirm: [info: ConfirmInfo];
  /** Status bar update */
  status: [status: ClaudeCodeStatus];
  /** Tool output parsed */
  tool: [output: ClaudeCodeToolOutput];
  /** Session exited */
  exit: [code: number];
  /** Error occurred */
  error: [error: Error];
}

// ============ Helper Functions ============

/** Check if state is a processing state (Claude is active) */
export function isProcessingState(state: SessionState): boolean {
  return state === 'thinking' || state === 'tool_running' || state === 'responding';
}
