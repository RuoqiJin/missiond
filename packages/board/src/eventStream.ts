import { create } from 'zustand';

const WS_PORT = 9120;

type ConnectionState = 'connecting' | 'connected' | 'disconnected';

export interface FrontendEvent {
  type: string;
  ts: number;
  seq: number;
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  payload?: any;
}

// eslint-disable-next-line @typescript-eslint/no-explicit-any
export type HealthSnapshot = Record<string, any>;

interface EventStreamState {
  // Connection
  ws: WebSocket | null;
  connectionState: ConnectionState;
  lastSeq: number;
  reconnectAttempts: number;

  // Push snapshots (used directly, no HTTP refetch)
  healthSnapshot: HealthSnapshot | null;

  // Version counters — bump triggers debounced refetch in subscribed components
  slotVersion: number;
  taskVersion: number;
  questionVersion: number;
  decisionVersion: number;
  memoryVersion: number;
  deployVersion: number;
  engineVersion: number;
  timelineVersion: number;

  // Actions
  connect: () => void;
  disconnect: () => void;
}

// Debounce timers for version bumps
const bumpTimers: Record<string, ReturnType<typeof setTimeout>> = {};

function debouncedBump(
  set: (fn: (s: EventStreamState) => Partial<EventStreamState>) => void,
  key: keyof Pick<EventStreamState, 'slotVersion' | 'taskVersion' | 'questionVersion' | 'decisionVersion' | 'memoryVersion' | 'deployVersion' | 'engineVersion' | 'timelineVersion'>,
  delayMs = 80,
) {
  if (bumpTimers[key]) clearTimeout(bumpTimers[key]);
  bumpTimers[key] = setTimeout(() => {
    set((s) => ({ [key]: s[key] + 1 }) as Partial<EventStreamState>);
  }, delayMs);
}

function handleEvent(
  event: FrontendEvent,
  set: (fn: (s: EventStreamState) => Partial<EventStreamState>) => void,
) {
  // Update seq
  set(() => ({ lastSeq: event.seq }));

  switch (event.type) {
    case 'health_snapshot':
      set(() => ({ healthSnapshot: event.payload }));
      debouncedBump(set, 'engineVersion');
      break;

    case 'slot_state_changed':
      debouncedBump(set, 'slotVersion');
      break;

    case 'task_lifecycle':
      debouncedBump(set, 'taskVersion');
      break;

    case 'question_created':
    case 'question_resolved':
      debouncedBump(set, 'questionVersion');
      break;

    case 'decision_made':
      debouncedBump(set, 'decisionVersion');
      break;

    case 'memory_phase_changed':
      debouncedBump(set, 'memoryVersion');
      break;

    case 'gemini_request_started':
    case 'gemini_request_completed':
    case 'codex_request_started':
    case 'codex_request_completed':
      debouncedBump(set, 'engineVersion');
      debouncedBump(set, 'timelineVersion');
      break;

    case 'board_task_updated': {
      debouncedBump(set, 'taskVersion');
      const cat = event.payload?.category;
      if (cat === 'deploy') debouncedBump(set, 'deployVersion');
      break;
    }

    case 'insight_generated':
      debouncedBump(set, 'timelineVersion');
      break;

    case 'slot_task_dispatched':
      debouncedBump(set, 'slotVersion');
      debouncedBump(set, 'timelineVersion');
      break;

    case 'briefing_batch_started':
      debouncedBump(set, 'timelineVersion');
      break;

    case 'briefing_summary_generated':
      // In-place cache update: dispatch CustomEvent so timeline can update without refetch
      if (typeof window !== 'undefined' && event.payload) {
        window.dispatchEvent(new CustomEvent('timeline-summary-update', {
          detail: { target_seq: event.payload.target_seq, summary: event.payload.summary },
        }));
      }
      break;

    case 'resync':
      // Server says we missed events — bump all versions to force refetch
      debouncedBump(set, 'slotVersion', 0);
      debouncedBump(set, 'taskVersion', 0);
      debouncedBump(set, 'questionVersion', 0);
      debouncedBump(set, 'decisionVersion', 0);
      debouncedBump(set, 'memoryVersion', 0);
      debouncedBump(set, 'deployVersion', 0);
      debouncedBump(set, 'engineVersion', 0);
      debouncedBump(set, 'timelineVersion', 0);
      break;

    default:
      break;
  }
}

export const useEventStreamStore = create<EventStreamState>()((set, get) => ({
  ws: null,
  connectionState: 'disconnected',
  lastSeq: 0,
  reconnectAttempts: 0,

  healthSnapshot: null,

  slotVersion: 0,
  taskVersion: 0,
  questionVersion: 0,
  decisionVersion: 0,
  memoryVersion: 0,
  deployVersion: 0,
  engineVersion: 0,
  timelineVersion: 0,

  connect: () => {
    const state = get();
    // Don't connect if already connecting/connected
    if (state.ws && state.connectionState !== 'disconnected') return;

    const ws = new WebSocket(`ws://localhost:${WS_PORT}/events`);
    set({ ws, connectionState: 'connecting' });

    ws.onopen = () => {
      set({ connectionState: 'connected', reconnectAttempts: 0 });
    };

    ws.onmessage = (e) => {
      try {
        const event: FrontendEvent = JSON.parse(e.data);
        if (event.type === 'connected') {
          // Server sends latest seq; if we have a previous seq, request catch-up
          const prevSeq = get().lastSeq;
          set({ lastSeq: event.seq });
          if (prevSeq > 0 && prevSeq < event.seq) {
            ws.send(JSON.stringify({ action: 'sync', since_seq: prevSeq }));
          }
          return;
        }
        if (event.type === 'caught_up') {
          // Catch-up replay finished — update seq
          set({ lastSeq: event.seq });
          return;
        }
        if (event.type === 'too_far_behind') {
          // Gap too large — bump all versions to trigger full HTTP refresh
          handleEvent({ type: 'resync', ts: event.ts, seq: event.seq } as FrontendEvent, set);
          set({ lastSeq: event.seq });
          return;
        }
        // Dedup: skip events we've already seen
        if (event.seq > 0 && event.seq <= get().lastSeq) return;
        handleEvent(event, set);
      } catch {
        // Ignore malformed messages
      }
    };

    ws.onclose = () => {
      set({ ws: null, connectionState: 'disconnected' });
      // Exponential backoff reconnect: 1s, 2s, 4s, 8s, 16s cap
      const attempts = get().reconnectAttempts;
      const delay = Math.min(1000 * Math.pow(2, attempts), 16000);
      setTimeout(() => {
        set({ reconnectAttempts: attempts + 1 });
        get().connect();
      }, delay);
    };

    ws.onerror = () => {
      // onclose will fire after onerror
    };
  },

  disconnect: () => {
    const { ws } = get();
    if (ws) {
      ws.close();
      set({ ws: null, connectionState: 'disconnected' });
    }
  },
}));
