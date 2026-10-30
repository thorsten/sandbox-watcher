import { useCallback, useEffect, useRef, useState } from "react";
import { appVersion, getOverview, onRuntimeEvent } from "./api";
import { ContainerList } from "./components/ContainerList";
import { RuntimeBanner } from "./components/RuntimeBanner";
import type { Overview } from "./types";

/** Fallback poll. Lifecycle events from the runtime trigger refreshes sooner. */
const POLL_MS = 10_000;
/** Collapse bursts of events (start, exec_create, ...) into one refresh. */
const EVENT_DEBOUNCE_MS = 250;

type LoadState =
  | { phase: "loading" }
  | { phase: "ready"; overview: Overview; version: string; refreshedAt: Date }
  | { phase: "error"; message: string };

export function App() {
  const [state, setState] = useState<LoadState>({ phase: "loading" });
  const inFlight = useRef(false);

  const refresh = useCallback(async () => {
    if (inFlight.current) return;
    inFlight.current = true;
    try {
      const [overview, version] = await Promise.all([getOverview(), appVersion().catch(() => "")]);
      setState({ phase: "ready", overview, version, refreshedAt: new Date() });
    } catch (error) {
      setState({ phase: "error", message: error instanceof Error ? error.message : String(error) });
    } finally {
      inFlight.current = false;
    }
  }, []);

  useEffect(() => {
    // refresh() awaits the Tauri command before it sets state, so there is no synchronous
    // setState here; the rule cannot see through the async boundary.
    // oxlint-disable-next-line react/set-state-in-effect
    void refresh();
    const timer = window.setInterval(() => void refresh(), POLL_MS);

    let debounce: number | undefined;
    const unlisten = onRuntimeEvent(() => {
      window.clearTimeout(debounce);
      debounce = window.setTimeout(() => void refresh(), EVENT_DEBOUNCE_MS);
    });

    return () => {
      window.clearInterval(timer);
      window.clearTimeout(debounce);
      unlisten.then((fn) => fn()).catch(() => undefined);
    };
  }, [refresh]);

  return (
    <main className="app">
      <header className="app-header">
        <h1>
          <span className="logo" aria-hidden="true">
            ⬡
          </span>
          Sandbox Watcher
        </h1>
        <div className="header-meta">
          {state.phase === "ready" && state.overview.source === "mock" && (
            <span className="badge badge-mock" title="SANDBOX_WATCHER_MOCK=1">
              mock data
            </span>
          )}
          {state.phase === "ready" && state.version && (
            <span className="muted">v{state.version}</span>
          )}
          <button type="button" onClick={() => void refresh()}>
            Refresh
          </button>
        </div>
      </header>

      {state.phase === "loading" && <p className="muted">Looking for container runtimes…</p>}

      {state.phase === "error" && (
        <section className="banner banner-error">
          <strong>Could not load overview.</strong>
          <pre>{state.message}</pre>
        </section>
      )}

      {state.phase === "ready" && (
        <>
          <RuntimeBanner runtime={state.overview.runtime} />
          <ContainerList
            containers={state.overview.containers}
            runtimeStatus={state.overview.runtime.status}
            now={state.refreshedAt}
          />
          <footer className="muted">
            Tested against sandbox {state.overview.tested_sandbox_version}. Refreshed{" "}
            {state.refreshedAt.toLocaleTimeString()}.
          </footer>
        </>
      )}
    </main>
  );
}
