import { useEffect, useState } from "react";
import { getContainerDetails, revealMount } from "../api";
import { t, tx } from "../i18n";
import type { ContainerDetails, MountRole } from "../types";

const ROLE_ORDER: MountRole[] = ["project", "persisted", "other", "runtime"];

export function MountsPanel({ containerKey }: { containerKey: string }) {
  const [details, setDetails] = useState<ContainerDetails | null | undefined>(undefined);
  const [error, setError] = useState<string | null>(null);
  const remote = containerKey.includes("/");

  useEffect(() => {
    let cancelled = false;
    getContainerDetails(containerKey).then(
      (d) => {
        if (!cancelled) setDetails(d);
      },
      (e) => {
        if (!cancelled) setError(e instanceof Error ? e.message : String(e));
      },
    );
    return () => {
      cancelled = true;
    };
  }, [containerKey]);

  if (error) return <p className="network-note warn">{error}</p>;
  if (details === undefined) return <div className="mounts muted">{t("mounts.inspecting")}</div>;
  if (details === null) return <div className="mounts muted">{t("mounts.gone")}</div>;

  const mounts = [...details.mounts].toSorted(
    (a, b) => ROLE_ORDER.indexOf(a.role) - ROLE_ORDER.indexOf(b.role),
  );

  return (
    <div className="mounts">
      <table className="network-table">
        <thead>
          <tr>
            <th>{t("mounts.role")}</th>
            <th>{t("mounts.hostPath")}</th>
            <th>{t("mounts.inContainer")}</th>
            <th>{t("mounts.access")}</th>
            <th aria-label={t("mounts.actions")} />
          </tr>
        </thead>
        <tbody>
          {mounts.map((m) => (
            <tr key={`${m.source}:${m.destination}`}>
              <td>
                <span className={`chip role-${m.role}`}>{t(`mount.${m.role}`)}</span>
              </td>
              <td className="path" title={m.source}>
                {m.type === "volume" ? (
                  <span>{tx("mounts.volume", { name: <code>{m.source}</code> })}</span>
                ) : (
                  <code>{m.host_path ?? m.source}</code>
                )}
              </td>
              <td className="path">
                <code>{m.destination}</code>
              </td>
              <td>{m.read_only ? t("mounts.readOnly") : t("mounts.writable")}</td>
              <td>
                {m.type === "bind" && !remote && (
                  <button
                    type="button"
                    className="link"
                    onClick={() => void revealMount(containerKey, m.source).catch(() => undefined)}
                  >
                    {t("mounts.show")}
                  </button>
                )}
              </td>
            </tr>
          ))}
        </tbody>
      </table>

      <dl className="kv">
        <dt>{t("mounts.workingDir")}</dt>
        <dd>
          <code>{details.working_dir ?? "–"}</code>
        </dd>
        <dt>{t("mounts.user")}</dt>
        <dd>{details.user ?? "–"}</dd>
        <dt>{t("mounts.ports")}</dt>
        <dd>
          {details.ports.length === 0
            ? t("mounts.none")
            : details.ports
                .map((p) =>
                  p.host_port
                    ? `${p.host_port} → ${p.container_port}/${p.protocol}`
                    : t("mounts.notPublished", { port: `${p.container_port}/${p.protocol}` }),
                )
                .join(", ")}
        </dd>
        <dt>{t("mounts.envNames")}</dt>
        <dd className="env-names">
          {details.env_names.length === 0
            ? t("mounts.none")
            : details.env_names.map((n) => <code key={n}>{n}</code>)}
          <span className="muted"> {t("mounts.valuesNeverRead")}</span>
        </dd>
      </dl>
    </div>
  );
}
