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
  resolvedLanguage,
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
import { LanguageProvider, formatTime, setLanguage as applyToHelpers, t, tx } from "./i18n";
import { applyLanguage } from "./language";
import { recentBlockedCount } from "./network";
import { applyTheme } from "./theme";
import type {
  Language,
  NetworkReport,
  Overview,
  ContainerOverview,
  Settings,
  UpdateInfo,
} from "./types";

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
  const [language, setLanguage] = useState<Language>("en");

  const acceptSettings = useCallback((s: Settings) => {
    setSettings(s);
    applyConfirmStop(s.confirm_stop);
    applyTheme(s.theme);
    // What `system` means is only known on the Rust side.
    resolvedLanguage().then(
      (l) => {
        applyLanguage(l);
        setLanguage(l);
      },
      () => undefined,
    );
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

  // App's own texts render before the provider below sets the language.
  applyToHelpers(language);
  const summary = summarize(containers);
  const recentBlocked =
    network && state.phase === "ready" ? recentBlockedCount(network, state.refreshedAt) : 0;

  return (
    <LanguageProvider language={language}>
      <div className="app">
        <header className="app-header">
          <h1>
            <img className="logo" src={logoUrl} alt="" width={26} height={26} />
            Sandbox Watcher
          </h1>
          {state.phase === "ready" && state.overview.runtime.status === "connected" && (
            <div className="header-stats" aria-label={t("app.summary")}>
              <Stat label={t("app.stat.projects")} value={summary.projects} />
              <Stat label={t("app.stat.running")} value={summary.running} />
              <Stat label={t("app.stat.sessions")} value={summary.sessions} />
              {[...summary.byAgent].map(([agent, n]) => (
                <span key={agent} className={`chip agent-${agent}`}>
                  {AGENT_LABELS[agent]} <strong>{n}</strong>
                </span>
              ))}
              {recentBlocked > 0 && (
                <span className="badge badge-err" title={t("app.blockedRecentTitle")}>
                  {t("app.blockedCount", { n: recentBlocked })}
                </span>
              )}
            </div>
          )}
          <div className="header-meta">
            {state.phase === "ready" && state.overview.source === "mock" && (
              <span className="badge badge-mock" title="SANDBOX_WATCHER_MOCK=1">
                {t("app.mockData")}
              </span>
            )}
            <StopAllButton
              running={containers.filter((c) => c.state === "running" && !c.last_seen_at)}
              hosts={state.phase === "ready" ? state.overview.hosts : []}
              onChanged={() => void refresh()}
            />
            <button type="button" onClick={() => void refresh()} disabled={refreshing}>
              {refreshing ? t("app.refreshing") : t("app.refresh")}
            </button>
            <button
              type="button"
              onClick={() => {
                setShowHistory((v) => !v);
                setShowSettings(false);
              }}
              aria-pressed={showHistory}
              title={t("app.historyTitle")}
            >
              {t("app.history")}
            </button>
            <button
              type="button"
              onClick={() => {
                setShowSettings((v) => !v);
                setShowHistory(false);
              }}
              aria-pressed={showSettings}
              title={t("app.settings")}
            >
              {t("app.settings")}
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

        {state.phase === "loading" && <p className="muted pad">{t("app.looking")}</p>}

        {state.phase === "error" && (
          <section className="banner banner-error pad">
            <strong>{t("app.loadFailed")}</strong>
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
                  initialSection={focusDiagnostics ? "diagnostics" : undefined}
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
                  <h2>{t("app.emptyTitle")}</h2>
                  <p className="muted">
                    {tx("app.emptyHint", { command: <code>sandbox run claude</code> })}
                  </p>
                </section>
              )}
              <footer className="muted small">
                {t("app.footer", {
                  version: state.overview.tested_sandbox_version,
                  time: formatTime(state.refreshedAt),
                })}
              </footer>
            </main>
          </div>
        )}
      </div>
    </LanguageProvider>
  );
}

function Stat({ label, value }: { label: string; value: number }) {
  return (
    <span className="header-stat">
      <strong>{value}</strong> <span className="muted">{label}</span>
    </span>
  );
}
