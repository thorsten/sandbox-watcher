import { useCallback, useEffect, useState } from "react";
import { appVersion, getOverview } from "./api";
import { ContainerList } from "./components/ContainerList";
import { RuntimeBanner } from "./components/RuntimeBanner";
import type { Overview } from "./types";

const REFRESH_MS = 5_000;

type LoadState =
  | { phase: "loading" }
  | { phase: "ready"; overview: Overview; refreshedAt: Date }
  | { phase: "error"; message: string };

export function App() {
  const [state, setState] = useState<LoadState>({ phase: "loading" });
  const [version, setVersion] = useState<string>("");

  const refresh = useCallback(async () => {
    try {
      const overview = await getOverview();
      setState({ phase: "ready", overview, refreshedAt: new Date() });
    } catch (error) {
      setState({ phase: "error", message: error instanceof Error ? error.message : String(error) });
    }
  }, []);

  useEffect(() => {
    appVersion().then(setVersion, () => setVersion(""));
    void refresh();
    const timer = window.setInterval(() => void refresh(), REFRESH_MS);
    return () => window.clearInterval(timer);
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
          {version && <span className="muted">v{version}</span>}
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
