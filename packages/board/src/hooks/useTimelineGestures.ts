import { useState, useEffect, useRef, useCallback } from 'react';

export interface TimeRange {
  min: number;
  max: number;
}

interface UseTimelineGesturesOptions {
  initialRange: TimeRange;
  /** Minimum visible window in ms (max zoom in). Default: 30s */
  minWindowMs?: number;
  /** Maximum visible window in ms (max zoom out). Default: 7 days */
  maxWindowMs?: number;
  /** Right boundary — cannot pan beyond this. Default: Date.now() + 5min buffer */
  maxAllowedTime?: number;
}

export function useTimelineGestures({
  initialRange,
  minWindowMs = 30 * 1000,
  maxWindowMs = 7 * 24 * 3600 * 1000,
  maxAllowedTime = Date.now() + 5 * 60 * 1000,
}: UseTimelineGesturesOptions) {
  const [timeRange, setTimeRange] = useState<TimeRange>(initialRange);
  const containerRef = useRef<HTMLDivElement>(null);

  // Use ref to avoid stale closures in high-frequency event handlers
  const stateRef = useRef(timeRange);
  stateRef.current = timeRange;

  // Unified range updater with boundary constraints
  const updateRange = useCallback((newMin: number, newMax: number) => {
    let min = newMin;
    let max = newMax;
    let windowMs = max - min;

    // Zoom bounds
    if (windowMs < minWindowMs) {
      const center = (min + max) / 2;
      min = center - minWindowMs / 2;
      max = center + minWindowMs / 2;
      windowMs = minWindowMs;
    } else if (windowMs > maxWindowMs) {
      const center = (min + max) / 2;
      min = center - maxWindowMs / 2;
      max = center + maxWindowMs / 2;
      windowMs = maxWindowMs;
    }

    // Cannot pan into the future
    if (max > maxAllowedTime) {
      max = maxAllowedTime;
      min = max - windowMs;
    }

    // Only update if changed meaningfully (>1ms)
    const prev = stateRef.current;
    if (Math.abs(prev.min - min) > 1 || Math.abs(prev.max - max) > 1) {
      const next = { min, max };
      stateRef.current = next;
      setTimeRange(next);
    }
  }, [minWindowMs, maxWindowMs, maxAllowedTime]);

  useEffect(() => {
    const container = containerRef.current;
    if (!container) return;

    const handleWheel = (e: WheelEvent) => {
      // Prevent browser default scroll / back-forward navigation
      e.preventDefault();

      const { min, max } = stateRef.current;
      const windowMs = max - min;
      const rect = container.getBoundingClientRect();
      const mouseX = e.clientX - rect.left;
      const anchorRatio = Math.max(0, Math.min(1, mouseX / rect.width));

      if (e.ctrlKey || e.metaKey) {
        // ── Pinch-to-zoom (trackpad sends ctrlKey + deltaY) ──
        const zoomSensitivity = 0.005;
        const zoomFactor = Math.exp(e.deltaY * zoomSensitivity);
        const newWindowMs = windowMs * zoomFactor;

        // Zoom anchored at mouse position
        const anchorTime = min + windowMs * anchorRatio;
        const newMin = anchorTime - newWindowMs * anchorRatio;
        const newMax = anchorTime + newWindowMs * (1 - anchorRatio);

        updateRange(newMin, newMax);
      } else {
        // ── Two-finger scroll (horizontal pan) ──
        // Prefer deltaX for horizontal scroll; fall back to deltaY for vertical-only mice
        const delta = Math.abs(e.deltaX) > Math.abs(e.deltaY) ? e.deltaX : e.deltaY;

        // Convert pixel displacement to time displacement
        const panSensitivity = windowMs / rect.width;
        const timeShift = delta * panSensitivity;

        updateRange(min + timeShift, max + timeShift);
      }
    };

    // Safari gesturestart/gesturechange for non-standard pinch
    let gestureStartRange: TimeRange | null = null;

    const handleGestureStart = (e: Event) => {
      e.preventDefault();
      gestureStartRange = { ...stateRef.current };
    };

    const handleGestureChange = (e: Event) => {
      e.preventDefault();
      if (!gestureStartRange) return;
      const ge = e as unknown as { scale: number; clientX: number };
      const scale = ge.scale;

      const rect = container.getBoundingClientRect();
      const mouseX = ge.clientX - rect.left;
      const anchorRatio = Math.max(0, Math.min(1, mouseX / rect.width));

      const origWindow = gestureStartRange.max - gestureStartRange.min;
      const newWindowMs = origWindow / scale;
      const anchorTime = gestureStartRange.min + origWindow * anchorRatio;

      updateRange(
        anchorTime - newWindowMs * anchorRatio,
        anchorTime + newWindowMs * (1 - anchorRatio),
      );
    };

    const handleGestureEnd = (e: Event) => {
      e.preventDefault();
      gestureStartRange = null;
    };

    // Must be passive: false to call preventDefault
    container.addEventListener('wheel', handleWheel, { passive: false });
    container.addEventListener('gesturestart', handleGestureStart, { passive: false } as EventListenerOptions);
    container.addEventListener('gesturechange', handleGestureChange, { passive: false } as EventListenerOptions);
    container.addEventListener('gestureend', handleGestureEnd, { passive: false } as EventListenerOptions);

    return () => {
      container.removeEventListener('wheel', handleWheel);
      container.removeEventListener('gesturestart', handleGestureStart);
      container.removeEventListener('gesturechange', handleGestureChange);
      container.removeEventListener('gestureend', handleGestureEnd);
    };
  }, [updateRange]);

  return {
    containerRef,
    timeRange,
    setTimeRange: (range: TimeRange) => {
      stateRef.current = range;
      setTimeRange(range);
    },
  };
}
