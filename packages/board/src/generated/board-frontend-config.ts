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

export interface TimelineEventVisualConfig {
  dot: string;
  glow: string;
  bg: string;
  text: string;
  label: string;
  icon: string;
}

export interface TimelineSlotColorConfig {
  badge: string;
  border: string;
  line: string;
}

export interface TimelineSwimlaneConfig {
  id: string;
  label: string;
  accent: { dot: string; css: string; bg: string };
  types: readonly string[];
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

export const TIMELINE_EVENT_VISUALS = {
  user_message: { dot: "bg-blue-400", glow: "shadow-blue-400/50", bg: "bg-blue-400/10", text: "text-blue-400", label: "User", icon: "MessageSquare" },
  assistant_message: { dot: "bg-teal-400", glow: "shadow-teal-400/50", bg: "bg-teal-400/10", text: "text-teal-400", label: "Assistant", icon: "Brain" },
  thinking_message: { dot: "bg-violet-400", glow: "shadow-violet-400/50", bg: "bg-violet-400/10", text: "text-violet-400", label: "Thinking", icon: "Brain" },
  cli_request_started: { dot: "bg-fuchsia-400", glow: "shadow-fuchsia-400/50", bg: "bg-fuchsia-400/10", text: "text-fuchsia-400", label: "CLI ▸", icon: "Cpu" },
  cli_request_completed: { dot: "bg-fuchsia-500", glow: "shadow-fuchsia-500/50", bg: "bg-fuchsia-500/10", text: "text-fuchsia-400", label: "CLI ◂", icon: "Cpu" },
  cli_tool_activity: { dot: "bg-fuchsia-300", glow: "shadow-fuchsia-300/50", bg: "bg-fuchsia-300/10", text: "text-fuchsia-300", label: "CLI Tool", icon: "Wrench" },
  gemini_request_started: { dot: "bg-fuchsia-400", glow: "shadow-fuchsia-400/50", bg: "bg-fuchsia-400/10", text: "text-fuchsia-400", label: "Gemini ▸", icon: "Cpu" },
  gemini_request_completed: { dot: "bg-fuchsia-500", glow: "shadow-fuchsia-500/50", bg: "bg-fuchsia-500/10", text: "text-fuchsia-400", label: "Gemini ◂", icon: "Cpu" },
  codex_request_started: { dot: "bg-lime-400", glow: "shadow-lime-400/50", bg: "bg-lime-400/10", text: "text-lime-400", label: "GPT ▸", icon: "Zap" },
  codex_request_completed: { dot: "bg-lime-500", glow: "shadow-lime-500/50", bg: "bg-lime-500/10", text: "text-lime-400", label: "GPT ◂", icon: "Zap" },
  git_commit: { dot: "bg-yellow-400", glow: "shadow-yellow-400/50", bg: "bg-yellow-400/10", text: "text-yellow-400", label: "Commit", icon: "GitCommit" },
  task_lifecycle: { dot: "bg-sky-400", glow: "shadow-sky-400/50", bg: "bg-sky-400/10", text: "text-sky-400", label: "Task", icon: "Activity" },
  question_created: { dot: "bg-amber-400", glow: "shadow-amber-400/50", bg: "bg-amber-400/10", text: "text-amber-400", label: "Question", icon: "MessageSquare" },
  question_resolved: { dot: "bg-amber-300", glow: "shadow-amber-300/50", bg: "bg-amber-300/10", text: "text-amber-300", label: "Resolved", icon: "MessageSquare" },
  decision_made: { dot: "bg-orange-400", glow: "shadow-orange-400/50", bg: "bg-orange-400/10", text: "text-orange-400", label: "Decision", icon: "GitBranch" },
  insight_generated: { dot: "bg-emerald-400", glow: "shadow-emerald-400/50", bg: "bg-emerald-400/10", text: "text-emerald-400", label: "Insight", icon: "Sparkles" },
  board_task_created: { dot: "bg-indigo-400", glow: "shadow-indigo-400/50", bg: "bg-indigo-400/10", text: "text-indigo-400", label: "Created", icon: "Activity" },
  board_task_status_changed: { dot: "bg-indigo-300", glow: "shadow-indigo-300/50", bg: "bg-indigo-300/10", text: "text-indigo-300", label: "Status", icon: "Activity" },
  board_task_note_added: { dot: "bg-indigo-200", glow: "shadow-indigo-200/50", bg: "bg-indigo-200/10", text: "text-indigo-200", label: "Note", icon: "MessageSquare" },
  board_task_claimed: { dot: "bg-indigo-500", glow: "shadow-indigo-500/50", bg: "bg-indigo-500/10", text: "text-indigo-400", label: "Claimed", icon: "Wrench" },
  board_task_deleted: { dot: "bg-red-400", glow: "shadow-red-400/50", bg: "bg-red-400/10", text: "text-red-400", label: "Deleted", icon: "AlertTriangle" },
  board_task_updated: { dot: "bg-indigo-300", glow: "shadow-indigo-300/50", bg: "bg-indigo-300/10", text: "text-indigo-300", label: "Board", icon: "Activity" },
  slot_state_changed: { dot: "bg-slate-400", glow: "shadow-slate-400/50", bg: "bg-slate-400/10", text: "text-slate-400", label: "Slot", icon: "Settings2" },
  memory_phase_changed: { dot: "bg-cyan-400", glow: "shadow-cyan-400/50", bg: "bg-cyan-400/10", text: "text-cyan-400", label: "Memory", icon: "Brain" },
  briefing_batch_started: { dot: "bg-rose-300", glow: "shadow-rose-300/50", bg: "bg-rose-300/10", text: "text-rose-300", label: "Briefing", icon: "Sparkles" },
  briefing_summary_generated: { dot: "bg-rose-400", glow: "shadow-rose-400/50", bg: "bg-rose-400/10", text: "text-rose-400", label: "Summary", icon: "Sparkles" },
  system_message: { dot: "bg-slate-400", glow: "shadow-slate-400/50", bg: "bg-slate-400/10", text: "text-slate-400", label: "Daemon", icon: "Terminal" },
  slot_task_dispatched: { dot: "bg-amber-400", glow: "shadow-amber-400/50", bg: "bg-amber-400/10", text: "text-amber-400", label: "Dispatch", icon: "ArrowRight" },
  translation_started: { dot: "bg-indigo-400", glow: "shadow-indigo-400/50", bg: "bg-indigo-400/10", text: "text-indigo-400", label: "Translating", icon: "Languages" },
  translation_completed: { dot: "bg-indigo-500", glow: "shadow-indigo-500/50", bg: "bg-indigo-500/10", text: "text-indigo-400", label: "Translated", icon: "CheckCheck" },
  translation_failed: { dot: "bg-red-400", glow: "shadow-red-400/50", bg: "bg-red-400/10", text: "text-red-400", label: "Trans Err", icon: "AlertTriangle" },
  narration_session_started: { dot: "bg-violet-300", glow: "shadow-violet-300/50", bg: "bg-violet-300/10", text: "text-violet-300", label: "Narrating", icon: "Sparkles" },
  narration_batch_completed: { dot: "bg-violet-400", glow: "shadow-violet-400/50", bg: "bg-violet-400/10", text: "text-violet-400", label: "Batch Done", icon: "CheckCheck" },
  narration_session_completed: { dot: "bg-violet-500", glow: "shadow-violet-500/50", bg: "bg-violet-500/10", text: "text-violet-400", label: "Narrated", icon: "CheckCheck" },
  narration_failed: { dot: "bg-red-400", glow: "shadow-red-400/50", bg: "bg-red-400/10", text: "text-red-400", label: "Narr Err", icon: "AlertTriangle" },
} as const satisfies Record<string, TimelineEventVisualConfig>;
export const TIMELINE_SLOT_COLORS = {
  "slot-coder-1": { badge: "bg-blue-500/20 text-blue-300", border: "border-l-blue-400", line: "rgba(96,165,250,0.25)" },
  "slot-coder-bypass": { badge: "bg-indigo-500/20 text-indigo-300", border: "border-l-indigo-400", line: "rgba(129,140,248,0.25)" },
  "slot-deploy-1": { badge: "bg-orange-500/20 text-orange-300", border: "border-l-orange-400", line: "rgba(251,146,60,0.25)" },
  "slot-memory": { badge: "bg-cyan-500/20 text-cyan-300", border: "border-l-cyan-400", line: "rgba(34,211,238,0.25)" },
  "slot-memory-slow": { badge: "bg-teal-500/20 text-teal-300", border: "border-l-teal-400", line: "rgba(45,212,191,0.25)" },
} as const satisfies Record<string, TimelineSlotColorConfig>;
export const TIMELINE_SLOT_FALLBACK_LINES = [
  "rgba(168,85,247,0.25)",
  "rgba(244,114,182,0.25)",
  "rgba(74,222,128,0.25)",
  "rgba(251,191,36,0.25)",
] as const;
export const TIMELINE_SWIMLANES = [
  { id: "chat", label: "Chat", accent: { dot: "bg-indigo-400", css: "#818cf8", bg: "bg-indigo-500/[0.03]" }, types: [
  "user_message",
  "assistant_message",
  "thinking_message",
] },
  { id: "slot", label: "Slot", accent: { dot: "bg-teal-400", css: "#2dd4bf", bg: "bg-teal-500/[0.03]" }, types: [
  "slot_task_dispatched",
  "system_message",
] },
  { id: "ai", label: "AI / LLM", accent: { dot: "bg-emerald-400", css: "#34d399", bg: "bg-emerald-500/[0.03]" }, types: [
  "cli_request_started",
  "cli_request_completed",
  "cli_tool_activity",
  "gemini_request_started",
  "gemini_request_completed",
  "decision_made",
  "insight_generated",
] },
  { id: "gpt", label: "GPT", accent: { dot: "bg-amber-400", css: "#fbbf24", bg: "bg-amber-500/[0.03]" }, types: [
  "codex_request_started",
  "codex_request_completed",
] },
  { id: "code", label: "Code", accent: { dot: "bg-blue-400", css: "#60a5fa", bg: "bg-blue-500/[0.03]" }, types: [
  "git_commit",
] },
  { id: "flow", label: "Flow", accent: { dot: "bg-orange-400", css: "#fb923c", bg: "bg-orange-500/[0.03]" }, types: [
  "task_lifecycle",
  "question_created",
  "question_resolved",
] },
  { id: "board", label: "Board", accent: { dot: "bg-pink-400", css: "#f472b6", bg: "bg-pink-500/[0.03]" }, types: [
  "board_task_created",
  "board_task_status_changed",
  "board_task_note_added",
  "board_task_claimed",
  "board_task_deleted",
  "board_task_updated",
] },
  { id: "sys", label: "System", accent: { dot: "bg-slate-400", css: "#94a3b8", bg: "bg-slate-500/[0.03]" }, types: [
  "slot_state_changed",
  "memory_phase_changed",
  "briefing_batch_started",
  "briefing_summary_generated",
  "narration_session_started",
  "narration_batch_completed",
  "narration_session_completed",
  "narration_failed",
] },
] as const satisfies readonly TimelineSwimlaneConfig[];
export const TIMELINE_SESSION_COLORS = [
  { dot: "bg-cyan-400", line: "rgba(34,211,238,0.25)", ring: "ring-cyan-400/40" },
  { dot: "bg-green-400", line: "rgba(74,222,128,0.25)", ring: "ring-green-400/40" },
  { dot: "bg-amber-400", line: "rgba(251,191,36,0.25)", ring: "ring-amber-400/40" },
  { dot: "bg-pink-400", line: "rgba(244,114,182,0.25)", ring: "ring-pink-400/40" },
  { dot: "bg-violet-400", line: "rgba(167,139,250,0.25)", ring: "ring-violet-400/40" },
  { dot: "bg-orange-400", line: "rgba(251,146,60,0.25)", ring: "ring-orange-400/40" },
  { dot: "bg-teal-300", line: "rgba(94,234,212,0.25)", ring: "ring-teal-300/40" },
  { dot: "bg-rose-400", line: "rgba(251,113,133,0.25)", ring: "ring-rose-400/40" },
] as const;
export const TIMELINE_WINDOW_OPTIONS = [
  { label: "5m", value: "5min" },
  { label: "10m", value: "10min" },
  { label: "30m", value: "30min" },
  { label: "1h", value: "1h" },
  { label: "6h", value: "6h" },
  { label: "24h", value: "24h" },
] as const;
