import React from 'react';
import {
  Zap, Brain, Wrench, ArrowRight,
  MessageSquare, GitBranch, GitCommit, Activity, Cpu, Settings2,
  Terminal, Sparkles, AlertTriangle, Languages, CheckCheck,
} from 'lucide-react';
import type { DotVisualStatus, CapsuleVisualStatus } from './types';

// ── Event Colors ──────────────────────────────────────────

export const EVENT_COLORS: Record<string, { dot: string; glow: string; bg: string; text: string; label: string; icon: React.ReactNode }> = {
  // ── Chat ──
  user_message:             { dot: 'bg-blue-400',    glow: 'shadow-blue-400/50',    bg: 'bg-blue-400/10',    text: 'text-blue-400',    label: 'User',      icon: <MessageSquare className="w-3 h-3" /> },
  assistant_message:        { dot: 'bg-teal-400',    glow: 'shadow-teal-400/50',    bg: 'bg-teal-400/10',    text: 'text-teal-400',    label: 'Assistant',  icon: <Brain className="w-3 h-3" /> },
  thinking_message:         { dot: 'bg-violet-400',  glow: 'shadow-violet-400/50',  bg: 'bg-violet-400/10',  text: 'text-violet-400',  label: 'Thinking',   icon: <Brain className="w-3 h-3" /> },
  // ── Unified CLI Engine (cyan gradient) ──
  cli_request_started:      { dot: 'bg-fuchsia-400', glow: 'shadow-fuchsia-400/50', bg: 'bg-fuchsia-400/10', text: 'text-fuchsia-400', label: 'CLI ▸',      icon: <Cpu className="w-3 h-3" /> },
  cli_request_completed:    { dot: 'bg-fuchsia-500', glow: 'shadow-fuchsia-500/50', bg: 'bg-fuchsia-500/10', text: 'text-fuchsia-400', label: 'CLI ◂',      icon: <Cpu className="w-3 h-3" /> },
  cli_tool_activity:        { dot: 'bg-fuchsia-300', glow: 'shadow-fuchsia-300/50', bg: 'bg-fuchsia-300/10', text: 'text-fuchsia-300', label: 'CLI Tool',   icon: <Wrench className="w-3 h-3" /> },
  // ── Legacy: Gemini (purple) ──
  gemini_request_started:   { dot: 'bg-fuchsia-400', glow: 'shadow-fuchsia-400/50', bg: 'bg-fuchsia-400/10', text: 'text-fuchsia-400', label: 'Gemini ▸',   icon: <Cpu className="w-3 h-3" /> },
  gemini_request_completed: { dot: 'bg-fuchsia-500', glow: 'shadow-fuchsia-500/50', bg: 'bg-fuchsia-500/10', text: 'text-fuchsia-400', label: 'Gemini ◂',   icon: <Cpu className="w-3 h-3" /> },
  // ── Legacy: GPT / Codex (lime-green) ──
  codex_request_started:    { dot: 'bg-lime-400',    glow: 'shadow-lime-400/50',    bg: 'bg-lime-400/10',    text: 'text-lime-400',    label: 'GPT ▸',      icon: <Zap className="w-3 h-3" /> },
  codex_request_completed:  { dot: 'bg-lime-500',    glow: 'shadow-lime-500/50',    bg: 'bg-lime-500/10',    text: 'text-lime-400',    label: 'GPT ◂',      icon: <Zap className="w-3 h-3" /> },
  // ── Code (yellow) ──
  git_commit:               { dot: 'bg-yellow-400',  glow: 'shadow-yellow-400/50',  bg: 'bg-yellow-400/10',  text: 'text-yellow-400',  label: 'Commit',     icon: <GitCommit className="w-3 h-3" /> },
  // ── Flow ──
  task_lifecycle:           { dot: 'bg-sky-400',     glow: 'shadow-sky-400/50',     bg: 'bg-sky-400/10',     text: 'text-sky-400',     label: 'Task',       icon: <Activity className="w-3 h-3" /> },
  question_created:         { dot: 'bg-amber-400',   glow: 'shadow-amber-400/50',   bg: 'bg-amber-400/10',   text: 'text-amber-400',   label: 'Question',   icon: <MessageSquare className="w-3 h-3" /> },
  question_resolved:        { dot: 'bg-amber-300',   glow: 'shadow-amber-300/50',   bg: 'bg-amber-300/10',   text: 'text-amber-300',   label: 'Resolved',   icon: <MessageSquare className="w-3 h-3" /> },
  decision_made:            { dot: 'bg-orange-400',  glow: 'shadow-orange-400/50',  bg: 'bg-orange-400/10',  text: 'text-orange-400',  label: 'Decision',   icon: <GitBranch className="w-3 h-3" /> },
  insight_generated:        { dot: 'bg-emerald-400', glow: 'shadow-emerald-400/50', bg: 'bg-emerald-400/10', text: 'text-emerald-400', label: 'Insight',    icon: <Sparkles className="w-3 h-3" /> },
  // ── Board (indigo) ──
  board_task_created:       { dot: 'bg-indigo-400',  glow: 'shadow-indigo-400/50',  bg: 'bg-indigo-400/10',  text: 'text-indigo-400',  label: 'Created',    icon: <Activity className="w-3 h-3" /> },
  board_task_status_changed:{ dot: 'bg-indigo-300',  glow: 'shadow-indigo-300/50',  bg: 'bg-indigo-300/10',  text: 'text-indigo-300',  label: 'Status',     icon: <Activity className="w-3 h-3" /> },
  board_task_note_added:    { dot: 'bg-indigo-200',  glow: 'shadow-indigo-200/50',  bg: 'bg-indigo-200/10',  text: 'text-indigo-200',  label: 'Note',       icon: <MessageSquare className="w-3 h-3" /> },
  board_task_claimed:       { dot: 'bg-indigo-500',  glow: 'shadow-indigo-500/50',  bg: 'bg-indigo-500/10',  text: 'text-indigo-400',  label: 'Claimed',    icon: <Wrench className="w-3 h-3" /> },
  board_task_deleted:       { dot: 'bg-red-400',     glow: 'shadow-red-400/50',     bg: 'bg-red-400/10',     text: 'text-red-400',     label: 'Deleted',    icon: <AlertTriangle className="w-3 h-3" /> },
  board_task_updated:       { dot: 'bg-indigo-300',  glow: 'shadow-indigo-300/50',  bg: 'bg-indigo-300/10',  text: 'text-indigo-300',  label: 'Board',      icon: <Activity className="w-3 h-3" /> },
  // ── System (slate/cyan/rose) ──
  slot_state_changed:       { dot: 'bg-slate-400',   glow: 'shadow-slate-400/50',   bg: 'bg-slate-400/10',   text: 'text-slate-400',   label: 'Slot',       icon: <Settings2 className="w-3 h-3" /> },
  memory_phase_changed:     { dot: 'bg-cyan-400',    glow: 'shadow-cyan-400/50',    bg: 'bg-cyan-400/10',    text: 'text-cyan-400',    label: 'Memory',     icon: <Brain className="w-3 h-3" /> },
  briefing_batch_started:   { dot: 'bg-rose-300',    glow: 'shadow-rose-300/50',    bg: 'bg-rose-300/10',    text: 'text-rose-300',    label: 'Briefing',   icon: <Sparkles className="w-3 h-3" /> },
  briefing_summary_generated: { dot: 'bg-rose-400',  glow: 'shadow-rose-400/50',    bg: 'bg-rose-400/10',    text: 'text-rose-400',    label: 'Summary',    icon: <Sparkles className="w-3 h-3" /> },
  system_message:           { dot: 'bg-slate-400',   glow: 'shadow-slate-400/50',   bg: 'bg-slate-400/10',   text: 'text-slate-400',   label: 'Daemon',     icon: <Terminal className="w-3 h-3" /> },
  slot_task_dispatched:     { dot: 'bg-amber-400',   glow: 'shadow-amber-400/50',   bg: 'bg-amber-400/10',   text: 'text-amber-400',   label: 'Dispatch',   icon: <ArrowRight className="w-3 h-3" /> },
  // ── Translation Worker (indigo) ──
  translation_started:      { dot: 'bg-indigo-400',  glow: 'shadow-indigo-400/50',  bg: 'bg-indigo-400/10',  text: 'text-indigo-400',  label: 'Translating', icon: <Languages className="w-3 h-3" /> },
  translation_completed:    { dot: 'bg-indigo-500',  glow: 'shadow-indigo-500/50',  bg: 'bg-indigo-500/10',  text: 'text-indigo-400',  label: 'Translated',  icon: <CheckCheck className="w-3 h-3" /> },
  translation_failed:       { dot: 'bg-red-400',     glow: 'shadow-red-400/50',     bg: 'bg-red-400/10',     text: 'text-red-400',     label: 'Trans Err',   icon: <AlertTriangle className="w-3 h-3" /> },
  // ── Step Narrator (violet) ──
  narration_batch_started:  { dot: 'bg-violet-300',  glow: 'shadow-violet-300/50',  bg: 'bg-violet-300/10',  text: 'text-violet-300',  label: 'Narrator',    icon: <Sparkles className="w-3 h-3" /> },
  narration_session_started:{ dot: 'bg-violet-400',  glow: 'shadow-violet-400/50',  bg: 'bg-violet-400/10',  text: 'text-violet-400',  label: 'Narrating',   icon: <Sparkles className="w-3 h-3" /> },
  narration_completed:      { dot: 'bg-violet-500',  glow: 'shadow-violet-500/50',  bg: 'bg-violet-500/10',  text: 'text-violet-400',  label: 'Narrated',    icon: <CheckCheck className="w-3 h-3" /> },
  narration_failed:         { dot: 'bg-red-400',     glow: 'shadow-red-400/50',     bg: 'bg-red-400/10',     text: 'text-red-400',     label: 'Narr Err',    icon: <AlertTriangle className="w-3 h-3" /> },
};

// ── Slot Colors ──

export const SLOT_COLORS: Record<string, { badge: string; border: string; line: string }> = {
  'slot-coder-1':      { badge: 'bg-blue-500/20 text-blue-300',    border: 'border-l-blue-400',   line: 'rgba(96,165,250,0.25)' },
  'slot-coder-bypass': { badge: 'bg-indigo-500/20 text-indigo-300', border: 'border-l-indigo-400', line: 'rgba(129,140,248,0.25)' },
  'slot-deploy-1':     { badge: 'bg-orange-500/20 text-orange-300', border: 'border-l-orange-400', line: 'rgba(251,146,60,0.25)' },
  'slot-memory':       { badge: 'bg-cyan-500/20 text-cyan-300',    border: 'border-l-cyan-400',   line: 'rgba(34,211,238,0.25)' },
  'slot-memory-slow':  { badge: 'bg-teal-500/20 text-teal-300',    border: 'border-l-teal-400',   line: 'rgba(45,212,191,0.25)' },
};

// Fallback for unknown slot IDs — cycle through a palette
const SLOT_FALLBACK_LINES = [
  'rgba(168,85,247,0.25)',   // purple
  'rgba(244,114,182,0.25)',  // pink
  'rgba(74,222,128,0.25)',   // green
  'rgba(251,191,36,0.25)',   // amber
];
export function getSlotLine(slotId: string): string {
  const known = SLOT_COLORS[slotId];
  if (known) return known.line;
  // Deterministic hash for unknown slots
  let hash = 0;
  for (let i = 0; i < slotId.length; i++) hash = ((hash << 5) - hash + slotId.charCodeAt(i)) | 0;
  return SLOT_FALLBACK_LINES[Math.abs(hash) % SLOT_FALLBACK_LINES.length];
}

// ── Swimlanes ──

export const SWIMLANES = [
  { id: 'chat',    label: 'Chat',      accent: { dot: 'bg-indigo-400',  css: '#818cf8', bg: 'bg-indigo-500/[0.03]' }, types: ['user_message', 'assistant_message', 'thinking_message'] },
  { id: 'slot',    label: 'Slot',      accent: { dot: 'bg-teal-400',    css: '#2dd4bf', bg: 'bg-teal-500/[0.03]' },   types: ['slot_task_dispatched', 'system_message'] },
  { id: 'ai',      label: 'AI / LLM',  accent: { dot: 'bg-emerald-400', css: '#34d399', bg: 'bg-emerald-500/[0.03]' }, types: ['cli_request_started', 'cli_request_completed', 'cli_tool_activity', 'gemini_request_started', 'gemini_request_completed', 'decision_made', 'insight_generated'] },
  { id: 'gpt',     label: 'GPT',       accent: { dot: 'bg-amber-400',   css: '#fbbf24', bg: 'bg-amber-500/[0.03]' },  types: ['codex_request_started', 'codex_request_completed'] },
  { id: 'code',    label: 'Code',      accent: { dot: 'bg-blue-400',    css: '#60a5fa', bg: 'bg-blue-500/[0.03]' },   types: ['git_commit'] },
  { id: 'flow',    label: 'Flow',      accent: { dot: 'bg-orange-400',  css: '#fb923c', bg: 'bg-orange-500/[0.03]' }, types: ['task_lifecycle', 'question_created', 'question_resolved'] },
  { id: 'board',   label: 'Board',     accent: { dot: 'bg-pink-400',    css: '#f472b6', bg: 'bg-pink-500/[0.03]' },   types: ['board_task_created', 'board_task_status_changed', 'board_task_note_added', 'board_task_claimed', 'board_task_deleted', 'board_task_updated'] },
  { id: 'sys',     label: 'System',    accent: { dot: 'bg-slate-400',   css: '#94a3b8', bg: 'bg-slate-500/[0.03]' },  types: ['slot_state_changed', 'memory_phase_changed', 'briefing_batch_started', 'briefing_summary_generated', 'narration_batch_started', 'narration_session_started', 'narration_completed', 'narration_failed'] },
];

export const SLOT_LANE_IDX = SWIMLANES.findIndex(s => s.id === 'slot');

// ── Session Colors ──

export const SESSION_COLORS = [
  { dot: 'bg-cyan-400',    line: 'rgba(34,211,238,0.25)',  ring: 'ring-cyan-400/40' },
  { dot: 'bg-green-400',   line: 'rgba(74,222,128,0.25)',  ring: 'ring-green-400/40' },
  { dot: 'bg-amber-400',   line: 'rgba(251,191,36,0.25)',  ring: 'ring-amber-400/40' },
  { dot: 'bg-pink-400',    line: 'rgba(244,114,182,0.25)', ring: 'ring-pink-400/40' },
  { dot: 'bg-violet-400',  line: 'rgba(167,139,250,0.25)', ring: 'ring-violet-400/40' },
  { dot: 'bg-orange-400',  line: 'rgba(251,146,60,0.25)',  ring: 'ring-orange-400/40' },
  { dot: 'bg-teal-300',    line: 'rgba(94,234,212,0.25)',  ring: 'ring-teal-300/40' },
  { dot: 'bg-rose-400',    line: 'rgba(251,113,133,0.25)', ring: 'ring-rose-400/40' },
];

// ── Window Options ──

export const WINDOW_OPTIONS = [
  { label: '5m', value: '5min' },
  { label: '10m', value: '10min' },
  { label: '30m', value: '30min' },
  { label: '1h', value: '1h' },
  { label: '6h', value: '6h' },
  { label: '24h', value: '24h' },
];

// ── Visual Status Styles ──

export const DOT_STYLES: Record<DotVisualStatus, string> = {
  focused:     'ring-[3px] ring-white/50 scale-[1.6] z-[35] shadow-[0_0_10px_var(--tw-shadow-color)]',
  highlighted: 'ring-2 ring-white/30 scale-125 z-30 shadow-[0_0_6px_var(--tw-shadow-color)]',
  dimmed:      'opacity-20 scale-90',
  normal:      '',
};

export const CAPSULE_STYLES: Record<CapsuleVisualStatus, string> = {
  selected: 'z-[5] shadow-lg shadow-black/40',
  dimmed:   'z-[1] opacity-25',
  normal:   'z-[1]',
};

// ── View Persistence ──

export const STORAGE_KEY = 'timeline-view-state';
