import { useState, useEffect, useRef, useCallback } from 'react';

export interface TimeRange {
  min: number;
  max: number;
}

type PullState = 'IDLE' | 'PULLING' | 'TRIGGERED' | 'REFRESHING' | 'DECAYING';

interface UseTimelineGesturesOptions {
  initialRange: TimeRange;
  /** Minimum visible window in ms (max zoom in). Default: 30s */
  minWindowMs?: number;
  /** Maximum visible window in ms (max zoom out). Default: 7 days */
  maxWindowMs?: number;
  /** Right boundary — cannot pan beyond this. Default: Date.now() + 5min buffer */
  maxAllowedTime?: number;
  /** Called when pull-to-refresh triggers at the right boundary */
  onRefresh?: () => Promise<void> | void;
  /** Overscroll distance (px) to trigger refresh. Default: 80 */
  pullThreshold?: number;
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

/** Max overscroll distance in px */
const MAX_OVERSCROLL = 150;
/** Damping constant for exponential rubber-band: higher = easier to pull */
const DAMPING_CONSTANT = 200;
/** Overscroll offset (px) held visible during REFRESHING state */
const REFRESHING_OFFSET = 44;

export function useTimelineGestures({
  initialRange,
  minWindowMs = 30 * 1000,
  maxWindowMs = 7 * 24 * 3600 * 1000,
  maxAllowedTime = Date.now() + 5 * 60 * 1000,
  onRefresh,
  pullThreshold = 80,
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

  // ── Pull-to-refresh state ──
  const overscrollRawRef = useRef(0);   // Accumulated raw delta (before damping)
  const overscrollPxRef = useRef(0);    // Damped px value written to CSS
  const pullStateRef = useRef<PullState>('IDLE');
  const wheelTimeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const decayRafRef = useRef(0);
  const [isRefreshing, setIsRefreshing] = useState(false);

  // Keep refs fresh for use inside event handlers
  const onRefreshRef = useRef(onRefresh);
  onRefreshRef.current = onRefresh;
  const maxAllowedTimeRef = useRef(maxAllowedTime);
  maxAllowedTimeRef.current = maxAllowedTime;
  const pullThresholdRef = useRef(pullThreshold);
  pullThresholdRef.current = pullThreshold;

  // ── Overscroll helpers (pure DOM, no React render) ──
  const writeOverscrollCss = useCallback((px: number) => {
    overscrollPxRef.current = px;
    if (containerRef.current) {
      containerRef.current.style.setProperty('--overscroll-x', `${-px}px`);
      // Unitless 0–1 progress for CSS-driven rotation (px/deg conversion impossible in calc)
      containerRef.current.style.setProperty('--overscroll-progress', String(Math.min(px / MAX_OVERSCROLL, 1)));
    }
  }, []);

  /** Apply exponential damping: overscroll = MAX * (1 - e^(-raw / CONSTANT)) */
  const dampedOverscroll = useCallback((raw: number) => {
    return MAX_OVERSCROLL * (1 - Math.exp(-raw / DAMPING_CONSTANT));
  }, []);

  /** Animate overscroll from current value to target using rAF */
  const decayTo = useCallback((target: number, onDone?: () => void) => {
    cancelAnimationFrame(decayRafRef.current);
    const tick = () => {
      const current = overscrollPxRef.current;
      const diff = current - target;
      if (Math.abs(diff) < 0.5) {
        writeOverscrollCss(target);
        overscrollRawRef.current = 0;
        onDone?.();
        return;
      }
      // Exponential ease toward target
      writeOverscrollCss(current - diff * 0.18);
      decayRafRef.current = requestAnimationFrame(tick);
    };
    decayRafRef.current = requestAnimationFrame(tick);
  }, [writeOverscrollCss]);

  /** Reset overscroll to IDLE */
  const resetOverscroll = useCallback(() => {
    cancelAnimationFrame(decayRafRef.current);
    if (wheelTimeoutRef.current) clearTimeout(wheelTimeoutRef.current);
    overscrollRawRef.current = 0;
    pullStateRef.current = 'IDLE';
    writeOverscrollCss(0);
  }, [writeOverscrollCss]);

  /** Called when wheel events stop (timeout fires) */
  const handleScrollEnd = useCallback(() => {
    const state = pullStateRef.current;
    if (state === 'TRIGGERED') {
      // Threshold was met — trigger refresh
      pullStateRef.current = 'REFRESHING';
      setIsRefreshing(true);

      // Decay to REFRESHING_OFFSET (keep spinner visible)
      decayTo(REFRESHING_OFFSET, () => {
        // Call onRefresh, then snap range to now and decay fully
        const result = onRefreshRef.current?.();
        Promise.resolve(result).finally(() => {
          // Snap range to now
          const { min, max } = stateRef.current;
          const windowMs = max - min;
          const n = Date.now();
          const newRange = { min: n - windowMs, max: n };
          stateRef.current = newRange;
          if (containerRef.current) writeCssVars(containerRef.current, newRange.min, newRange.max - newRange.min);
          setCommittedRange(newRange);

          // Decay overscroll to 0
          pullStateRef.current = 'DECAYING';
          setIsRefreshing(false);
          decayTo(0, () => {
            pullStateRef.current = 'IDLE';
          });
        });
      });
    } else if (state === 'PULLING') {
      // Below threshold — just bounce back
      pullStateRef.current = 'DECAYING';
      decayTo(0, () => {
        pullStateRef.current = 'IDLE';
      });
    }
  }, [decayTo]);

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
      containerRef.current.style.setProperty('--overscroll-x', '0px');
      containerRef.current.style.setProperty('--overscroll-progress', '0');
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
        // Force reset any overscroll during zoom
        if (pullStateRef.current !== 'IDLE' && pullStateRef.current !== 'REFRESHING') {
          resetOverscroll();
        }
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

        // Overscroll triggers near "now", not at the far-future maxAllowedTime buffer
        const overscrollEdge = Date.now() + 5_000; // 5s grace for in-flight events
        const isAtRightEdge = max >= overscrollEdge;
        const state = pullStateRef.current;

        // ── Pull-to-refresh logic ──
        if (state === 'REFRESHING') {
          // During refresh: allow visual overscroll but don't re-trigger
          if (delta > 0) {
            overscrollRawRef.current += Math.abs(delta);
            writeOverscrollCss(dampedOverscroll(overscrollRawRef.current));
          } else if (delta < 0) {
            overscrollRawRef.current = Math.max(0, overscrollRawRef.current - Math.abs(delta) * 1.5);
            writeOverscrollCss(Math.max(REFRESHING_OFFSET, dampedOverscroll(overscrollRawRef.current)));
          }
          return;
        }

        if (overscrollPxRef.current > 0 || (isAtRightEdge && delta > 0)) {
          // Cancel any running decay
          cancelAnimationFrame(decayRafRef.current);

          if (delta > 0) {
            // Pulling further right — accumulate raw overscroll
            overscrollRawRef.current += Math.abs(delta);
          } else {
            // Pulling back left — reduce raw overscroll
            overscrollRawRef.current = Math.max(0, overscrollRawRef.current - Math.abs(delta) * 1.5);
          }

          const dampedPx = dampedOverscroll(overscrollRawRef.current);
          writeOverscrollCss(dampedPx);

          // State transitions
          if (dampedPx <= 0) {
            // Pulled all the way back — exit overscroll
            resetOverscroll();
            return;
          } else if (dampedPx >= pullThresholdRef.current) {
            pullStateRef.current = 'TRIGGERED';
          } else {
            pullStateRef.current = 'PULLING';
          }

          // Schedule scroll-end detection
          if (wheelTimeoutRef.current) clearTimeout(wheelTimeoutRef.current);
          wheelTimeoutRef.current = setTimeout(handleScrollEnd, 150);
          return;
        }

        // ── Normal pan ──
        updateRange(min + timeShift, max + timeShift);
      }
    };

    // Safari gesturestart/gesturechange for non-standard pinch
    let gestureStartRange: TimeRange | null = null;

    const handleGestureStart = (e: Event) => {
      e.preventDefault();
      gestureStartRange = { ...stateRef.current };
      // Reset overscroll on pinch start
      if (pullStateRef.current !== 'IDLE' && pullStateRef.current !== 'REFRESHING') {
        resetOverscroll();
      }
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
      if (wheelTimeoutRef.current) clearTimeout(wheelTimeoutRef.current);
      cancelAnimationFrame(decayRafRef.current);
    };
  }, [updateRange, resetOverscroll, handleScrollEnd, dampedOverscroll, writeOverscrollCss]);

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
    /** Whether a pull-to-refresh is currently in progress */
    isRefreshing,
  };
}
