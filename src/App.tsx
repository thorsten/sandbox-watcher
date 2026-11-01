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
  onSelectContainer,
  onSettingsUpdated,
  onUpdateAvailable,
  onUpdateProgress,
  refreshNow,
} from "./api";
import logoUrl from "./assets/logo.svg";
import { DetailPane } from "./components/DetailPane";
import { HistoryView } from "./components/HistoryView";
import { RuntimeBanner } from "./components/RuntimeBanner";
import { SettingsPanel } from "./components/SettingsPanel";
import { Sidebar } from "./components/Sidebar";
import { StopAllButton } from "./components/StopAllButton";
import { UpdateBanner } from "./components/UpdateBanner";
import { AGENT_LABELS, summarize } from "./format";
import { filterByHost, hostsInUse, pickSelected } from "./hosts";
import { recentBlockedCount } from "./network";
import { applyTheme } from "./theme";
import type { NetworkReport, Overview, ContainerOverview, Settings, UpdateInfo } from "./types";

const EMPTY: ContainerOverview[] = [];

type LoadState =
  | { phase: "loading" }
  | { phase: "ready"; overview: Overview; refreshedAt: Date }
  | { phase: "error"; message: string };

export function App() {
  const [state, setState] = useState<LoadState>({ phase: "loading" });
  const [network, setNetwork] = useState<NetworkReport[] | undefined>(undefined);
  const [settings, setSettings] = useState<Settings | null>(null);
  const [showSettings, setShowSettings] = useState(false);
  const [focusDiagnostics, setFocusDiagnostics] = useState(false);
  const [showHistory, setShowHistory] = useState(false);
  const [update, setUpdate] = useState<UpdateInfo | null>(null);
  const [dismissedVersion, setDismissedVersion] = useState<string | null>(null);
  const [updateProgress, setUpdateProgress] = useState<{
    downloaded: number;
    total: number | null;
  } | null>(null);
  const [version, setVersion] = useState("");
  const [refreshing, setRefreshing] = useState(false);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [hostFilter, setHostFilter] = useState("all");

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
      onUpdateAvailable((u) => !cancelled && setUpdate(u)),
      onUpdateProgress((p) => !cancelled && setUpdateProgress(p)),
      onSelectContainer((id) => {
        if (cancelled) return;
        setSelectedId(id);
        // The tray may pick a container the host filter hides.
        setHostFilter("all");
        setShowSettings(false);
        setShowHistory(false);
      }),
    ];
    return () => {
      cancelled = true;
      for (const sub of subscriptions) sub.then((unlisten) => unlisten()).catch(() => undefined);
    };
  }, [acceptSettings, acceptOverview]);

  const containers = state.phase === "ready" ? state.overview.containers : EMPTY;

  const visible = filterByHost(containers, hostFilter);
  // Keep the selection on the same container across refreshes while the
  // host filter shows it; otherwise pick a running one of a reachable host.
  const selected = pickSelected(visible, selectedId);

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
            running={containers.filter((c) => c.state === "running" && !c.last_seen_at)}
            hosts={state.phase === "ready" ? state.overview.hosts : []}
            onChanged={() => void refresh()}
          />
          <button type="button" onClick={() => void refresh()} disabled={refreshing}>
            {refreshing ? "Refreshing…" : "Refresh"}
          </button>
          <button
            type="button"
            onClick={() => {
              setShowHistory((v) => !v);
              setShowSettings(false);
            }}
            aria-pressed={showHistory}
            title="Session history"
          >
            History
          </button>
          <button
            type="button"
            onClick={() => {
              setShowSettings((v) => !v);
              setShowHistory(false);
            }}
            aria-pressed={showSettings}
            title="Settings"
          >
            Settings
          </button>
          {version && <span className="muted small">v{version}</span>}
        </div>
      </header>

      {update && update.version !== dismissedVersion && (
        <UpdateBanner
          update={update}
          progress={updateProgress}
          onDismiss={() => setDismissedVersion(update.version)}
        />
      )}

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
            containers={visible}
            runtime={state.overview.runtime}
            hosts={state.overview.hosts}
            hostChoices={hostsInUse(containers)}
            hostFilter={hostFilter}
            onHostFilter={setHostFilter}
            network={network}
            selectedId={selected?.key ?? null}
            onSelect={setSelectedId}
            now={state.refreshedAt}
          />
          <main className="main">
            {showHistory ? (
              <HistoryView onClose={() => setShowHistory(false)} />
            ) : showSettings && settings ? (
              <SettingsPanel
                key={JSON.stringify(settings)}
                settings={settings}
                hostStatuses={state.overview.hosts}
                focusDiagnostics={focusDiagnostics}
                onSaved={acceptSettings}
                onClose={() => {
                  setShowSettings(false);
                  setFocusDiagnostics(false);
                }}
              />
            ) : state.overview.runtime.status !== "connected" ? (
              <RuntimeBanner
                runtime={state.overview.runtime}
                hosts={state.overview.hosts}
                onDiagnostics={() => {
                  setFocusDiagnostics(true);
                  setShowSettings(true);
                }}
              />
            ) : selected ? (
              <DetailPane
                key={selected.key}
                container={selected}
                runtime={state.overview.runtime}
                report={network?.find((r) => r.container_id === selected.key)}
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
