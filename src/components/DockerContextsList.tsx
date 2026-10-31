import { useState } from "react";
import { copyText } from "../actions";
import type { RemoteHost, SshContext } from "../types";

interface Props {
  contexts: SshContext[];
  hosts: RemoteHost[];
  onAdd: (added: RemoteHost[]) => void;
}

/** New hosts for the selected contexts, skipping aliases already present. */
export function contextsToAdd(
  contexts: SshContext[],
  selected: string[],
  hosts: RemoteHost[],
): RemoteHost[] {
  const taken = new Set(hosts.map((h) => h.alias));
  const out: RemoteHost[] = [];
  for (const c of contexts) {
    if (!c.alias || !selected.includes(c.name) || taken.has(c.alias)) continue;
    taken.add(c.alias);
    out.push({ alias: c.alias, display_name: c.name, enabled: true });
  }
  return out;
}

export function DockerContextsList({ contexts, hosts, onAdd }: Props) {
  const [selected, setSelected] = useState<string[]>([]);
  if (contexts.length === 0) return null;
  const aliases = new Set(hosts.map((h) => h.alias));
  const toggle = (name: string, on: boolean) =>
    setSelected((s) => (on ? [...s, name] : s.filter((n) => n !== name)));
  const add = () => {
    onAdd(contextsToAdd(contexts, selected, hosts));
    setSelected([]);
  };

  return (
    <div className="docker-contexts">
      <h4>From Docker contexts</h4>
      <ul className="remote-hosts">
        {contexts.map((c) => (
          <li key={c.name}>
            {c.alias && aliases.has(c.alias) ? (
              <span>
                {c.name} <span className="muted">{c.endpoint} · added</span>
              </span>
            ) : c.alias ? (
              <label className="check">
                <input
                  type="checkbox"
                  checked={selected.includes(c.name)}
                  onChange={(e) => toggle(c.name, e.target.checked)}
                />{" "}
                {c.name} <span className="muted">{c.endpoint}</span>
              </label>
            ) : (
              <span className="muted">
                {c.name} {c.endpoint} · {c.reason}{" "}
                {c.snippet && (
                  <button type="button" onClick={() => void copyText(c.snippet ?? "")}>
                    Copy ~/.ssh/config entry
                  </button>
                )}
              </span>
            )}
          </li>
        ))}
      </ul>
      <button type="button" disabled={selected.length === 0} onClick={add}>
        Add selected
      </button>
    </div>
  );
}
