import React from 'react';
import {
  Zap, Brain, Wrench, ArrowRight,
  MessageSquare, GitBranch, GitCommit, Activity, Cpu, Settings2,
  Terminal, Sparkles, AlertTriangle, Languages, CheckCheck,
} from 'lucide-react';
import {
  TIMELINE_EVENT_VISUALS,
  TIMELINE_SESSION_COLORS,
  TIMELINE_SLOT_COLORS,
  TIMELINE_SLOT_FALLBACK_LINES,
  TIMELINE_SWIMLANES,
  TIMELINE_WINDOW_OPTIONS,
} from '../../generated/board-frontend-config';
import type { DotVisualStatus, CapsuleVisualStatus } from './types';

const EVENT_ICON_MAP: Record<string, React.ReactNode> = {
  Activity: <Activity className="w-3 h-3" />,
  AlertTriangle: <AlertTriangle className="w-3 h-3" />,
  ArrowRight: <ArrowRight className="w-3 h-3" />,
  Brain: <Brain className="w-3 h-3" />,
  CheckCheck: <CheckCheck className="w-3 h-3" />,
  Cpu: <Cpu className="w-3 h-3" />,
  GitBranch: <GitBranch className="w-3 h-3" />,
  GitCommit: <GitCommit className="w-3 h-3" />,
  Languages: <Languages className="w-3 h-3" />,
  MessageSquare: <MessageSquare className="w-3 h-3" />,
  Settings2: <Settings2 className="w-3 h-3" />,
  Sparkles: <Sparkles className="w-3 h-3" />,
  Terminal: <Terminal className="w-3 h-3" />,
  Wrench: <Wrench className="w-3 h-3" />,
  Zap: <Zap className="w-3 h-3" />,
};

export const EVENT_COLORS: Record<string, { dot: string; glow: string; bg: string; text: string; label: string; icon: React.ReactNode }> =
  Object.fromEntries(Object.entries(TIMELINE_EVENT_VISUALS).map(([type, config]) => [
    type,
    { ...config, icon: EVENT_ICON_MAP[config.icon] ?? null },
  ]));

export const SLOT_COLORS = TIMELINE_SLOT_COLORS as Record<string, { badge: string; border: string; line: string }>;
const SLOT_FALLBACK_LINES = TIMELINE_SLOT_FALLBACK_LINES as readonly string[];

export function getSlotLine(slotId: string): string {
  const known = SLOT_COLORS[slotId];
  if (known) return known.line;
  // Deterministic hash for unknown slots
  let hash = 0;
  for (let i = 0; i < slotId.length; i++) hash = ((hash << 5) - hash + slotId.charCodeAt(i)) | 0;
  return SLOT_FALLBACK_LINES[Math.abs(hash) % SLOT_FALLBACK_LINES.length];
}

export const SWIMLANES = TIMELINE_SWIMLANES as readonly { id: string; label: string; accent: { dot: string; css: string; bg: string }; types: readonly string[] }[];
export const SLOT_LANE_IDX = SWIMLANES.findIndex(s => s.id === 'slot');

export const SESSION_COLORS = TIMELINE_SESSION_COLORS;
export const WINDOW_OPTIONS = TIMELINE_WINDOW_OPTIONS;

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

// ── CSS Helpers ──

/** CSS calc expression for GPU-driven horizontal positioning via custom properties */
export const CSS_LEFT = 'calc((var(--t-event) - var(--t-min)) / var(--t-range) * 100%)';

// ── View Persistence ──

export const STORAGE_KEY = 'timeline-view-state';
