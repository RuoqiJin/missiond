import { useEffect } from 'react';
import { useShallow } from 'zustand/react/shallow';
import { useEventStreamStore } from '../eventStream';

/**
 * Initialize the global EventBus WebSocket connection.
 * Call once in App.tsx — the connection persists across tab switches.
 */
export function useEventStream() {
  const connect = useEventStreamStore((s) => s.connect);

  useEffect(() => {
    connect();
    // WS stays alive across component lifecycle — no cleanup disconnect
  }, [connect]);
}

/**
 * Subscribe to a version counter for a specific domain.
 * When the counter bumps (due to a WS event), the component re-renders
 * and can refetch data.
 */
export function useEventInvalidation(
  domain: 'slot' | 'task' | 'question' | 'decision' | 'memory' | 'deploy' | 'engine' | 'timeline',
): number {
  const key = `${domain}Version` as const;
  return useEventStreamStore((s) => s[key]);
}

/**
 * Get the current WS connection state.
 */
export function useConnectionState() {
  return useEventStreamStore((s) => s.connectionState);
}

/**
 * Get EventBus WebSocket health details for operator-facing diagnostics.
 */
export function useEventHealth() {
  const refreshEventHealth = useEventStreamStore((s) => s.refreshEventHealth);
  useEffect(() => {
    refreshEventHealth();
    const timer = window.setInterval(refreshEventHealth, 5_000);
    return () => window.clearInterval(timer);
  }, [refreshEventHealth]);

  return useEventStreamStore(
    useShallow((s) => ({
      connectionState: s.connectionState,
      lastSeq: s.lastSeq,
      reconnectAttempts: s.reconnectAttempts,
      lastMessageAt: s.lastMessageAt,
      lastError: s.lastError,
      lastResyncAt: s.lastResyncAt,
      lastResyncReason: s.lastResyncReason,
      malformedCount: s.malformedCount,
      status: s.eventHealthStatus,
      severity: s.eventHealthSeverity,
      isStale: s.eventHealthIsStale,
      ageMs: s.eventHealthAgeMs,
    })),
  );
}
