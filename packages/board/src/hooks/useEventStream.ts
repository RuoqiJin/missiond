import { useEffect } from 'react';
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
  domain: 'slot' | 'task' | 'question' | 'decision' | 'memory' | 'deploy' | 'engine',
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
