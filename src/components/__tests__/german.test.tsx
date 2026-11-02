// Each batch of docs/I18N.md renders its text-heaviest components in German
// once; the English cases stay in the component's own test file.
import { describe, expect, it, vi } from "vitest";
import { connected, container, session } from "../../test/fixtures";
import { render } from "../../test/render";
import { ContainerActions } from "../ContainerActions";
import { RuntimeBanner } from "../RuntimeBanner";
import { Sidebar } from "../Sidebar";
import { StopAllButton } from "../StopAllButton";
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
});
