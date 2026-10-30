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
import { DetailPane } from "./components/DetailPane";
import { RuntimeBanner } from "./components/RuntimeBanner";
import { SettingsPanel } from "./components/SettingsPanel";
import { Sidebar } from "./components/Sidebar";
import { StopAllButton } from "./components/StopAllButton";
import { AGENT_LABELS, summarize } from "./format";
import { recentBlockedCount } from "./network";
import { applyTheme } from "./theme";
import type { NetworkReport, Overview, SandboxContainer, Settings } from "./types";

const EMPTY: SandboxContainer[] = [];

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
  const [selectedId, setSelectedId] = useState<string | null>(null);

  const acceptSettings = useCallback((s: Settings) => {
    setSettings(s);
    applyConfirmStop(s.confirm_stop);
    applyTheme(s.theme);
  }, []);

  const acceptOverview = useCallback((overview: Overview) => {
    setState({ phase: "ready", overview, refreshedAt: new Date() });
  }, []);

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

  const containers = state.phase === "ready" ? state.overview.containers : EMPTY;

  // Keep the selection on the same container across refreshes; otherwise
  // pick the first running one.
  const selected =
    containers.find((c) => c.id === selectedId) ??
    containers.find((c) => c.state === "running") ??
    containers[0] ??
    null;

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

  const summary = summarize(containers);
  const recentBlocked =
    network && state.phase === "ready" ? recentBlockedCount(network, state.refreshedAt) : 0;

  return (
    <div className="app">
      <header className="app-header">
        <h1>
          <img className="logo" src={logoUrl} alt="" width={26} height={26} />
          Sandbox Watcher
        </h1>
        {state.phase === "ready" && state.overview.runtime.status === "connected" && (
          <div className="header-stats" aria-label="Summary">
            <Stat label="projects" value={summary.projects} />
            <Stat label="running" value={summary.running} />
            <Stat label="sessions" value={summary.sessions} />
            {[...summary.byAgent].map(([agent, n]) => (
              <span key={agent} className={`chip agent-${agent}`}>
                {AGENT_LABELS[agent]} <strong>{n}</strong>
              </span>
            ))}
            {recentBlocked > 0 && (
              <span className="badge badge-err" title="Blocked requests in the last five minutes">
                {recentBlocked} blocked
              </span>
            )}
          </div>
        )}
        <div className="header-meta">
          {state.phase === "ready" && state.overview.source === "mock" && (
            <span className="badge badge-mock" title="SANDBOX_WATCHER_MOCK=1">
              mock data
            </span>
          )}
          <StopAllButton
            running={containers.filter((c) => c.state === "running")}
            onChanged={() => void refresh()}
          />
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
          {version && <span className="muted small">v{version}</span>}
        </div>
      </header>

      {state.phase === "loading" && <p className="muted pad">Looking for container runtimes…</p>}

      {state.phase === "error" && (
        <section className="banner banner-error pad">
          <strong>Could not load overview.</strong>
          <pre>{state.message}</pre>
        </section>
      )}

      {state.phase === "ready" && (
        <div className="layout">
          <Sidebar
            containers={containers}
            runtime={state.overview.runtime}
            network={network}
            selectedId={selected?.id ?? null}
            onSelect={setSelectedId}
            now={state.refreshedAt}
          />
          <main className="main">
            {showSettings && settings ? (
              <SettingsPanel
                key={JSON.stringify(settings)}
                settings={settings}
                onSaved={acceptSettings}
                onClose={() => setShowSettings(false)}
              />
            ) : state.overview.runtime.status !== "connected" ? (
              <RuntimeBanner runtime={state.overview.runtime} />
            ) : selected ? (
              <DetailPane
                key={selected.id}
                container={selected}
                runtime={state.overview.runtime}
                report={network?.find((r) => r.container_id === selected.id)}
                now={state.refreshedAt}
                onChanged={() => void refresh()}
              />
            ) : (
              <section className="empty">
                <h2>No sandbox containers.</h2>
                <p className="muted">
                  Start one with <code>sandbox run claude</code> in a project and it appears on the
                  left.
                </p>
              </section>
            )}
            <footer className="muted small">
              Tested against sandbox {state.overview.tested_sandbox_version}. Refreshed{" "}
              {state.refreshedAt.toLocaleTimeString()}.
            </footer>
          </main>
        </div>
      )}
    </div>
  );
}

function Stat({ label, value }: { label: string; value: number }) {
  return (
    <span className="header-stat">
      <strong>{value}</strong> <span className="muted">{label}</span>
    </span>
  );
}
