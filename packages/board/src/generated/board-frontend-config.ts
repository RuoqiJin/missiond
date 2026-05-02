// GENERATED FILE - do not edit by hand.
// GENERATED_FROM: .missiond/frontend/board-blueprint.lisp
// GENERATED_TO: packages/board/src/generated/board-frontend-config.ts
// To refresh: node scripts/project-frontend-board-config.mjs --write

import type { FlowPhase, GroupBy, TaskCategory, TaskPriority } from '../types';

export type BoardTabId = "jarvis" | "board" | "terminal" | "exec" | "system" | "knowledge" | "logs";
export type EventVersionKey = "slotVersion" | "taskVersion" | "questionVersion" | "decisionVersion" | "memoryVersion" | "deployVersion" | "engineVersion" | "timelineVersion";

export interface BoardTabConfig {
  id: BoardTabId;
  label: string;
  icon: string;
}

export interface EventRouteConfig {
  events: readonly string[];
  bump: readonly EventVersionKey[];
  delayMs?: number;
  healthSnapshot?: boolean;
  deployCategoryBump?: boolean;
}

export interface EventCustomEventConfig {
  event: string;
  name: string;
  detail: readonly string[];
}

export interface EventPrefixRouteConfig {
  prefix: string;
  bump: readonly EventVersionKey[];
  delayMs?: number;
}

export const DEFAULT_TAB: BoardTabId = "jarvis";
export const BOARD_TABS = [
  { id: "jarvis", label: "Jarvis", icon: "Sparkles" },
  { id: "board", label: "Board", icon: "ClipboardList" },
  { id: "terminal", label: "Terminal", icon: "MonitorUp" },
  { id: "exec", label: "Exec", icon: "Crosshair" },
  { id: "system", label: "System", icon: "Gauge" },
  { id: "knowledge", label: "Knowledge", icon: "Brain" },
  { id: "logs", label: "Logs", icon: "MessageSquareText" },
] as const satisfies readonly BoardTabConfig[];
export const TAB_MIGRATION = {
  autopilot: "exec",
  decisions: "exec",
  memory: "system",
  engine: "system",
  conversations: "logs",
  timeline: "logs",
  architecture: "knowledge",
  deploy: "board",
  research: "board",
} as const satisfies Record<string, BoardTabId>;

export const CATEGORY_CONFIG = {
  deploy: { label: "部署", className: "bg-orange-500/10 text-orange-400 border-orange-500/20" },
  dev: { label: "开发", className: "bg-blue-500/10 text-blue-400 border-blue-500/20" },
  infra: { label: "基建", className: "bg-purple-500/10 text-purple-400 border-purple-500/20" },
  test: { label: "测试", className: "bg-green-500/10 text-green-400 border-green-500/20" },
  research: { label: "研究", className: "bg-cyan-500/10 text-cyan-400 border-cyan-500/20" },
  diagnosis: { label: "诊断", className: "bg-rose-500/10 text-rose-400 border-rose-500/20" },
  investigation: { label: "调查", className: "bg-amber-500/10 text-amber-400 border-amber-500/20" },
  other: { label: "其他", className: "bg-neutral-500/10 text-neutral-400 border-neutral-500/20" },
} as const satisfies Record<TaskCategory, { label: string; className: string }>;
export const PRIORITY_CONFIG = {
  high: { label: "高", dotColor: "bg-red-500" },
  medium: { label: "中", dotColor: "bg-yellow-500" },
  low: { label: "低", dotColor: "bg-blue-500" },
} as const satisfies Record<TaskPriority, { label: string; dotColor: string }>;
export const GROUP_OPTIONS = [
  { value: "none" as GroupBy, label: "不分组" },
  { value: "category" as GroupBy, label: "按分类" },
  { value: "priority" as GroupBy, label: "按优先级" },
  { value: "project" as GroupBy, label: "按项目" },
] as const satisfies readonly { value: GroupBy; label: string }[];
export const SERVER_OPTIONS = [
  "私有云",
  "ECS",
  "GCP",
  "Win Agent",
] as const;

export const FLOW_TEMPLATE_OPTIONS = [
  { value: "", label: "无（普通任务）" },
  { value: "engineering", label: "Engineering Flow" },
] as const;
export const FLOW_PHASES = [
  "investigate" as FlowPhase,
  "consult_gemini_1" as FlowPhase,
  "plan" as FlowPhase,
  "consult_gemini_2" as FlowPhase,
  "execute" as FlowPhase,
  "finalize" as FlowPhase,
  "done" as FlowPhase,
] as const satisfies readonly FlowPhase[];
export const FLOW_PHASE_LABELS = {
  investigate: "调查",
  consult_gemini_1: "咨询1",
  plan: "方案",
  consult_gemini_2: "咨询2",
  execute: "执行",
  finalize: "收尾",
  done: "完成",
} as const satisfies Record<FlowPhase, string>;

export const RESYNC_VERSION_KEYS = [
  "slotVersion" as EventVersionKey,
  "taskVersion" as EventVersionKey,
  "questionVersion" as EventVersionKey,
  "decisionVersion" as EventVersionKey,
  "memoryVersion" as EventVersionKey,
  "deployVersion" as EventVersionKey,
  "engineVersion" as EventVersionKey,
  "timelineVersion" as EventVersionKey,
] as const satisfies readonly EventVersionKey[];
export const EVENT_ROUTE_TABLE = [
  { events: [
  "health_snapshot",
], bump: [
  "engineVersion" as EventVersionKey,
], healthSnapshot: true },
  { events: [
  "slot_state_changed",
  "slot_task_dispatched",
], bump: [
  "slotVersion" as EventVersionKey,
  "timelineVersion" as EventVersionKey,
] },
  { events: [
  "task_lifecycle",
], bump: [
  "taskVersion" as EventVersionKey,
  "timelineVersion" as EventVersionKey,
] },
  { events: [
  "question_created",
  "question_resolved",
], bump: [
  "questionVersion" as EventVersionKey,
  "timelineVersion" as EventVersionKey,
] },
  { events: [
  "decision_made",
], bump: [
  "decisionVersion" as EventVersionKey,
  "timelineVersion" as EventVersionKey,
] },
  { events: [
  "memory_phase_changed",
], bump: [
  "memoryVersion" as EventVersionKey,
  "timelineVersion" as EventVersionKey,
] },
  { events: [
  "cli_request_started",
  "cli_request_completed",
  "cli_tool_activity",
  "gemini_request_started",
  "gemini_request_completed",
  "gemini_tool_activity",
  "codex_request_started",
  "codex_request_completed",
], bump: [
  "engineVersion" as EventVersionKey,
  "timelineVersion" as EventVersionKey,
] },
  { events: [
  "board_task_updated",
], bump: [
  "taskVersion" as EventVersionKey,
  "timelineVersion" as EventVersionKey,
], deployCategoryBump: true },
  { events: [
  "insight_generated",
  "briefing_batch_started",
  "git_commit",
  "board_task_created",
  "board_task_status_changed",
  "board_task_note_added",
  "board_task_claimed",
  "board_task_deleted",
  "translation_started",
  "translation_completed",
  "translation_failed",
], bump: [
  "timelineVersion" as EventVersionKey,
] },
  { events: [
  "user_message",
  "assistant_message",
  "thinking_message",
  "system_message",
], bump: [
  "timelineVersion" as EventVersionKey,
], delayMs: 500 },
] as const satisfies readonly EventRouteConfig[];
export const EVENT_CUSTOM_EVENTS = [
  { event: "briefing_summary_generated", name: "timeline-summary-update", detail: [
  "target_seq",
  "summary",
] },
  { event: "jarvis_task_completed", name: "jarvis-task-completed", detail: [
  "conversation_id",
  "task_id",
] },
] as const satisfies readonly EventCustomEventConfig[];
export const EVENT_PREFIX_ROUTES = [
  { prefix: "narration_", bump: [
  "timelineVersion" as EventVersionKey,
], delayMs: 500 },
] as const satisfies readonly EventPrefixRouteConfig[];
