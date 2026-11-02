import type { Dictionary } from "./dictionary";

/** German. The user is addressed as "du"; Session, Turn and Prompt stay as they are. */
export const de: Dictionary = {
  "common.unknown": "unbekannt",
  "common.thisMachine": "Dieser Rechner",
  "common.thisMac": "Dieser Mac",

  "uptime.seconds": { one: "Seit {n} Sekunde", other: "Seit {n} Sekunden" },
  "uptime.minutes": { one: "Seit {n} Minute", other: "Seit {n} Minuten" },
  "uptime.hours": { one: "Seit {n} Stunde", other: "Seit {n} Stunden" },
  "uptime.days": { one: "Seit {n} Tag", other: "Seit {n} Tagen" },

  "ago.seconds": "vor {n} s",
  "ago.minutes": "vor {n} min",
  "ago.hours": "vor {n} h",
  "ago.days": "vor {n} d",

  "duration.seconds": "{n} s",
  "duration.minutes": "{n} min",
  "duration.hoursMinutes": "{h} h {m} min",
  "duration.daysHours": "{d} d {h} h",

  "cost.noPrice": "kein Preis",

  "stop.noSessions": "{name}: keine aktiven Sessions",
  "stop.interrupts": {
    one: "{name}: {n} Session wird unterbrochen: {sessions}",
    other: "{name}: {n} Sessions werden unterbrochen: {sessions}",
  },
  "stop.stopped": { one: "{n} Container gestoppt", other: "{n} Container gestoppt" },
  "stop.stoppedPart": "{n} gestoppt",
  "stop.failedPart": { one: "{n} fehlgeschlagen: {list}", other: "{n} fehlgeschlagen: {list}" },
  "stop.couldNotListLocal":
    "{runtime}-Container auf diesem Rechner konnten nicht aufgelistet werden: {error}",
  "stop.couldNotListHost":
    "{runtime}-Container auf {host} konnten nicht aufgelistet werden: {error}",
};
