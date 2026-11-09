import { useCallback, useEffect, useState } from "react";
import { describeError } from "./errors";

export interface Polled<T> {
  /** Last successful result for the current `load`, or null before the first one arrived. */
  data: T | null;
  /** Message of the most recent failed request; cleared by the next success. */
  error: string | null;
  /**
   * Request now instead of waiting for the timer. The pending request's answer is dropped,
   * `data` stays on screen until the new one arrives, and the interval restarts from now.
   */
  refresh: () => void;
}

interface Slot<T> {
  load: () => Promise<T>;
  data: T | null;
  error: string | null;
}

function documentHidden(): boolean {
  return typeof document !== "undefined" && document.visibilityState === "hidden";
}

/**
 * Poll `load` with at most one request in flight.
 *
 * - The first request starts when `load` changes; the next is scheduled `intervalMs` after the
 *   previous one settled (success or failure), so slow requests never overlap and a failing
 *   backend is retried at the same fixed rate instead of piling up requests.
 * - `intervalMs === null` loads once and does not repeat.
 * - While the document is hidden (window closed to the tray, minimised) nothing is scheduled;
 *   every poll is a command inside a container. The first request after it becomes visible
 *   again runs at once.
 * - A response that arrives after `load` or `intervalMs` changed, or after unmount, is dropped.
 * - When `load` changes, `data` and `error` start from null again; while `load` is stable
 *   (for example only `intervalMs` changed) the last result stays visible.
 * - On failure the last `data` is kept and `error` is set; a later success clears `error`.
 * - `refresh()` starts a request at once (for example after an action changed the data) and
 *   keeps the current `data` meanwhile.
 *
 * Pass a memoised `load` (`useCallback`) so that only real changes restart polling.
 */
export function usePolling<T>(load: () => Promise<T>, intervalMs: number | null): Polled<T> {
  const [slot, setSlot] = useState<Slot<T>>(() => ({ load, data: null, error: null }));
  const [generation, setGeneration] = useState(0);
  const refresh = useCallback(() => setGeneration((g) => g + 1), []);

  useEffect(() => {
    let active = true;
    let timer: number | undefined;
    let inFlight = false;
    // A poll was due while the document was hidden; run it when it becomes visible.
    let paused = false;

    const schedule = () => {
      if (intervalMs === null) return;
      if (documentHidden()) {
        paused = true;
        return;
      }
      timer = window.setTimeout(() => void run(), intervalMs);
    };

    const run = async () => {
      inFlight = true;
      let next: Partial<Slot<T>>;
      try {
        next = { data: await load(), error: null };
      } catch (e) {
        next = { error: describeError(e) };
      }
      inFlight = false;
      if (!active) return;
      setSlot((prev) => {
        const base = prev.load === load ? prev : { load, data: null, error: null };
        return { ...base, ...next };
      });
      schedule();
    };

    const onVisibility = () => {
      if (intervalMs === null) return;
      if (documentHidden()) {
        window.clearTimeout(timer);
        paused = true;
      } else if (paused) {
        paused = false;
        // A request still running schedules the next one itself when it settles.
        if (!inFlight) void run();
      }
    };

    document.addEventListener("visibilitychange", onVisibility);
    void run();
    return () => {
      active = false;
      window.clearTimeout(timer);
      document.removeEventListener("visibilitychange", onVisibility);
    };
    // `generation` is not read inside: bumping it on refresh() restarts the cycle.
    // oxlint-disable-next-line react/exhaustive-effect-dependencies
  }, [load, intervalMs, generation]);

  const current = slot.load === load ? slot : { data: null, error: null };
  return { data: current.data, error: current.error, refresh };
}
