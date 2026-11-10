// Renders every component once in German with the fixtures and fails on
// English words that are easy to leave behind. The English tests stay in
// each component's own file; this only catches strings that never got a key.
import type { ReactElement } from "react";
import { describe, expect, it, vi } from "vitest";
import {
  NOW,
  connected,
  container,
  entry,
  file,
  report,
  session,
  settings,
  turn,
  turnReport,
} from "../../test/fixtures";
import { render } from "../../test/render";
import type { DiagnosticsReport, HostStatus, RemoteHost, SshContext } from "../../types";
import { ActivityPanel } from "../ActivityPanel";
import { AllowDialog } from "../AllowDialog";
import { ConfirmDialog } from "../ConfirmDialog";
import { ContainerActions } from "../ContainerActions";
import { DetailPane } from "../DetailPane";
import { DiagnosticsReportView, DiagnosticsSection } from "../DiagnosticsSection";
import { DockerContextsList } from "../DockerContextsList";
import { FilesList } from "../FilesList";
import { HistoryView } from "../HistoryView";
import { LogsPanel } from "../LogsPanel";
import { MountsPanel } from "../MountsPanel";
import { NetworkPanel } from "../NetworkPanel";
import { RemoteHostsSection } from "../RemoteHostsSection";
import { ResourcesPanel } from "../ResourcesPanel";
import { RuntimeBanner } from "../RuntimeBanner";
import { SettingsPanel } from "../SettingsPanel";
import { Sidebar } from "../Sidebar";
import { Sparkline } from "../Sparkline";
import { StopAllButton } from "../StopAllButton";
import { TurnsList, TurnsSection } from "../TurnsSection";
import { UpdateBanner } from "../UpdateBanner";
import { UpdateCheckNotice } from "../UpdateCheckNotice";

const noop = vi.fn<() => void>();
const de = { language: "de" as const };

/** Words that only appear in the English dictionary. Product names and commands are not listed. */
const ENGLISH =
  /\b(Stop|Settings|Refresh|Close|Cancel|Show|Hide|Nothing|Running|Stopped|Install|Later|Dismiss|latest version|Follow|Remove|Save|Clear|History|Looking|Reading|Checking|unknown|last seen|not set|No active|No sandbox|will be interrupted|is available|you have|Select|Destination|Count|Window|Role|Access|Ports|Write to|Entries|Done|Environment|Outcome|Candidate|Started|Duration|Agent time|By agent|By project|Today|days|Allow blocked|minutes|hours|seconds|ago|turns?|files? changed|tool calls?|session|sessions)\b/;

const unreachable: HostStatus = {
  state: "unreachable",
  reason: "ssh: connection refused",
  since: NOW.toISOString(),
  alias: "devbox",
  display_name: "Dev VM",
  runtimes: [],
  probes: ["docker.sock missing"],
};
const host: RemoteHost = { alias: "devbox", display_name: "Dev VM", enabled: true };
const ctx = (name: string, alias: string | null): SshContext => ({
  name,
  endpoint: `ssh://${name}`,
  alias,
  reason: alias ? null : "uses port 2222",
  snippet: alias ? null : `Host ${name}\n  Port 2222\n`,
});
const diagnostics: DiagnosticsReport = {
  generated_at: NOW.toISOString(),
  app_version: "0.9.0",
  environment: {
    os: "linux",
    docker_host: null,
    container_host: null,
    runtime_override: null,
    config_runtime: "docker",
    config_path: "",
    binaries: [{ name: "docker", found: true }],
  },
  local: [
    { kind: "podman", candidate: "CONTAINER_HOST", outcome: "missing" },
    { kind: "docker", candidate: "/var/run/docker.sock", outcome: "found", endpoint: "/x" },
    { kind: "docker", candidate: "connect", outcome: "failed", reason: "permission denied" },
    { kind: "apple-container", candidate: "container", outcome: "skipped", reason: "not macOS" },
  ],
  hosts: [{ alias: "devbox", status: unreachable }],
};

const running = container({
  id: "a",
  sessions: [session()],
  started_at: "2026-10-30T10:00:00Z",
  config_hash: "abc",
});
const remote = container({ id: "b", key: "devbox/b", host: "devbox", sessions: [] });
const gone = container({ id: "c", last_seen_at: "2026-10-30T11:55:00Z" });
const stopped = container({ id: "d", state: "exited", project_slug: "other", sessions: [] });
const runtime = connected(["docker", "podman"]);
const network = report(
  [entry({ destination: "evil.example", status: "blocked" }), entry({ status: "allowed" })],
  { failed_sources: ["dns"], unparsed_sources: ["firewall", "proxy-access"] },
);

const trees: [string, () => ReactElement][] = [
  [
    "Sidebar",
    () => (
      <Sidebar
        containers={[running, remote, gone, stopped]}
        runtime={runtime}
        hosts={[unreachable]}
        network={[network]}
        selectedId="a"
        onSelect={noop}
        now={NOW}
      />
    ),
  ],
  [
    "Sidebar empty",
    () => (
      <Sidebar
        containers={[]}
        runtime={runtime}
        hosts={[]}
        network={undefined}
        selectedId={null}
        onSelect={noop}
        now={NOW}
      />
    ),
  ],
  [
    "RuntimeBanner not-detected",
    () => <RuntimeBanner runtime={{ status: "not-detected" }} onDiagnostics={noop} />,
  ],
  [
    "RuntimeBanner detected-only",
    () => (
      <RuntimeBanner
        runtime={{
          status: "detected-only",
          candidates: [{ kind: "docker", endpoint: "/var/run/docker.sock", source: "default" }],
        }}
      />
    ),
  ],
  [
    "RuntimeBanner unavailable",
    () => <RuntimeBanner runtime={{ status: "unavailable", candidates: [], reason: "denied" }} />,
  ],
  ["RuntimeBanner connected", () => <RuntimeBanner runtime={runtime} hosts={[unreachable]} />],
  [
    "RuntimeBanner compact",
    () => <RuntimeBanner runtime={runtime} hosts={[unreachable]} compact />,
  ],
  [
    "UpdateBanner",
    () => (
      <UpdateBanner
        update={{ version: "1.0.0", current_version: "0.9.0", notes: "- x", date: null }}
        progress={{ downloaded: 1, total: 2 }}
        onDismiss={noop}
      />
    ),
  ],
  [
    "UpdateCheckNotice up to date",
    () => (
      <UpdateCheckNotice
        check={{ outcome: "up-to-date", current_version: "0.9.0" }}
        onDismiss={noop}
      />
    ),
  ],
  [
    "UpdateCheckNotice failed",
    () => (
      <UpdateCheckNotice
        check={{ outcome: "failed", error: { code: "update-check-failed", detail: "x" } }}
        onDismiss={noop}
      />
    ),
  ],
  [
    "UpdateCheckNotice checking",
    () => <UpdateCheckNotice check={{ outcome: "checking" }} onDismiss={noop} />,
  ],
  ["StopAllButton", () => <StopAllButton running={[running]} onChanged={noop} />],
  [
    "ConfirmDialog",
    () => (
      <ConfirmDialog title="T" lines={["l"]} confirmLabel="C" onConfirm={noop} onCancel={noop} />
    ),
  ],
  ["ContainerActions", () => <ContainerActions container={running} onChanged={noop} />],
  ["ContainerActions remote", () => <ContainerActions container={remote} onChanged={noop} />],
  [
    "DetailPane",
    () => (
      <DetailPane
        container={running}
        runtime={runtime}
        report={network}
        now={NOW}
        onChanged={noop}
      />
    ),
  ],
  [
    "DetailPane gone",
    () => (
      <DetailPane
        container={gone}
        runtime={runtime}
        report={undefined}
        now={NOW}
        onChanged={noop}
      />
    ),
  ],
  [
    "DetailPane stopped",
    () => (
      <DetailPane
        container={stopped}
        runtime={runtime}
        report={undefined}
        now={NOW}
        onChanged={noop}
      />
    ),
  ],
  ["ActivityPanel", () => <ActivityPanel containerKey="a" now={NOW} />],
  [
    "FilesList",
    () => (
      <FilesList
        files={[
          file({ edits: 2, reads: 1 }),
          file({ path: "/p/r", created: true }),
          file({ path: "/p/d", deleted: true }),
          file({ path: "/p/x" }),
        ]}
        containerKey="a"
        projectDir="/p"
        now={NOW}
      />
    ),
  ],
  ["FilesList empty", () => <FilesList files={[]} projectDir={null} now={NOW} />],
  [
    "TurnsSection",
    () => <TurnsSection containerKey="a" agent="claude-code" projectDir="/p" now={NOW} />,
  ],
  [
    "TurnsList",
    () => (
      <TurnsList
        report={turnReport({
          turns: [
            turn({ index: 0, prompt: "", usage: null, tool_calls: 0, files: [] }),
            turn({ tool_errors: 1 }),
          ],
          truncated: true,
        })}
        projectDir="/p"
        now={NOW}
      />
    ),
  ],
  [
    "TurnsList empty",
    () => <TurnsList report={turnReport({ turns: [] })} projectDir="/p" now={NOW} />,
  ],
  ["NetworkPanel", () => <NetworkPanel containerKey="a" report={network} now={NOW} />],
  ["NetworkPanel loading", () => <NetworkPanel containerKey="a" report={undefined} now={NOW} />],
  ["ResourcesPanel", () => <ResourcesPanel containerKey="a" />],
  ["Sparkline", () => <Sparkline title="CPU" values={[]} format={String} current="" />],
  ["LogsPanel", () => <LogsPanel containerKey="a" />],
  ["MountsPanel", () => <MountsPanel containerKey="a" />],
  ["AllowDialog", () => <AllowDialog containerKey="a" selections={[]} onClose={noop} />],
  ...(["general", "runtime", "actions", "notifications", "history", "diagnostics"] as const).map(
    (section): [string, () => ReactElement] => [
      `SettingsPanel ${section}`,
      () => (
        <SettingsPanel
          settings={settings({
            terminal: { kind: "custom", command: "x" },
            editor: { kind: "custom", command: "y" },
            remote_hosts: [host],
          })}
          initialSection={section}
          hostStatuses={[unreachable]}
          onSaved={noop}
          onClose={noop}
        />
      ),
    ],
  ),
  ["RemoteHostsSection", () => <RemoteHostsSection hosts={[host]} statuses={[]} onChange={noop} />],
  [
    "DockerContextsList",
    () => (
      <DockerContextsList
        contexts={[ctx("staging", "dev@staging"), ctx("vm", null), ctx("devbox", "devbox")]}
        hosts={[host]}
        onAdd={noop}
      />
    ),
  ],
  ["DiagnosticsSection", () => <DiagnosticsSection initialReport={diagnostics} />],
  ["DiagnosticsReportView", () => <DiagnosticsReportView report={diagnostics} />],
  ["HistoryView", () => <HistoryView onClose={noop} />],
];

describe("German guard", () => {
  it.each(trees)("%s has no English left", (_name, tree) => {
    const { text } = render(tree(), de);
    const hit = ENGLISH.exec(text);
    expect(hit ? `${hit[0]} in: ${text}` : null).toBeNull();
  });
});
