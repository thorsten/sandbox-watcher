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
};
