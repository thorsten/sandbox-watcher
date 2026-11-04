import { useEffect, useState } from "react";
import { listDockerSshContexts, testRemoteHost } from "../api";
import { t, tx } from "../i18n";
import type { HostStatus, RemoteHost, SshContext } from "../types";
import { DockerContextsList } from "./DockerContextsList";
import { describeError } from "../errors";

interface Props {
  hosts: RemoteHost[];
  statuses: HostStatus[];
  onChange: (hosts: RemoteHost[]) => void;
}

const ALIAS = /^[A-Za-z0-9._@-]+$/;

export function validAlias(alias: string): boolean {
  return ALIAS.test(alias) && !alias.startsWith("-") && alias !== "local" && alias.length <= 255;
}

export function RemoteHostsSection({ hosts, statuses, onChange }: Props) {
  const [draft, setDraft] = useState("");
  const [result, setResult] = useState<HostStatus | null>(null);
  const [testError, setTestError] = useState<string | null>(null);
  const [testing, setTesting] = useState(false);
  const isWindows = navigator.userAgent.includes("Windows");
  const [contexts, setContexts] = useState<SshContext[]>([]);

  useEffect(() => {
    if (isWindows) return;
    listDockerSshContexts()
      .then(setContexts)
      .catch(() => setContexts([]));
  }, [isWindows]);

  const add = () => {
    const alias = draft.trim();
    if (!validAlias(alias) || hosts.some((h) => h.alias === alias)) return;
    onChange([...hosts, { alias, display_name: null, enabled: true }]);
    setDraft("");
  };

  const test = async (alias: string) => {
    setTesting(true);
    setTestError(null);
    try {
      setResult(await testRemoteHost(alias));
    } catch (e) {
      setResult(null);
      setTestError(describeError(e));
    } finally {
      setTesting(false);
    }
  };

  return (
    <section className="settings-section">
      <h3>{t("remote.title")}</h3>
      {isWindows ? (
        <p className="muted">{t("remote.windows")}</p>
      ) : (
        <>
          <p className="muted">
            {tx("remote.intro", {
              ssh: <code>ssh</code>,
              config: <code>~/.ssh/config</code>,
              command: <code>ssh &lt;alias&gt;</code>,
            })}
          </p>
          <ul className="remote-hosts">
            {hosts.map((h) => {
              const status = statuses.find((s) => s.alias === h.alias);
              return (
                <li key={h.alias}>
                  <label className="check">
                    <input
                      type="checkbox"
                      checked={h.enabled}
                      onChange={(e) =>
                        onChange(
                          hosts.map((x) =>
                            x.alias === h.alias ? { ...x, enabled: e.target.checked } : x,
                          ),
                        )
                      }
                    />{" "}
                    {h.display_name || h.alias}
                  </label>
                  <span className="muted">
                    {" "}
                    {status ? t(`hoststate.${status.state}`) : t("remote.notConnected")}
                  </span>
                  <button type="button" disabled={testing} onClick={() => void test(h.alias)}>
                    {t("remote.test")}
                  </button>
                  <button
                    type="button"
                    onClick={() => onChange(hosts.filter((x) => x.alias !== h.alias))}
                  >
                    {t("remote.remove")}
                  </button>
                </li>
              );
            })}
          </ul>
          <div className="remote-host-add">
            <input
              placeholder={t("remote.aliasPlaceholder")}
              aria-label={t("remote.aliasLabel")}
              value={draft}
              onChange={(e) => setDraft(e.target.value)}
            />
            <button type="button" disabled={!validAlias(draft.trim())} onClick={add}>
              {t("remote.add")}
            </button>
            <button
              type="button"
              disabled={testing || !validAlias(draft.trim())}
              onClick={() => void test(draft.trim())}
            >
              {t("remote.test")}
            </button>
          </div>
          <DockerContextsList
            contexts={contexts}
            hosts={hosts}
            onAdd={(added) => onChange([...hosts, ...added])}
          />
          {testError && <div className="notice notice-err">{testError}</div>}
          {result && (
            <div
              className={result.state === "connected" ? "notice notice-ok" : "notice notice-err"}
            >
              <strong>{result.alias}</strong>:{" "}
              {result.state === "unreachable"
                ? result.reason
                : t("remote.stateWith", {
                    state: t(`hoststate.${result.state}`),
                    runtimes: result.runtimes.join(", ") || t("remote.noRuntimes"),
                  })}
              <ul>
                {result.probes.map((p) => (
                  <li key={p}>{p}</li>
                ))}
              </ul>
            </div>
          )}
        </>
      )}
    </section>
  );
}
