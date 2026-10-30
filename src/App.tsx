import { useCallback, useEffect, useState } from "react";
import { applyConfirmStop } from "./actions";
import {
  appVersion,
  getNetworkOverview,
  getOverview,
  getSettings,
  onNetworkUpdated,
  onOpenSettings,
  onOverviewUpdated,
  onSettingsUpdated,
  refreshNow,
} from "./api";
import logoUrl from "./assets/logo.svg";
import { ContainerList } from "./components/ContainerList";
import { RuntimeBanner } from "./components/RuntimeBanner";
import { SettingsPanel } from "./components/SettingsPanel";
import { applyTheme } from "./theme";
import type { NetworkReport, Overview, Settings } from "./types";

type LoadState =
  | { phase: "loading" }
  | { phase: "ready"; overview: Overview; refreshedAt: Date }
  | { phase: "error"; message: string };

export function App() {
  const [state, setState] = useState<LoadState>({ phase: "loading" });
  const [network, setNetwork] = useState<NetworkReport[] | undefined>(undefined);
  const [settings, setSettings] = useState<Settings | null>(null);
  const [showSettings, setShowSettings] = useState(false);
  const [version, setVersion] = useState("");
  const [refreshing, setRefreshing] = useState(false);

  const acceptSettings = useCallback((s: Settings) => {
    setSettings(s);
    applyConfirmStop(s.confirm_stop);
    applyTheme(s.theme);
  }, []);

  const acceptOverview = useCallback((overview: Overview) => {
    setState({ phase: "ready", overview, refreshedAt: new Date() });
  }, []);

  // The Rust watcher refreshes on its own; the UI only asks for the current
  // state once and then listens for pushes.
  useEffect(() => {
    let cancelled = false;
    const fail = (e: unknown) => {
      if (!cancelled)
        setState({ phase: "error", message: e instanceof Error ? e.message : String(e) });
    };
    getSettings().then((s) => !cancelled && acceptSettings(s), fail);
    appVersion().then(
      (v) => !cancelled && setVersion(v),
      () => undefined,
    );
    getOverview().then((o) => !cancelled && acceptOverview(o), fail);
    getNetworkOverview().then(
      (n) => !cancelled && setNetwork(n),
      () => undefined,
    );

    const subscriptions = [
      onOverviewUpdated((o) => !cancelled && acceptOverview(o)),
      onNetworkUpdated((n) => !cancelled && setNetwork(n)),
      onSettingsUpdated((s) => !cancelled && acceptSettings(s)),
      onOpenSettings(() => !cancelled && setShowSettings(true)),
    ];
    return () => {
      cancelled = true;
      for (const sub of subscriptions) sub.then((unlisten) => unlisten()).catch(() => undefined);
    };
  }, [acceptSettings, acceptOverview]);

  const refresh = async () => {
    setRefreshing(true);
    try {
      acceptOverview(await refreshNow());
    } catch (e) {
      setState({ phase: "error", message: e instanceof Error ? e.message : String(e) });
    } finally {
      setRefreshing(false);
    }
  };

  return (
    <main className="app">
      <header className="app-header">
        <h1>
          <img className="logo" src={logoUrl} alt="" width={28} height={28} />
          Sandbox Watcher
        </h1>
        <div className="header-meta">
          {state.phase === "ready" && state.overview.source === "mock" && (
            <span className="badge badge-mock" title="SANDBOX_WATCHER_MOCK=1">
              mock data
            </span>
          )}
          {version && <span className="muted">v{version}</span>}
          <button type="button" onClick={() => void refresh()} disabled={refreshing}>
            {refreshing ? "Refreshing…" : "Refresh"}
          </button>
          <button
            type="button"
            onClick={() => setShowSettings((v) => !v)}
            aria-pressed={showSettings}
            title="Settings"
          >
            Settings
          </button>
        </div>
      </header>

      {showSettings && settings && (
        <SettingsPanel
          key={JSON.stringify(settings)}
          settings={settings}
          onSaved={acceptSettings}
          onClose={() => setShowSettings(false)}
        />
      )}

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
            runtime={state.overview.runtime}
            network={network}
            now={state.refreshedAt}
            onChanged={() => void refresh()}
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
