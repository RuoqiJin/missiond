/**
 * @missiond/core - Node.js client for missiond
 *
 * Claude Code multi-instance orchestration system.
 *
 * @example
 * ```typescript
 * import { MissionControl, PTYManager, PTYSession, connectPTY } from '@missiond/core';
 *
 * // Create a MissionControl client
 * const mc = new MissionControl();
 *
 * // Check daemon status
 * if (await mc.isRunning()) {
 *   // Use PTYManager for WebSocket-based sessions
 *   const ptyManager = new PTYManager(mc);
 *   const session = await ptyManager.spawn({ slotId: 'worker-1' });
 *
 *   session.on('data', (data) => process.stdout.write(data));
 *   session.on('state', (state, prev) => console.log(`State: ${prev} -> ${state}`));
 *
 *   // Send a message and wait for response
 *   const response = await session.send('Hello, Claude!');
 *   console.log('Response:', response);
 *
 *   // Or use IPC directly
 *   const text = await mc.ptySend('worker-1', 'Another message');
 * }
 *
 * // Direct WebSocket connection
 * const session = await connectPTY('ws://localhost:9120', 'worker-1');
 * ```
 *
 * @packageDocumentation
 */

// Main client
export { MissionControl, CCTasksManager } from './client.js';

// PTY session and manager
export { PTYSession, PTYManager, connectPTY } from './pty.js';

// Re-export types from client.js
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
} from './client.js';

// Re-export all types from types.ts for convenience
export type {
  PTYInMessage,
  TasksEventMessage,
  CCTaskStatus,
  TasksByStatus,
  TaskStatusChange,
  SlotConfig,
  Slot,
  TaskStats,
  AgentStats,
  SlotStats,
  InboxStats,
  MissionStats,
  // PTY session events
  PTYSessionEvents,
  // Text output
  TextOutputEvent,
  TextOutputStreamEvent,
  TextOutputCompleteEvent,
  // Confirmation
  ConfirmInfo,
  ConfirmType,
  ConfirmOption,
  ConfirmKey,
  ConfirmResponse,
  ConfirmAction,
  ToolInfo,
  // Status and tools
  ClaudeCodeStatus,
  StatusPhase,
  ClaudeCodeToolOutput,
  ToolExecutionStatus,
  // Session events
  SessionEvent,
  ScreenTextSource,
  ScreenTextEvent,
  // Message types
  Message,
  MessageRole,
  InboxMessage,
  // Tool types
  ToolResult,
  ToolContent,
  ToolContentText,
  ToolDefinition,
  // Permission
  PermissionDecision,
  PermissionRule,
} from './types.js';

// Utility functions
export { isProcessingState } from './types.js';

// Daemon management
export {
  DaemonManager,
  DaemonOptions,
  getDefaultManager,
  isDaemonRunning,
  ensureDaemonRunning,
  stopDaemon,
} from './daemon.js';

// Binary path resolution
export {
  getBinaryPath,
  getExpectedPackageName,
  getAllExpectedPackageNames,
  isBinaryAvailable,
  getBinaryDiagnostics,
} from './binary.js';

// Default export
export { default } from './client.js';
