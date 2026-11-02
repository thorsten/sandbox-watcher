// Each batch of docs/I18N.md renders its text-heaviest components in German
// once; the English cases stay in the component's own test file.
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
import { ContainerActions } from "../ContainerActions";
import { DetailPane } from "../DetailPane";
import { FilesList } from "../FilesList";
import { HistoryView } from "../HistoryView";
import { NetworkPanel } from "../NetworkPanel";
import { RemoteHostsSection } from "../RemoteHostsSection";
import { RuntimeBanner } from "../RuntimeBanner";
import { SettingsPanel } from "../SettingsPanel";
import { Sidebar } from "../Sidebar";
import { StopAllButton } from "../StopAllButton";
import { TurnsList } from "../TurnsSection";
import { UpdateBanner } from "../UpdateBanner";

const noop = vi.fn<() => void>();
const now = new Date("2026-10-30T12:00:00Z");
const de = { language: "de" as const };

describe("German rendering", () => {
  it("sidebar", () => {
    const running = container({
      id: "a",
      started_at: "2026-10-30T11:15:00Z",
      sessions: [session()],
    });
    const stopped = container({ id: "b", project_slug: "other", state: "exited", sessions: [] });
    const { text } = render(
      <Sidebar
        containers={[running, stopped]}
        runtime={connected(["docker"])}
        hosts={[]}
        network={undefined}
        selectedId={null}
        onSelect={noop}
        now={now}
      />,
      de,
    );
    expect(text).toContain("Seit 45 Minuten");
    expect(text).toContain("Gestoppt");
    expect(text).toContain("beendet");
    expect(text).not.toMatch(/\bexited\b|\bStopped\b|\bUp \d/);

    const empty = render(
      <Sidebar
        containers={[]}
        runtime={connected(["docker"])}
        hosts={[]}
        network={undefined}
        selectedId={null}
        onSelect={noop}
        now={now}
      />,
      de,
    );
    expect(empty.text).toContain("Keine Sandbox-Container. Starte einen mit sandbox run claude");
  });

  it("runtime banner", () => {
    const notDetected = render(<RuntimeBanner runtime={{ status: "not-detected" }} />, de);
    expect(notDetected.text).toContain("Keine Container-Runtime gefunden.");
    expect(notDetected.html).toContain("<code>DOCKER_HOST</code>");
    const unavailable = render(
      <RuntimeBanner
        runtime={{ status: "unavailable", candidates: [], reason: "permission denied" }}
      />,
      de,
    );
    expect(unavailable.text).toContain("Runtime erkannt, aber nicht erreichbar.");
    expect(unavailable.text).toContain("permission denied");
  });

  it("container actions and stop all", () => {
    const c = container({ id: "a", sessions: [session()] });
    const actions = render(<ContainerActions container={c} onChanged={noop} />, de);
    for (const label of ["Terminal", "Ordner", "Editor", "Kopieren", "Stoppen"]) {
      expect(actions.text).toContain(label);
    }
    expect(actions.html).toContain('title="Projektverzeichnis öffnen"');
    const stopAll = render(<StopAllButton running={[c, c]} onChanged={noop} />, de);
    expect(stopAll.text).toBe("Alle stoppen (2)");
  });

  it("update banner", () => {
    const { text } = render(
      <UpdateBanner
        update={{ version: "0.9.0", current_version: "0.8.0", notes: null, date: null }}
        progress={null}
        onDismiss={noop}
      />,
      de,
    );
    expect(text).toContain("Sandbox Watcher 0.9.0 ist verfügbar");
    expect(text).toContain("du hast 0.8.0");
    expect(text).toContain("Installieren und neu starten");
  });

  it("detail pane", () => {
    const c = container({ id: "a", sessions: [session()], started_at: "2026-10-30T10:00:00Z" });
    const { text, html } = render(
      <DetailPane
        container={c}
        runtime={connected(["docker"])}
        report={undefined}
        now={now}
        onChanged={noop}
      />,
      de,
    );
    expect(text).toContain("Seit 2 Stunden");
    for (const tab of ["Aktivität", "Netzwerk", "Ressourcen", "Logs", "Mounts"]) {
      expect(text).toContain(tab);
    }
    expect(html).toContain('aria-label="Container-Details"');
    const gone = render(
      <DetailPane
        container={container({ id: "b", last_seen_at: "2026-10-30T11:55:00Z" })}
        runtime={connected(["docker"])}
        report={undefined}
        now={now}
        onChanged={noop}
      />,
      de,
    );
    expect(gone.text).toContain("Host nicht erreichbar — zuletzt gesehen vor 5 min");
  });

  it("turns and files", () => {
    const turns = render(
      <TurnsList
        report={turnReport({ turns: [turn(), turn({ index: 2 })], truncated: true })}
        projectDir="/p"
        now={NOW}
      />,
      de,
    );
    expect(turns.text).toContain("2 Turns, neueste zuerst · ältere Turns nicht gezeigt");
    expect(turns.text).toContain(
      "3 min · 1,5k Tokens · ≈0,01 $ · 3 Tool-Aufrufe · 1 Datei geändert",
    );
    const files = render(
      <FilesList
        files={[file({ edits: 2, reads: 1 }), file({ path: "/p/x" })]}
        projectDir="/p"
        now={NOW}
      />,
      de,
    );
    expect(files.text).toContain("1 geändert · 1 gelesen");
    expect(files.text).toContain("2× bearbeitet · 1× gelesen");
    expect(files.text).toContain("1 gelesene zeigen");
  });

  it("network panel", () => {
    const r = report(
      [entry({ destination: "evil.example", status: "blocked" }), entry({ status: "allowed" })],
      { failed_sources: ["dns", "firewall"] },
    );
    const { text, html } = render(<NetworkPanel containerKey="c1" report={r} now={NOW} />, de);
    expect(text).toContain("Alle 2");
    expect(text).toContain("Blockiert 1");
    expect(text).toContain("blockiert");
    expect(text).toContain("erlaubt");
    expect(text).toContain("DNS-Log, Firewall-Log konnten nicht gelesen werden.");
    expect(html).toContain('placeholder="Nach Host, IP oder Port filtern"');
    expect(html).toContain('aria-label="evil.example auswählen"');
  });

  it("settings panel", () => {
    const sections = ["general", "runtime", "actions", "notifications", "history"] as const;
    const texts = sections.map(
      (section) =>
        render(
          <SettingsPanel
            settings={settings({ language: "de" })}
            initialSection={section}
            onSaved={noop}
            onClose={noop}
          />,
          de,
        ).text,
    );
    const all = texts.join(" ");
    for (const label of [
      "Allgemein",
      "Darstellung",
      "Aktualisierung",
      "Aktionen",
      "Benachrichtigungen",
      "Verlauf und Kosten",
      "Updates",
      "Fenster",
      "Diagnose",
      "Remote-Hosts",
    ]) {
      expect(all).toContain(label);
    }
    expect(all).not.toContain("Ungespeicherte");
    expect(all).toContain("Vor dem Stoppen von Containern nachfragen");
    expect(all).toContain("Sandbox gestoppt, während Sessions liefen");
    expect(all).toContain("Speichern");
    expect(all).not.toMatch(/\bSettings\b|\bSave\b|Notifications/);
    const general = render(
      <SettingsPanel settings={settings({ language: "de" })} onSaved={noop} onClose={noop} />,
      de,
    );
    expect(general.html).toContain('<option value="de" selected="">Deutsch</option>');
    expect(general.html).toContain('aria-label="Bereiche der Einstellungen"');
  });

  it("remote hosts", () => {
    const { text } = render(
      <RemoteHostsSection
        hosts={[{ alias: "devbox", display_name: "Dev VM", enabled: true }]}
        statuses={[
          {
            state: "unreachable",
            reason: "ssh: connection refused",
            since: NOW.toISOString(),
            alias: "devbox",
            display_name: "Dev VM",
            runtimes: [],
            probes: [],
          },
        ]}
        onChange={noop}
      />,
      de,
    );
    expect(text).toContain("Dev VM nicht erreichbar Verbindung testen Entfernen");
    expect(text).toContain("führe zuerst einmal ssh <alias> aus");
  });

  it("history view", () => {
    const { text, html } = render(<HistoryView onClose={noop} />, de);
    expect(text).toContain("Verlauf Leeren Schließen Heute 7 Tage 30 Tage Alle");
    expect(text).toContain("Alle Projekte");
    expect(text).toContain("Alle Agenten");
    expect(html).toContain('aria-label="Zeitraum"');
  });
});
