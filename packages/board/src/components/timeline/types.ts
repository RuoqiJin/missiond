// ── Timeline Types ──────────────────────────────────────────

export interface TimelineEvent {
  seq: number;
  event_type: string;
  trace_id: string | null;
  span_id: string | null;
  parent_span_id: string | null;
  summary: string | null;
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  payload: any;
  created_at: string;
}

export type SelectionScope = 'global' | 'session' | 'trace';

export interface SelectionState {
  scope: SelectionScope;
  scopeId: string | null;       // session_id or trace_id
  focusedSeq: number | null;    // current focused event seq
  contextSeqs: number[];
  source: 'timeline' | 'list' | null;
}

export const EMPTY_SELECTION: SelectionState = {
  scope: 'global', scopeId: null, focusedSeq: null, contextSeqs: [], source: null,
};

export type DotVisualStatus = 'focused' | 'highlighted' | 'dimmed' | 'normal';
export type CapsuleVisualStatus = 'selected' | 'dimmed' | 'normal';

export interface TimelineStats {
  total_events: number;
  by_type: Array<[string, number]>; // [event_type, count] tuples
  traced_events: number;
  unique_traces: number;
  gemini_latency: { p50_ms: number; p90_ms: number; p99_ms: number } | null;
}

export interface FullMessage {
  content: string;
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  contentBlocks: any[] | null;
  imageCount: number;
  translation?: string | null;
}

export type ViewMode = 'relative' | 'daily';

export interface Narration {
  message_id: number;
  step_title: string;
  step_intent: string;
  step_result: string;
}
