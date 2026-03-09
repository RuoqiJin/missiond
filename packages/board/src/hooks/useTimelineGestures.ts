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

/**
 * Write CSS custom properties on the timeline container for GPU-driven positioning.
 * Elements use calc((var(--t-event) - var(--t-min)) / var(--t-range) * 100%) for left%.
 * This bypasses React re-renders during gestures entirely.
 */
function writeCssVars(el: HTMLDivElement, min: number, range: number) {
  el.style.setProperty('--t-min', String(min));
  el.style.setProperty('--t-range', String(range));
}

export function useTimelineGestures({
  initialRange,
  minWindowMs = 30 * 1000,
  maxWindowMs = 7 * 24 * 3600 * 1000,
  maxAllowedTime = Date.now() + 5 * 60 * 1000,
}: UseTimelineGesturesOptions) {
  // committedRange: debounced, drives React re-renders (histogram, ticks, etc.)
  const [committedRange, setCommittedRange] = useState<TimeRange>(initialRange);
  const containerRef = useRef<HTMLDivElement>(null);

  // Live range ref — updated on every gesture event, never triggers React render
  const stateRef = useRef(initialRange);

  // RAF + debounce refs
  const rafRef = useRef(0);
  const debounceRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const animRafRef = useRef(0); // For animated transitions

  // Clamp and apply range update
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

    // Skip if unchanged
    const prev = stateRef.current;
    if (Math.abs(prev.min - min) <= 1 && Math.abs(prev.max - max) <= 1) return;

    stateRef.current = { min, max };

    // 1. Immediate visual update — write CSS vars on next animation frame
    cancelAnimationFrame(rafRef.current);
    rafRef.current = requestAnimationFrame(() => {
      if (containerRef.current) writeCssVars(containerRef.current, min, max - min);
    });

    // 2. Debounced React state — triggers histogram, ticks, and other heavy computations
    if (debounceRef.current) clearTimeout(debounceRef.current);
    debounceRef.current = setTimeout(() => {
      setCommittedRange({ min, max });
    }, 150);
  }, [minWindowMs, maxWindowMs, maxAllowedTime]);

  // Initialize CSS vars on mount
  useEffect(() => {
    if (containerRef.current) {
      writeCssVars(containerRef.current, initialRange.min, initialRange.max - initialRange.min);
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  useEffect(() => {
    const container = containerRef.current;
    if (!container) return;

    const handleWheel = (e: WheelEvent) => {
      e.preventDefault();

      const { min, max } = stateRef.current;
      const windowMs = max - min;
      const rect = container.getBoundingClientRect();
      const mouseX = e.clientX - rect.left;
      const anchorRatio = Math.max(0, Math.min(1, mouseX / rect.width));

      if (e.ctrlKey || e.metaKey) {
        // Pinch-to-zoom (trackpad sends ctrlKey + deltaY)
        const zoomSensitivity = 0.005;
        const zoomFactor = Math.exp(e.deltaY * zoomSensitivity);
        const newWindowMs = windowMs * zoomFactor;
        const anchorTime = min + windowMs * anchorRatio;
        updateRange(
          anchorTime - newWindowMs * anchorRatio,
          anchorTime + newWindowMs * (1 - anchorRatio),
        );
      } else {
        // Two-finger scroll (horizontal pan)
        const delta = Math.abs(e.deltaX) > Math.abs(e.deltaY) ? e.deltaX : e.deltaY;
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

      const rect = container.getBoundingClientRect();
      const mouseX = ge.clientX - rect.left;
      const anchorRatio = Math.max(0, Math.min(1, mouseX / rect.width));

      const origWindow = gestureStartRange.max - gestureStartRange.min;
      const newWindowMs = origWindow / ge.scale;
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

  /** Animate CSS variables from current to target over durationMs using RAF interpolation. */
  const animateToRange = useCallback((target: TimeRange, durationMs = 300) => {
    cancelAnimationFrame(animRafRef.current);
    const from = { ...stateRef.current };
    const startTime = performance.now();

    const tick = (now: number) => {
      const elapsed = now - startTime;
      const t = Math.min(elapsed / durationMs, 1);
      // ease-out cubic: 1 - (1-t)^3
      const ease = 1 - Math.pow(1 - t, 3);

      const min = from.min + (target.min - from.min) * ease;
      const max = from.max + (target.max - from.max) * ease;

      stateRef.current = { min, max };
      if (containerRef.current) writeCssVars(containerRef.current, min, max - min);

      if (t < 1) {
        animRafRef.current = requestAnimationFrame(tick);
      } else {
        // Animation complete — commit final state to React
        stateRef.current = target;
        if (containerRef.current) writeCssVars(containerRef.current, target.min, target.max - target.min);
        setCommittedRange(target);
      }
    };

    animRafRef.current = requestAnimationFrame(tick);
  }, []);

  return {
    containerRef,
    /** Debounced range — use for React-rendered elements (histogram, ticks). */
    timeRange: committedRange,
    /** Force-set range (e.g. window button click) — updates both CSS and React immediately. */
    setTimeRange: (range: TimeRange) => {
      cancelAnimationFrame(animRafRef.current); // Cancel any running animation
      stateRef.current = range;
      if (containerRef.current) writeCssVars(containerRef.current, range.min, range.max - range.min);
      setCommittedRange(range);
    },
    /** Animated range transition — smooth CSS variable interpolation via RAF. */
    animateToRange,
  };
}
