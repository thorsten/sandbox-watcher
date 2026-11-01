import { useEffect, useState } from "react";

export interface Polled<T> {
  /** Last successful result for the current `load`, or null before the first one arrived. */
  data: T | null;
  /** Message of the most recent failed request; cleared by the next success. */
  error: string | null;
}

interface Slot<T> extends Polled<T> {
  load: () => Promise<T>;
}

/**
 * Poll `load` with at most one request in flight.
 *
 * - The first request starts when `load` changes; the next is scheduled `intervalMs` after the
 *   previous one settled (success or failure), so slow requests never overlap and a failing
 *   backend is retried at the same fixed rate instead of piling up requests.
 * - `intervalMs === null` loads once and does not repeat.
 * - A response that arrives after `load` or `intervalMs` changed, or after unmount, is dropped.
 * - When `load` changes, `data` and `error` start from null again; while `load` is stable
 *   (for example only `intervalMs` changed) the last result stays visible.
 * - On failure the last `data` is kept and `error` is set; a later success clears `error`.
 *
 * Pass a memoised `load` (`useCallback`) so that only real changes restart polling.
 */
export function usePolling<T>(load: () => Promise<T>, intervalMs: number | null): Polled<T> {
  const [slot, setSlot] = useState<Slot<T>>(() => ({ load, data: null, error: null }));

  useEffect(() => {
    let active = true;
    let timer: number | undefined;

    const run = async () => {
      let next: Partial<Polled<T>>;
      try {
        next = { data: await load(), error: null };
      } catch (e) {
        next = { error: e instanceof Error ? e.message : String(e) };
      }
      if (!active) return;
      setSlot((prev) => {
        const base = prev.load === load ? prev : { load, data: null, error: null };
        return { ...base, ...next };
      });
      if (intervalMs !== null) timer = window.setTimeout(() => void run(), intervalMs);
    };

    void run();
    return () => {
      active = false;
      window.clearTimeout(timer);
    };
  }, [load, intervalMs]);

  return slot.load === load ? slot : { data: null, error: null };
}
