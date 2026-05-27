import { create } from 'zustand';
import {
  EVENT_CUSTOM_EVENTS,
  EVENT_PREFIX_ROUTES,
  EVENT_ROUTE_TABLE,
  RESYNC_VERSION_KEYS,
  type EventRouteConfig,
  type EventVersionKey,
} from './generated/board-frontend-config';

const WS_PORT = parseInt(process.env.NEXT_PUBLIC_WS_PORT || '9120', 10);
export const EVENT_HEALTH_STALE_AFTER_MS = 30_000;

type ConnectionState = 'connecting' | 'connected' | 'disconnected';
export type EventHealthStatus = 'ok' | 'stale' | 'connecting' | 'disconnected' | 'error';
export type EventHealthSeverity = 'good' | 'warn' | 'bad';

export interface FrontendEvent {
  type: string;
  ts: number;
  seq: number;
  schema?: string;
  subscriber_id?: string;
  missed?: number;
  latest_seq?: number;
  last_client_seq?: number;
  cursor_lag?: number;
  lag_class?: string;
  consecutive_lags?: number;
  diagnostic?: string;
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  payload?: any;
}

// eslint-disable-next-line @typescript-eslint/no-explicit-any
export type HealthSnapshot = Record<string, any>;

export interface EventBusLagDiagnostic {
  schema: string;
  subscriberId: string;
  lagClass: string;
  missed: number;
  latestSeq: number;
  lastClientSeq: number;
  cursorLag: number;
  consecutiveLags: number;
  diagnostic: string;
  observedAt: number;
}

interface EventStreamState {
  // Connection
  ws: WebSocket | null;
  connectionState: ConnectionState;
  lastSeq: number;
  reconnectAttempts: number;
  lastMessageAt: number | null;
  lastError: string | null;
  lastResyncAt: number | null;
  lastResyncReason: string | null;
  lastLagDiagnostic: EventBusLagDiagnostic | null;
  malformedCount: number;
  eventHealthStatus: EventHealthStatus;
  eventHealthSeverity: EventHealthSeverity;
  eventHealthIsStale: boolean;
  eventHealthAgeMs: number | null;

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
  refreshEventHealth: () => void;
}

// Debounce timers for version bumps
const bumpTimers: Record<string, ReturnType<typeof setTimeout>> = {};

function debouncedBump(
  set: (fn: (s: EventStreamState) => Partial<EventStreamState>) => void,
  key: EventVersionKey,
  delayMs = 80,
) {
  if (bumpTimers[key]) clearTimeout(bumpTimers[key]);
  bumpTimers[key] = setTimeout(() => {
    set((s) => ({ [key]: s[key] + 1 }) as Partial<EventStreamState>);
  }, delayMs);
}

function bumpKeys(
  set: (fn: (s: EventStreamState) => Partial<EventStreamState>) => void,
  keys: readonly EventVersionKey[],
  delayMs?: number,
) {
  for (const key of keys) debouncedBump(set, key, delayMs);
}

function deriveEventHealth(state: Pick<EventStreamState, 'connectionState' | 'lastMessageAt' | 'lastError'>): Pick<
  EventStreamState,
  'eventHealthStatus' | 'eventHealthSeverity' | 'eventHealthIsStale' | 'eventHealthAgeMs'
> {
  const ageMs = state.lastMessageAt ? Math.max(0, Date.now() - state.lastMessageAt) : null;
  const isStale = state.connectionState === 'connected' && ageMs !== null && ageMs > EVENT_HEALTH_STALE_AFTER_MS;
  if (state.connectionState === 'disconnected') {
    return { eventHealthStatus: 'disconnected', eventHealthSeverity: 'bad', eventHealthIsStale: isStale, eventHealthAgeMs: ageMs };
  }
  if (state.connectionState === 'connecting') {
    return { eventHealthStatus: 'connecting', eventHealthSeverity: 'warn', eventHealthIsStale: isStale, eventHealthAgeMs: ageMs };
  }
  if (state.lastError) {
    return { eventHealthStatus: 'error', eventHealthSeverity: 'bad', eventHealthIsStale: isStale, eventHealthAgeMs: ageMs };
  }
  if (isStale) {
    return { eventHealthStatus: 'stale', eventHealthSeverity: 'warn', eventHealthIsStale: true, eventHealthAgeMs: ageMs };
  }
  return { eventHealthStatus: 'ok', eventHealthSeverity: 'good', eventHealthIsStale: false, eventHealthAgeMs: ageMs };
}

function dispatchConfiguredCustomEvent(event: FrontendEvent) {
  const config = EVENT_CUSTOM_EVENTS.find((item) => item.event === event.type);
  if (!config) return false;
  if (typeof window !== 'undefined' && event.payload) {
    const detail = Object.fromEntries(config.detail.map((field) => [field, event.payload?.[field]]));
    window.dispatchEvent(new CustomEvent(config.name, { detail }));
  }
  return true;
}

function numOrZero(value: unknown): number {
  return typeof value === 'number' && Number.isFinite(value) ? value : 0;
}

function stringOrEmpty(value: unknown): string {
  return typeof value === 'string' ? value : '';
}

function normalizeLagDiagnostic(event: FrontendEvent): EventBusLagDiagnostic | null {
  if (event.schema !== 'missiond.eventbus-live-lag-diagnostic.v1') return null;
  return {
    schema: event.schema,
    subscriberId: stringOrEmpty(event.subscriber_id),
    lagClass: stringOrEmpty(event.lag_class) || 'resync',
    missed: numOrZero(event.missed),
    latestSeq: numOrZero(event.latest_seq),
    lastClientSeq: numOrZero(event.last_client_seq),
    cursorLag: numOrZero(event.cursor_lag),
    consecutiveLags: numOrZero(event.consecutive_lags),
    diagnostic: stringOrEmpty(event.diagnostic),
    observedAt: Date.now(),
  };
}

function handleEvent(
  event: FrontendEvent,
  set: (fn: (s: EventStreamState) => Partial<EventStreamState>) => void,
) {
  // Update seq
  set(() => ({ lastSeq: event.seq }));

  if (event.type === 'resync') {
    const lagDiagnostic = normalizeLagDiagnostic(event);
    set(() => ({
      lastResyncAt: Date.now(),
      lastResyncReason: lagDiagnostic?.lagClass ?? (typeof event.payload?.reason === 'string' ? event.payload.reason : 'resync'),
      lastLagDiagnostic: lagDiagnostic,
    }));
    bumpKeys(set, RESYNC_VERSION_KEYS, 0);
    return;
  }

  if (dispatchConfiguredCustomEvent(event)) return;

  const route = (EVENT_ROUTE_TABLE as readonly EventRouteConfig[]).find((item) => item.events.includes(event.type));
  if (route) {
    if (route.healthSnapshot) set(() => ({ healthSnapshot: event.payload }));
    bumpKeys(set, route.bump, route.delayMs);
    if (route.deployCategoryBump && event.payload?.category === 'deploy') {
      debouncedBump(set, 'deployVersion', route.delayMs);
    }
    return;
  }

  for (const prefixRoute of EVENT_PREFIX_ROUTES) {
    if (event.type.startsWith(prefixRoute.prefix)) {
      bumpKeys(set, prefixRoute.bump, prefixRoute.delayMs);
      return;
    }
  }
}

export const useEventStreamStore = create<EventStreamState>()((set, get) => ({
  ws: null,
  connectionState: 'disconnected',
  lastSeq: 0,
  reconnectAttempts: 0,
  lastMessageAt: null,
  lastError: null,
  lastResyncAt: null,
  lastResyncReason: null,
  lastLagDiagnostic: null,
  malformedCount: 0,
  eventHealthStatus: 'disconnected',
  eventHealthSeverity: 'bad',
  eventHealthIsStale: false,
  eventHealthAgeMs: null,

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

    const wsHost = process.env.NEXT_PUBLIC_WS_HOST || (typeof window !== 'undefined' ? window.location.hostname : 'localhost');
    const ws = new WebSocket(`ws://${wsHost}:${WS_PORT}/events`);
    set({ ws, connectionState: 'connecting', lastError: null });
    get().refreshEventHealth();

    ws.onopen = () => {
      set({ connectionState: 'connected', reconnectAttempts: 0, lastError: null });
      get().refreshEventHealth();
    };

    ws.onmessage = (e) => {
      set({ lastMessageAt: Date.now() });
      get().refreshEventHealth();
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
          handleEvent({
            type: 'resync',
            ts: event.ts,
            seq: event.seq,
            payload: { reason: 'too_far_behind' },
          } as FrontendEvent, set);
          set({ lastSeq: event.seq });
          return;
        }
        // Dedup: skip events we've already seen
        if (event.seq > 0 && event.seq <= get().lastSeq) return;
        handleEvent(event, set);
      } catch (err) {
        set((s) => ({
          malformedCount: s.malformedCount + 1,
          lastError: err instanceof Error ? err.message : 'malformed websocket message',
        }));
        get().refreshEventHealth();
      }
    };

    ws.onclose = () => {
      set({ ws: null, connectionState: 'disconnected', lastError: 'websocket closed' });
      get().refreshEventHealth();
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
      set({ lastError: 'websocket error' });
      get().refreshEventHealth();
    };
  },

  disconnect: () => {
    const { ws } = get();
    if (ws) {
      ws.close();
      set({ ws: null, connectionState: 'disconnected' });
      get().refreshEventHealth();
    }
  },

  refreshEventHealth: () => {
    set((state) => deriveEventHealth(state));
  },
}));
