import { useEffect, useState } from "react";
import { listDockerSshContexts, testRemoteHost } from "../api";
import type { HostStatus, RemoteHost, SshContext } from "../types";
import { DockerContextsList } from "./DockerContextsList";

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
      setTestError(e instanceof Error ? e.message : String(e));
    } finally {
      setTesting(false);
    }
  };

  return (
    <section className="settings-section">
      <h3>Remote hosts</h3>
      {isWindows ? (
        <p className="muted">Remote hosts are not supported on Windows.</p>
      ) : (
        <>
          <p className="muted">
            Uses your own <code>ssh</code> and <code>~/.ssh/config</code>. Key or agent
            authentication only; run <code>ssh &lt;alias&gt;</code> once first to accept the host
            key.
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
                  <span className="muted"> {status?.state ?? "not connected"}</span>
                  <button type="button" disabled={testing} onClick={() => void test(h.alias)}>
                    Test connection
                  </button>
                  <button
                    type="button"
                    onClick={() => onChange(hosts.filter((x) => x.alias !== h.alias))}
                  >
                    Remove
                  </button>
                </li>
              );
            })}
          </ul>
          <div className="remote-host-add">
            <input
              placeholder="SSH alias, e.g. devbox"
              aria-label="SSH alias"
              value={draft}
              onChange={(e) => setDraft(e.target.value)}
            />
            <button type="button" disabled={!validAlias(draft.trim())} onClick={add}>
              Add
            </button>
            <button
              type="button"
              disabled={testing || !validAlias(draft.trim())}
              onClick={() => void test(draft.trim())}
            >
              Test connection
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
                : `${result.state} (${result.runtimes.join(", ") || "no runtimes"})`}
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
