/**
 * English texts, the source of truth for every other language: `de.ts` is
 * typed as this object, so a missing or extra key fails `tsc`.
 *
 * Keys are `area.name`. A value is a sentence with `{param}` placeholders,
 * or `{ one, other }` forms picked by `Intl.PluralRules` from the `n`
 * parameter. Whole sentences only; never assemble text from fragments,
 * word order differs between languages.
 */
export const en = {
  "common.unknown": "unknown",
  "common.thisMachine": "This machine",
  "common.thisMac": "This Mac",

  // Container uptime, the same buckets as `sandbox status`.
  "uptime.seconds": { one: "Up {n} second", other: "Up {n} seconds" },
  "uptime.minutes": { one: "Up {n} minute", other: "Up {n} minutes" },
  "uptime.hours": { one: "Up {n} hour", other: "Up {n} hours" },
  "uptime.days": { one: "Up {n} day", other: "Up {n} days" },

  // Compact "how long ago" for lists.
  "ago.seconds": "{n}s ago",
  "ago.minutes": "{n}m ago",
  "ago.hours": "{n}h ago",
  "ago.days": "{n}d ago",

  // Compact durations for sessions and turns.
  "duration.seconds": "{n}s",
  "duration.minutes": "{n}m",
  "duration.hoursMinutes": "{h}h {m}m",
  "duration.daysHours": "{d}d {h}h",

  "cost.noPrice": "no price",

  // Stop confirmation and the "Stop all" notice.
  "stop.noSessions": "{name}: no active sessions",
  "stop.interrupts": {
    one: "{name}: {n} session will be interrupted: {sessions}",
    other: "{name}: {n} sessions will be interrupted: {sessions}",
  },
  "stop.stopped": { one: "Stopped {n} container", other: "Stopped {n} containers" },
  "stop.stoppedPart": "stopped {n}",
  "stop.failedPart": { one: "{n} failed: {list}", other: "{n} failed: {list}" },
  "stop.couldNotListLocal": "could not list {runtime} containers on this machine: {error}",
  "stop.couldNotListHost": "could not list {runtime} containers on {host}: {error}",

  // Container states as `docker ps` reports them.
  "state.created": "created",
  "state.running": "running",
  "state.paused": "paused",
  "state.stopping": "stopping",
  "state.exited": "exited",
  "state.dead": "dead",
  "state.unknown": "unknown",

  // App shell: header, footer, empty and error states.
  "app.summary": "Summary",
  "app.stat.projects": "projects",
  "app.stat.running": "running",
  "app.stat.sessions": "sessions",
  "app.blockedRecentTitle": "Blocked requests in the last five minutes",
  "app.blockedCount": "{n} blocked",
  "app.mockData": "mock data",
  "app.refresh": "Refresh",
  "app.refreshing": "Refreshing…",
  "app.history": "History",
  "app.historyTitle": "Session history",
  "app.settings": "Settings",
  "app.looking": "Looking for container runtimes…",
  "app.loadFailed": "Could not load overview.",
  "app.emptyTitle": "No sandbox containers.",
  "app.emptyHint": "Start one with {command} in a project and it appears on the left.",
  "app.footer": "Tested against sandbox {version}. Refreshed {time}.",

  // Sidebar.
  "sidebar.hostFilter": "Host filter",
  "sidebar.allHosts": "All hosts",
  "sidebar.empty": "No sandbox containers. Start one with {command}.",
  "sidebar.stopped": "Stopped",
  "sidebar.lastSeen": "last seen {when}",
  "sidebar.idle": "idle",
  "sidebar.starting": "starting",
  "sidebar.stale": "stale",

  // Runtime banner.
  "runtime.onHost": "{runtime} on {host}",
  "runtime.probes": "Probes",
  "runtime.diagnostics": "Diagnostics",
  "runtime.notDetectedTitle": "No container runtime detected.",
  "runtime.notDetectedBody":
    "Sandbox Watcher looks for Docker, Podman and Apple {container} in the same order sandbox does. Install one, or set {dockerHost} if your socket lives somewhere unusual, then press Refresh.",
  "runtime.detectedOnlyTitle": "Runtime found, but could not connect.",
  "runtime.at": "{runtime} at {endpoint}",
  "runtime.detectedOnlyBody": "Docker, Podman and Apple {container} are supported.",
  "runtime.unavailableTitle": "Runtime detected but not reachable.",
  "runtime.unavailableBody":
    "Is the daemon running? On Linux, check that your user may access the socket.",
  "runtime.via": "via {endpoint}",
  "runtime.failure": "{runtime} detected but not reachable: {reason}",

  // Update banner.
  "update.available": "Sandbox Watcher {version} is available",
  "update.current": "you have {version}",
  "update.downloading": "Downloading…",
  "update.downloadingPercent": "Downloading… {percent}%",
  "update.restarts": "The app restarts when done.",
  "update.later": "Later",
  "update.installing": "Installing…",
  "update.install": "Install and restart",

  // Stop buttons and the confirmation dialog.
  "stop.stopping": "Stopping…",
  "stop.all": "Stop all",
  "stop.allCount": "Stop all ({n})",
  "stop.allTitle": {
    one: "Stop the {n} running sandbox container?",
    other: "Stop all {n} running sandbox containers?",
  },
  "confirm.dontAsk": "Don't ask again",
  "confirm.cancel": "Cancel",

  // Container actions.
  "actions.stoppedNotice": "Stopped",
  "actions.noProjectDir": "No bind-mounted project directory",
  "actions.copied": "Copied",
  "actions.clipboardUnavailable": "Clipboard unavailable",
  "actions.remoteTitle": "Not available for remote hosts",
  "actions.remoteHost": "remote host",
  "actions.terminal": "Terminal",
  "actions.terminalTitle": "Open a terminal in the project directory",
  "actions.folder": "Folder",
  "actions.folderTitle": "Open the project directory",
  "actions.editor": "Editor",
  "actions.editorTitle": "Open the project in your editor",
  "actions.copy": "Copy ▾",
  "actions.copyName": "Container name",
  "actions.copyPath": "Project path",
  "actions.copyNetwork": "{command} command",
  "actions.stop": "Stop",
  "actions.stopTitle": "Stop {name}?",
  "actions.stopContainer": "Stop container",
};
